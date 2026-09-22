//! EPUB 2/3 native reading mode: rbook package parsing and semantic XHTML.
//!
//! Every byte decompressed from the container passes through one bounded
//! reader. rbook never touches the file: it parses a small in-memory package
//! holding only the entries it needs (container.xml, the OPF and its contents
//! documents), each extracted here with the same bound as every chapter.
use bibli_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, Row},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
};
use bibli_platform::{
    inspection::{self, limits},
    rich_preview,
};
use std::{collections::HashMap, io::Write, sync::Arc};
pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "preview-epub"
    }
    fn title(&self) -> &'static str {
        "Preview: EPUB"
    }
    fn description(&self) -> &'static str {
        "Read EPUB books with their table of contents"
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
        let mut container = Container::open(&path, cancel)?;
        let book = package(&mut container, cancel)?;
        let chapters: Vec<String> = book
            .spine()
            .iter()
            .filter_map(|s| s.manifest_entry())
            .map(|c| c.href().as_str().to_owned())
            .collect();
        anyhow::ensure!(!chapters.is_empty(), "EPUB has no readable chapters");
        // A chapter is found by the member it names, however a link spells
        // it (`Text/../Text/ch2.xhtml`, escaped, with a fragment).
        let keys: Vec<_> = chapters.iter().map(|href| member_key(href)).collect();
        let mut index = view.offset.min(chapters.len().saturating_sub(1) as u64) as usize;
        let mut notice = vec![String::from(
            "Reading mode · publisher CSS and fixed-page layout are not applied",
        )];
        if let Some(query) = view.query.as_deref().filter(|q| !q.is_empty()) {
            // A search starts at the requested chapter and wraps, so a caller
            // resuming at `position + 1` reaches the next matching chapter.
            // A chapter that cannot be read is passed over, not the search.
            let start = (view.offset % chapters.len() as u64) as usize;
            let query = query.to_lowercase();
            let found = (start..chapters.len()).chain(0..start).find(|&candidate| {
                !cancel.is_cancelled()
                    && container
                        .read(&chapters[candidate], "Chapter", cancel)
                        .and_then(|bytes| {
                            rich_preview::html(&inspection::decode(&bytes).0, cancel, |_| None)
                        })
                        .is_ok_and(|page| {
                            page.blocks
                                .iter()
                                .any(|b| b.text.to_lowercase().contains(&query))
                        })
            });
            inspection::check(cancel)?;
            if let Some(found) = found {
                index = found;
            }
            notice = vec![if found.is_some() {
                "Search match in this chapter".into()
            } else {
                "No matching chapter".into()
            }];
        }
        let href = view
            .target
            .clone()
            .filter(|_| view.query.is_none())
            .unwrap_or_else(|| chapters[index].clone());
        let target = member_key(&href);
        let index = keys
            .iter()
            .position(|key| key.is_some() && *key == target)
            .unwrap_or(index);
        let bytes = container.read(&href, "Chapter", cancel)?;
        let (source, encoding) = inspection::decode(&bytes);
        notice.extend(encoding.notice());
        let base = href.rsplit_once('/').map_or("", |(base, _)| base);
        let mut rows = Vec::new();
        for entry in book.toc() {
            contents(entry, 0, &mut rows);
        }
        if rows.is_empty() {
            rows = chapters
                .iter()
                .enumerate()
                .map(|(i, href)| Row {
                    cells: vec![format!("Chapter {}", i + 1)],
                    target: Some(href.clone()),
                    ..Row::default()
                })
                .collect();
        }
        // What the package says about the book, for the panel's Details.
        let metadata = book.metadata();
        let people = |names: Vec<&str>| names.join(", ");
        let details = inspection::details([
            (
                "Title",
                metadata.title().map(|title| title.value().to_owned()),
            ),
            (
                "Author",
                Some(people(metadata.creators().map(|c| c.value()).collect())),
            ),
            (
                "Publisher",
                Some(people(metadata.publishers().map(|p| p.value()).collect())),
            ),
            (
                "Language",
                metadata
                    .language()
                    .map(|language| language.value().to_owned()),
            ),
            ("Chapters", inspection::count(chapters.len())),
        ]);
        let frame = Inspection {
            details,
            title: format!("EPUB · chapter {} / {}", index + 1, chapters.len()),
            previous: (index > 0).then(|| index as u64 - 1),
            next: (index + 1 < chapters.len()).then_some(index as u64 + 1),
            position: index as u64,
            source_available: true,
            ..Inspection::default()
        };
        // A chapter's source is the shared code view, a line per block.
        if view.source {
            let code = inspection::code_lines(&source, Some("xhtml".into()));
            return Ok(Some(Inspection {
                kind: code.kind,
                blocks: code.blocks,
                notice: encoding.notice().unwrap_or_default(),
                ..frame
            }));
        }
        let page = rich_preview::html(&source, cancel, |src| {
            if src.contains(':') {
                return None;
            }
            let bytes = container
                .read(&resolve(base, src), "Embedded image", cancel)
                .ok()?;
            inspection::image(&bytes).ok()
        })?;
        if page.truncated {
            notice.push(inspection::blocks_notice());
        }
        let mut blocks = page.blocks;
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
        Ok(Some(Inspection {
            kind: bibli_core::inspection::InspectionKind::Book,
            blocks,
            rows,
            notice: notice.join(" · "),
            ..frame
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

/// The ZIP container with every read bounded by the actual decompressed
/// bytes, not the size a central directory declares.
struct Container {
    archive: zip::ZipArchive<std::fs::File>,
    /// Normalized member path → index, so hrefs never become archive lookups
    /// through a path the ZIP library interprets.
    members: HashMap<String, usize>,
}
impl Container {
    fn open(path: &std::path::Path, cancel: &CancelToken) -> anyhow::Result<Self> {
        let mut archive = zip::ZipArchive::new(inspection::open(path)?)?;
        anyhow::ensure!(
            archive.len() <= limits::CONTAINER_MEMBERS,
            "EPUB exceeds {} resources",
            limits::CONTAINER_MEMBERS
        );
        // Declared sizes are not checked here: a book's pictures, audio or
        // video are never read unless a chapter shows them, and `read`
        // bounds the bytes each read actually produces.
        let mut members = HashMap::new();
        for i in 0..archive.len() {
            inspection::check(cancel)?;
            let file = archive.by_index_raw(i)?;
            if let Some(name) = normalize(file.name()) {
                members.entry(name).or_insert(i);
            }
        }
        Ok(Self { archive, members })
    }
    /// Read one resource by container href (absolute or root-relative, percent-encoded or not).
    fn read(&mut self, href: &str, what: &str, cancel: &CancelToken) -> anyhow::Result<Vec<u8>> {
        let index = member_key(href)
            .and_then(|name| self.members.get(&name).copied())
            .ok_or_else(|| anyhow::anyhow!("Missing EPUB resource {href}"))?;
        let file = self.archive.by_index(index)?;
        inspection::copy_bounded(file, limits::BINARY_INPUT, what, cancel)
    }
}
/// The member an href names: without its fragment or query, unescaped and
/// normalized. Chapters, links and resources are all compared by it.
fn member_key(href: &str) -> Option<String> {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    let decoded = percent_encoding::percent_decode_str(href)
        .decode_utf8()
        .ok()?;
    normalize(&decoded)
}
/// Collapse `.`/`..`/empty segments; a path escaping the root is not a member.
fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}
/// Build the in-memory package rbook parses: container.xml, the OPF and the
/// contents documents (EPUB 3 nav, EPUB 2 NCX) it may read, all stored after
/// passing through the bounded reader.
fn package(container: &mut Container, cancel: &CancelToken) -> anyhow::Result<rbook::Epub> {
    const CONTAINER: &str = "META-INF/container.xml";
    let container_xml = container.read(CONTAINER, "EPUB container", cancel)?;
    let opf_path = {
        let text = inspection::decode(&container_xml).0;
        let doc = roxmltree::Document::parse(&text)?;
        doc.descendants()
            .find(|n| n.has_tag_name("rootfile"))
            .and_then(|n| n.attribute("full-path"))
            .ok_or_else(|| anyhow::anyhow!("EPUB container names no package"))?
            .to_owned()
    };
    let opf = container.read(&opf_path, "EPUB package", cancel)?;
    let opf_dir = opf_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut contents = Vec::new();
    {
        let text = inspection::decode(&opf).0;
        let doc = roxmltree::Document::parse(&text)?;
        let items: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name("item"))
            .collect();
        let ncx_id = doc
            .descendants()
            .find(|n| n.has_tag_name("spine"))
            .and_then(|n| n.attribute("toc"));
        for item in items {
            let nav = item
                .attribute("properties")
                .is_some_and(|p| p.split_whitespace().any(|p| p == "nav"));
            let ncx = item.attribute("media-type") == Some("application/x-dtbncx+xml")
                || (ncx_id.is_some() && item.attribute("id") == ncx_id);
            if let Some(href) = item.attribute("href").filter(|_| nav || ncx) {
                contents.push(resolve(opf_dir, href));
            }
        }
    }
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut written = std::collections::HashSet::new();
    for (name, bytes) in [(CONTAINER.to_owned(), container_xml), (opf_path, opf)] {
        let name = normalize(&name).unwrap_or(name);
        zip.start_file(&name, stored)?;
        zip.write_all(&bytes)?;
        written.insert(name);
    }
    for href in contents {
        inspection::check(cancel)?;
        let Some(name) = member_key(&href) else {
            continue;
        };
        if !written.insert(name.clone()) {
            continue;
        }
        // A contents document the container lacks is rbook's error to report.
        if let Ok(bytes) = container.read(&href, "EPUB contents", cancel) {
            zip.start_file(&name, stored)?;
            zip.write_all(&bytes)?;
        }
    }
    let bytes = zip.finish()?.into_inner();
    Ok(rbook::Epub::read(std::io::Cursor::new(bytes))?)
}

/// Flatten a contents tree. Wrapper entries without a target (such as the
/// root nav element) contribute no row and no indentation.
fn contents(entry: rbook::epub::toc::EpubTocEntry<'_>, depth: usize, rows: &mut Vec<Row>) {
    if depth > limits::NESTING || rows.len() >= limits::BLOCKS {
        return;
    }
    let mut child_depth = depth;
    if let Some(href) = entry.href() {
        rows.push(Row {
            cells: vec![entry.label().into()],
            target: Some(href.as_str().into()),
            depth,
            ..Row::default()
        });
        child_depth += 1;
    }
    for child in entry {
        contents(child, child_depth, rows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::op::EntryRef;

    const CONTAINER_XML: &str = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    const OPF: &str = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">urn:example:synthetic</dc:identifier><dc:title>Synthetic book</dc:title><dc:language>en</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="three" href="sub/caf%C3%A9%20three.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/><itemref idref="three"/></spine></package>"#;
    const NAV: &str = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="one.xhtml">Start</a><ol><li><a href="two.xhtml">Middle</a></li></ol></li><li><a href="sub/caf%C3%A9%20three.xhtml">End</a></li></ol></nav></body></html>"#;

    fn write(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, body) in entries {
            zip.start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
    }

    fn book(path: &std::path::Path) {
        write(
            path,
            &[
                ("mimetype", b"application/epub+zip"),
                ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
                ("EPUB/book.opf", OPF.as_bytes()),
                ("EPUB/nav.xhtml", NAV.as_bytes()),
                (
                    "EPUB/one.xhtml",
                    br"<html><body><h1>First</h1><p>The needle is here.</p></body></html>",
                ),
                (
                    "EPUB/two.xhtml",
                    br"<html><body><h1>Second</h1><p>Nothing to see.</p></body></html>",
                ),
                (
                    "EPUB/sub/café three.xhtml",
                    br"<html><body><h1>Third</h1><p>Another needle.</p></body></html>",
                ),
            ],
        );
    }

    fn inspect(path: &std::path::Path, view: &InspectRequest) -> anyhow::Result<Inspection> {
        let request = PreviewRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 64,
        };
        Provider
            .inspect(&request, view, &CancelToken::new())
            .map(Option::unwrap)
    }

    #[test]
    fn contents_and_encoded_names_resolve_through_the_bounded_container() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic book.epub");
        book(&path);
        let first = inspect(&path, &InspectRequest::default()).unwrap();
        assert_eq!(
            first
                .rows
                .iter()
                .map(|r| (r.cells[0].as_str(), r.depth))
                .collect::<Vec<_>>(),
            [("Start", 0), ("Middle", 1), ("End", 0)]
        );
        // What the package says about the book reaches the panel's Details;
        // a field the book leaves blank (no author here) is not a row.
        assert_eq!(
            first
                .details
                .iter()
                .map(|detail| (detail.label.as_str(), detail.value.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Title", "Synthetic book"),
                ("Language", "en"),
                ("Chapters", "3")
            ]
        );
        let third = inspect(
            &path,
            &InspectRequest {
                target: first.rows[2].target.clone(),
                ..InspectRequest::default()
            },
        )
        .unwrap();
        assert_eq!(third.position, 2);
        assert!(third.plain_text().contains("Another needle"));
    }

    #[test]
    fn repeated_search_advances_to_the_next_chapter_and_wraps() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic book.epub");
        book(&path);
        let search = |offset: u64| {
            inspect(
                &path,
                &InspectRequest {
                    query: Some("NEEDLE".into()),
                    offset,
                    ..InspectRequest::default()
                },
            )
            .unwrap()
        };
        let first = search(0);
        assert_eq!(first.position, 0);
        assert_eq!(first.notice, "Search match in this chapter");
        let second = search(first.position + 1);
        assert_eq!(second.position, 2, "chapter two has no match");
        let wrapped = search(second.position + 1);
        assert_eq!(wrapped.position, 0);
        let none = inspect(
            &path,
            &InspectRequest {
                query: Some("absent".into()),
                offset: 1,
                ..InspectRequest::default()
            },
        )
        .unwrap();
        assert_eq!(none.notice, "No matching chapter");
        assert_eq!(none.position, 1);
    }

    /// A container entry whose deflate stream inflates far beyond the size its
    /// headers declare must fail on the bytes produced, before rbook parses it.
    #[test]
    fn declared_sizes_do_not_bound_decompression() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic bomb.epub");
        let inflated = vec![0u8; limits::BINARY_INPUT + 1024];
        write(
            &path,
            &[
                ("META-INF/container.xml", &inflated),
                ("EPUB/book.opf", OPF.as_bytes()),
            ],
        );
        // Declare a tiny uncompressed size in the local header and central directory.
        let mut bytes = std::fs::read(&path).unwrap();
        let lying = 100u32.to_le_bytes();
        let local = bytes.windows(4).position(|w| w == b"PK\x03\x04").unwrap();
        bytes[local + 22..local + 26].copy_from_slice(&lying);
        let central = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        bytes[central + 24..central + 28].copy_from_slice(&lying);
        std::fs::write(&path, &bytes).unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        assert_eq!(
            archive.by_index_raw(0).unwrap().size(),
            100,
            "the lie is in place"
        );
        let error = inspect(&path, &InspectRequest::default()).unwrap_err();
        assert!(
            error.to_string().contains("EPUB container exceeds 16 MiB"),
            "{error}"
        );
    }

    /// The layout Sigil writes: package in `OEBPS/`, chapters in `Text/`,
    /// linked to each other through `../Text/`.
    fn sigil(path: &std::path::Path, extra: &[(&str, &[u8])]) {
        let opf = OPF
            .replace(
                "nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"",
                "Text/nav.xhtml\" media-type=\"application/xhtml+xml\"",
            )
            .replace("href=\"one.xhtml\"", "href=\"Text/one.xhtml\"")
            .replace("href=\"two.xhtml\"", "href=\"Text/two.xhtml\"")
            .replace(
                "href=\"sub/caf%C3%A9%20three.xhtml\"",
                "href=\"Text/three.xhtml\"",
            );
        let container = CONTAINER_XML.replace("EPUB/book.opf", "OEBPS/content.opf");
        let mut entries: Vec<(&str, &[u8])> = vec![
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", container.as_bytes()),
            ("OEBPS/content.opf", opf.as_bytes()),
            (
                "OEBPS/Text/one.xhtml",
                br#"<html><body><h1>First</h1><p><a href="../Text/two.xhtml#note5">Note</a></p></body></html>"#,
            ),
            (
                "OEBPS/Text/two.xhtml",
                br#"<html><body><h1>Second</h1><p id="note5">The note.</p></body></html>"#,
            ),
            (
                "OEBPS/Text/three.xhtml",
                b"<html><body><h1>Th\xe9</h1><p>caf\xe9</p></body></html>",
            ),
        ];
        entries.extend_from_slice(extra);
        write(path, &entries);
    }

    #[test]
    fn a_link_through_parent_folders_lands_on_its_chapter() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic sigil book.epub");
        sigil(&path, &[]);
        let first = inspect(&path, &InspectRequest::default()).unwrap();
        let link = first
            .blocks
            .iter()
            .flat_map(|b| &b.runs)
            .find_map(|r| r.link.clone())
            .unwrap();
        assert!(link.ends_with("../Text/two.xhtml#note5"), "{link}");
        let linked = inspect(
            &path,
            &InspectRequest {
                target: Some(link),
                ..InspectRequest::default()
            },
        )
        .unwrap();
        assert_eq!(linked.position, 1);
        assert_eq!(linked.title, "EPUB · chapter 2 / 3");
        assert_eq!((linked.previous, linked.next), (Some(0), Some(2)));
        assert!(linked.plain_text().contains("The note."));
        // A chapter in a legacy encoding reads, and says how.
        let third = inspect(
            &path,
            &InspectRequest {
                offset: 2,
                ..InspectRequest::default()
            },
        )
        .unwrap();
        assert!(third.plain_text().contains("caf\u{e9}"));
        assert!(
            third.notice.contains("Read as Windows-1252"),
            "{}",
            third.notice
        );
    }

    #[test]
    fn resources_never_read_do_not_refuse_the_book() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic illustrated book.epub");
        let video = vec![0u8; limits::BINARY_INPUT + 1024];
        sigil(&path, &[("OEBPS/Video/film.mp4", &video)]);
        let first = inspect(&path, &InspectRequest::default()).unwrap();
        assert!(first.plain_text().contains("First"));
    }

    #[test]
    fn a_chapter_source_is_the_code_view_and_search_passes_over_bad_chapters() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic book.epub");
        book(&path);
        let source = inspect(
            &path,
            &InspectRequest {
                source: true,
                offset: 1,
                ..InspectRequest::default()
            },
        )
        .unwrap();
        assert_eq!(source.kind, bibli_core::inspection::InspectionKind::Code);
        assert!(source.blocks.iter().all(|b| b.code));
        assert_eq!(source.blocks[0].language.as_deref(), Some("xhtml"));
        assert_eq!(source.position, 1);
        assert!(source.source_available);

        // The spine names a chapter the container lacks: the search reads on.
        let broken = tmp.path().join("Synthetic broken book.epub");
        let opf = OPF.replace("href=\"two.xhtml\"", "href=\"absent.xhtml\"");
        write(
            &broken,
            &[
                ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
                ("EPUB/book.opf", opf.as_bytes()),
                ("EPUB/nav.xhtml", NAV.as_bytes()),
                (
                    "EPUB/one.xhtml",
                    b"<html><body><p>Nothing.</p></body></html>",
                ),
                (
                    "EPUB/sub/caf\u{e9} three.xhtml",
                    b"<html><body><p>The needle.</p></body></html>",
                ),
            ],
        );
        let found = inspect(
            &broken,
            &InspectRequest {
                query: Some("needle".into()),
                ..InspectRequest::default()
            },
        )
        .unwrap();
        assert_eq!(found.position, 2);
    }

    #[test]
    fn member_paths_never_escape_the_container_root() {
        assert_eq!(
            normalize("/EPUB/./a/../b.xhtml"),
            Some("EPUB/b.xhtml".into())
        );
        assert_eq!(normalize("EPUB//x.xhtml"), Some("EPUB/x.xhtml".into()));
        assert_eq!(normalize("../outside.xhtml"), None);
        assert_eq!(normalize("EPUB/../../x"), None);
        assert_eq!(normalize("/"), None);
    }
}
