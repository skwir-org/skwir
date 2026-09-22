//! CBZ/CBR comic pages, read through libarchive without whole-archive extraction.
use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::{archive, inspection};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-comics"
    }
    fn title(&self) -> &'static str {
        "Preview: comics"
    }
    fn description(&self) -> &'static str {
        "Read CBZ and CBR comic books page by page"
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-comics"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        matches!(
            inspection::extension(&req.entry.path).as_str(),
            "cbz" | "cbr" | "cb7" | "cbt"
        )
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let path = req.entry.resolve()?;
        let mut pages: Vec<_> = archive::list(&path, cancel)?
            .into_iter()
            .filter(|m| {
                m.regular
                    && matches!(
                        inspection::extension(std::path::Path::new(&m.name)).as_str(),
                        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
                    )
            })
            .collect();
        pages.sort_by_cached_key(|m| natural_key(&m.name));
        anyhow::ensure!(!pages.is_empty(), "Comic contains no supported images");
        let index = view.offset.min(pages.len() as u64 - 1) as usize;
        let page = &pages[index];
        let image = inspection::image(&archive::read(&path, page, cancel)?)?;
        Ok(Some(Inspection {
            kind: bibli_core::inspection::InspectionKind::Comic,
            title: format!("Comic · {} / {}", index + 1, pages.len()),
            notice: page.name.clone(),
            image: Some(image),
            previous: (index > 0).then(|| index as u64 - 1),
            next: (index + 1 < pages.len()).then_some(index as u64 + 1),
            position: index as u64,
            ..Inspection::default()
        }))
    }
}
fn natural_key(name: &str) -> String {
    use std::fmt::Write;
    let mut key = String::new();
    let mut digits = String::new();
    for c in name.to_lowercase().chars().chain(std::iter::once('\0')) {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            if !digits.is_empty() {
                let number = digits.trim_start_matches('0');
                let _ = write!(key, "{:08}{}", number.len(), number);
                digits.clear();
            }
            key.push(c);
        }
    }
    key
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pages_sort_numerically() {
        let mut names = ["page10.png", "page2.png", "page1.png"];
        names.sort_by_key(|n| natural_key(n));
        assert_eq!(names, ["page1.png", "page2.png", "page10.png"]);
    }
}
