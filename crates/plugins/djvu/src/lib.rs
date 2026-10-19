//! `DjVu` documents, scanned books mostly, in the document reader, and their
//! first pages as thumbnails: read by `DjVuLibre` where it is installed, else
//! by djvu-rs, whose helper every package carries; or by the one the `DjVu`
//! engine setting names (ADR-061).
use skwir_core::{
    App, CancelToken,
    document::DocumentSession,
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
    thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider},
};
use skwir_platform::document::{self, Engine};
use skwir_platform::mime;
use std::sync::Arc;

const ID: &str = "djvu";
/// Which engine reads `DjVu` documents.
const ENGINE: &str = "djvu.engine";
/// The reader, which shows a document's pages.
const READER: &str = "djvu";

pub struct PreviewPlugin;

impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "DjVu"
    }
    fn description(&self) -> &'static str {
        "Read DjVu documents, scanned books mostly: continuous scrolling, search, text selection, links and contents; and show their first pages as thumbnails"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    /// A document open in the reader starts again with the engine chosen.
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|key| key == ENGINE) {
            app.previews.unregister(ID, READER);
            register_reader(app);
        }
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        vec![
            document::worker(Engine::Djvulibre).requirement("DjVu reading with DjVuLibre"),
            document::worker(Engine::DjvuRs).requirement("DjVu reading with djvu-rs"),
        ]
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read, Capability::Programs])
    }
    fn register(&self, app: &App) {
        app.settings.declare(skwir_core::settings::SettingSpec::choice(
            ENGINE,
            "DjVu engine",
            "Automatic prefers DjVuLibre, where it is installed, and falls back to djvu-rs, which skwir carries.",
            "auto",
            &[
                ("auto", "Automatic"),
                ("djvulibre", "DjVuLibre"),
                ("djvu-rs", "djvu-rs"),
            ],
        )
        .page(skwir_core::settings::page::READING)
        .section("Documents"));
        register_reader(app);
        // A fallback stage: the desktop's cache and thumbnailers come first.
        app.thumbnails.register(ID, Arc::new(FirstPages));
    }
}

/// `DjVu` documents, by type: one page ("`DjVu` image") or several.
static DJVU: mime::Types = mime::Types::new(&["image/vnd.djvu", "image/vnd.djvu+multipage"]);

/// The engines the setting `choice` names, in the order they are tried.
fn engines(choice: Option<&str>) -> &'static [Engine] {
    match choice {
        Some("djvulibre") => &[Engine::Djvulibre],
        Some("djvu-rs") => &[Engine::DjvuRs],
        _ => &document::DJVU_AUTO,
    }
}

/// The reader, with the engines the settings choose.
fn register_reader(app: &App) {
    let choice = app.settings.get::<String>(ENGINE);
    app.previews.register(
        ID,
        Arc::new(Reader {
            engines: engines(choice.as_deref()),
        }),
    );
}

struct Reader {
    engines: &'static [Engine],
}

impl PreviewProvider for Reader {
    fn id(&self) -> &'static str {
        READER
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        DJVU.holds(&req.entry.path, req.mime.as_deref())
    }
    /// A document of pages: read continuously or a page at a time, each
    /// page drawn at the side it is shown at.
    fn open_document(
        &self,
        req: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn DocumentSession>>> {
        let path = req.entry.resolve()?;
        if let Some(session) = document::open(&path, self.engines, lifetime)? {
            return Ok(Some(session));
        }
        // What is missing, in the words every helper's absence is said in.
        let helpers: Vec<&str> = self
            .engines
            .iter()
            .map(|engine| document::worker(*engine).executable)
            .collect();
        match self.engines {
            [engine] => anyhow::bail!(
                "Install {} to read DjVu documents with {}",
                helpers[0],
                engine.label()
            ),
            _ => anyhow::bail!("Install {} to read DjVu documents", helpers.join(" or ")),
        }
    }
}

/// A document's first page, for the grid. A fallback: a thumbnailer the
/// desktop installed for `DjVu` comes first; drawn by an engine in
/// `auto`'s order, whatever the reader's, as a PDF's first page is.
struct FirstPages;

impl ThumbnailProvider for FirstPages {
    fn id(&self) -> &'static str {
        "djvu-pages"
    }
    fn stage(&self) -> Stage {
        Stage::Fallback
    }
    fn accepts(&self, req: &ThumbRequest) -> bool {
        DJVU.holds(&req.entry.path, req.mime.as_deref())
    }
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let path = req.entry.resolve()?;
        skwir_platform::thumb_providers::ensure_file(&path)?;
        document::first_page(&path, req.size.pixels(), &document::DJVU_AUTO, None, cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_names_the_engines_tried() {
        assert!(matches!(engines(Some("djvulibre")), [Engine::Djvulibre]));
        assert!(matches!(engines(Some("djvu-rs")), [Engine::DjvuRs]));
        // Automatic, and anything the setting should not hold.
        for choice in [None, Some("auto"), Some("other")] {
            assert!(matches!(
                engines(choice),
                [Engine::Djvulibre, Engine::DjvuRs]
            ));
        }
    }
}
