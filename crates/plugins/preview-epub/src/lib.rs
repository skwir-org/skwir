//! EPUB 2/3 native reading mode: rbook container parsing and semantic XHTML.
use bibli_core::{
    App, CancelToken,
    inspection::{Block, InspectRequest, Inspection, Row},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::{inspection, rich_preview};
use std::sync::Arc;
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-epub"
    }
    fn title(&self) -> &'static str {
        "Preview: EPUB"
    }
    fn register(&self, app: &App) {
        app.previews.register(self.id(), Arc::new(Provider));
    }
}
struct Provider;
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "preview-epub"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        inspection::extension(&req.entry.path) == "epub"
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let path = req.entry.resolve()?;
        // Preflight all declared expansion sizes before rbook reads package XML.
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path)?)?;
        anyhow::ensure!(archive.len() <= 4000, "EPUB exceeds 4000 resources");
        let mut total = 0u64;
        for i in 0..archive.len() {
            inspection::check(cancel)?;
            let file = archive.by_index(i)?;
            total = total.saturating_add(file.size());
            anyhow::ensure!(
                file.size() <= 16 * 1024 * 1024 && total <= 128 * 1024 * 1024,
                "EPUB resource expansion exceeds preview limit"
            );
        }
        drop(archive);
        let book = rbook::Epub::open(path)?;
        let chapters: Vec<_> = book
            .spine()
            .iter()
            .filter_map(|s| s.manifest_entry())
            .collect();
        anyhow::ensure!(!chapters.is_empty(), "EPUB has no readable chapters");
        let mut index = view.offset.min(chapters.len().saturating_sub(1) as u64) as usize;
        let mut notice =
            String::from("Reading mode · publisher CSS and fixed-page layout are not applied");
        if let Some(query) = view.query.as_deref().filter(|q| !q.is_empty()) {
            let query = query.to_lowercase();
            let mut found = false;
            for candidate in (index..chapters.len()).chain(0..index) {
                inspection::check(cancel)?;
                let mut bytes = Limited {
                    bytes: Vec::new(),
                    cancel,
                };
                book.copy_resource(chapters[candidate].resource(), &mut bytes)?;
                let html = String::from_utf8(bytes.bytes)?;
                let blocks = rich_preview::html(&html, cancel, |_| None)?;
                if blocks
                    .iter()
                    .any(|b| b.text.to_lowercase().contains(&query))
                {
                    index = candidate;
                    found = true;
                    break;
                }
            }
            notice = if found {
                "Search match in this chapter".into()
            } else {
                "No matching chapter".into()
            };
        }
        let href = view
            .target
            .clone()
            .filter(|_| view.query.is_none())
            .unwrap_or_else(|| chapters[index].href().as_str().to_owned());
        let index = chapters
            .iter()
            .position(|c| c.href().as_str() == href.split('#').next().unwrap_or(&href))
            .unwrap_or(index);
        let mut bytes = Limited {
            bytes: Vec::new(),
            cancel,
        };
        book.copy_resource(href.as_str(), &mut bytes)?;
        let source = String::from_utf8(bytes.bytes)?;
        let base = href.rsplit_once('/').map_or("", |(base, _)| base);
        let mut blocks = if view.source {
            vec![Block {
                text: source,
                code: true,
                ..Block::default()
            }]
        } else {
            rich_preview::html(&source, cancel, |src| {
                if src.contains(':') {
                    return None;
                }
                let target = resolve(base, src);
                let mut bytes = Limited {
                    bytes: Vec::new(),
                    cancel,
                };
                book.copy_resource(target.as_str(), &mut bytes).ok()?;
                inspection::image(&bytes.bytes).ok()
            })?
        };
        for block in &mut blocks {
            for run in &mut block.runs {
                if let Some(link) = &mut run.link
                    && !link.contains(':')
                    && !link.starts_with('#')
                {
                    *link = resolve(base, link);
                }
            }
        }
        let mut rows = Vec::new();
        for entry in book.toc() {
            contents(entry, 0, &mut rows);
        }
        if rows.is_empty() {
            rows = chapters
                .iter()
                .enumerate()
                .map(|(i, c)| Row {
                    cells: vec![format!("Chapter {}", i + 1)],
                    target: Some(c.href().as_str().into()),
                    ..Row::default()
                })
                .collect();
        }
        Ok(Some(Inspection {
            kind: bibli_core::inspection::InspectionKind::Book,
            title: format!("EPUB · chapter {} / {}", index + 1, chapters.len()),
            blocks,
            rows,
            previous: (index > 0).then(|| index as u64 - 1),
            next: (index + 1 < chapters.len()).then_some(index as u64 + 1),
            position: index as u64,
            source_available: true,
            notice,
            ..Inspection::default()
        }))
    }
}
fn resolve(base: &str, href: &str) -> String {
    if href.starts_with('/') {
        href.into()
    } else {
        format!("{base}/{href}")
    }
}
struct Limited<'a> {
    bytes: Vec<u8>,
    cancel: &'a CancelToken,
}
impl std::io::Write for Limited<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.cancel.is_cancelled() || self.bytes.len() + buf.len() > inspection::IMAGE_LIMIT {
            return Err(std::io::Error::other(
                "EPUB resource cancelled or exceeds 16 MiB",
            ));
        }
        self.bytes.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn contents(entry: rbook::epub::toc::EpubTocEntry<'_>, depth: usize, rows: &mut Vec<Row>) {
    if depth > 64 || rows.len() >= 2000 {
        return;
    }
    if let Some(href) = entry.href() {
        rows.push(Row {
            cells: vec![entry.label().into()],
            target: Some(href.as_str().into()),
            depth,
            ..Row::default()
        });
    }
    for child in entry {
        contents(child, depth + 1, rows);
    }
}
