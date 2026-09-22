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
    fn requirements(&self) -> Vec<bibli_core::plugin::Requirement> {
        vec![
            bibli_core::plugin::Requirement::new(
                "bibli-archive-worker",
                "Reading the pages of a comic book",
                || archive::worker_path().is_some(),
            )
            .required(),
        ]
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
            .members
            .into_iter()
            .filter(is_page)
            .collect();
        pages.sort_by_cached_key(|m| bibli_core::collate::sort_key(m.name.as_ref()));
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
/// A picture the book shows: a regular, readable image, and not something a
/// tool left beside the pages. Archives made on a Mac carry an `AppleDouble`
/// twin of every page (`__MACOSX/._page01.jpg`) that sorts first and is no
/// picture at all; other dotfiles are thumbnails and caches.
fn is_page(member: &archive::Member) -> bool {
    member.regular
        && !member.encrypted
        && !member
            .name
            .split('/')
            .any(|part| part.starts_with('.') || part == "__MACOSX")
        && matches!(
            inspection::extension(std::path::Path::new(&member.name)).as_str(),
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
        )
}
#[cfg(test)]
mod tests {
    use super::*;

    fn member(index: u64, name: &str, encrypted: bool) -> archive::Member {
        archive::Member {
            index,
            name: name.into(),
            size: 1,
            directory: false,
            regular: true,
            encrypted,
        }
    }

    #[test]
    fn pages_sort_numerically_without_what_tools_left_beside_them() {
        let mut pages: Vec<_> = [
            member(0, "__MACOSX/._page01.jpg", false),
            member(1, "Book/page10.png", false),
            member(2, "Book/page2.png", false),
            member(3, "Book/.thumbs/page1.png", false),
            member(4, "Book/._page1.png", false),
            member(5, "Book/page1.png", false),
            member(6, "Book/secret.png", true),
            member(7, "Book/notes.txt", false),
        ]
        .into_iter()
        .filter(is_page)
        .collect();
        pages.sort_by_cached_key(|m| bibli_core::collate::sort_key(m.name.as_ref()));
        let names: Vec<_> = pages.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["Book/page1.png", "Book/page2.png", "Book/page10.png"]
        );
    }
}
