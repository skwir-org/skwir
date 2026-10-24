//! EPUB 2/3 native reading mode: rbook package parsing and semantic XHTML;
//! and books' covers as thumbnails.
//!
//! Every byte decompressed from the container passes through one bounded
//! reader. rbook never touches the file: it parses a small in-memory package
//! holding only the entries it needs (container.xml, the OPF and its contents
//! documents), each extracted here with the same bound as every chapter.
use skwir_core::{
    App, CancelToken,
    inspection::{InspectRequest, Inspection, Picture, PrintedPage, Row},
    plugin::Plugin,
    preview::{PreviewProvider, PreviewRequest},
    thumbs::{Image, Stage, ThumbRequest, ThumbnailProvider},
};
use skwir_platform::{
    inspection::{self, limits, limits::Bounds},
    rich_preview,
};
use std::{collections::HashMap, io::Write, sync::Arc};

mod pages;

pub struct PreviewPlugin;
impl Plugin for PreviewPlugin {
    fn id(&self) -> &'static str {
        "epub"
    }
    fn title(&self) -> &'static str {
        "EPUB"
    }
    fn description(&self) -> &'static str {
        "Read EPUB books with their table of contents"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    /// A new engine reads books from now on; covers are the same either way.
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|key| key == BACKEND) {
            app.previews.unregister(self.id(), self.id());
            register_books(app);
        }
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        use skwir_platform::document::{Engine, worker};
        vec![worker(Engine::Mupdf).requirement("Books laid out into pages by MuPDF")]
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read, Capability::Programs])
    }
    fn register(&self, app: &App) {
        app.settings.declare(skwir_core::settings::SettingSpec::choice(
            BACKEND,
            "Book engine",
            "Automatic reads books with the built-in engine: text that reflows in the theme, fixed-layout pages drawn as their book lays them out. MuPDF lays every book out into pages, with its own styles.",
            "auto",
            &[
                ("auto", "Automatic"),
                ("builtin", "Built-in"),
                ("mupdf", "MuPDF"),
            ],
        )
        .page(skwir_core::settings::page::READING)
        .section("Books"));
        register_books(app);
        app.thumbnails.register(self.id(), Arc::new(Cover));
    }
}

/// Which engine reads books.
const BACKEND: &str = "epub.backend";

/// The books provider, with the engine the setting chooses.
fn register_books(app: &App) {
    let engine = match app.settings.get::<String>(BACKEND).as_deref() {
        Some("mupdf") => Engine::Mupdf,
        _ => Engine::BuiltIn,
    };
    app.previews.register("epub", Arc::new(Provider { engine }));
}
/// A book's cover for the grid: the picture its package names, read through
/// the bounded container. A fallback: a thumbnailer the desktop installed
/// for books comes first.
struct Cover;
impl ThumbnailProvider for Cover {
    fn id(&self) -> &'static str {
        "epub-covers"
    }
    fn stage(&self) -> Stage {
        Stage::Fallback
    }
    fn accepts(&self, req: &ThumbRequest) -> bool {
        inspection::extension(&req.entry.path) == "epub"
    }
    fn thumbnail(&self, req: &ThumbRequest, cancel: &CancelToken) -> anyhow::Result<Option<Image>> {
        let mut container = Container::open(&req.entry.resolve()?, cancel)?;
        let Some(href) = cover(&mut container, cancel)? else {
            return Ok(None);
        };
        // Nothing for a cover the book encrypts.
        let Some(bytes) = container.file(&href, cancel) else {
            return Ok(None);
        };
        let cover = inspection::picture(&bytes, req.size.pixels())?;
        Ok(Some(Image {
            width: cover.width,
            height: cover.height,
            rgba: cover.rgba.into(),
        }))
    }
}
/// A part of the book's XML, parsed once it is known not to nest deeper
/// than the parser's recursion can go (`inspection::xml_depth`).
fn parse_xml(text: &str) -> anyhow::Result<roxmltree::Document<'_>> {
    inspection::xml_depth(text.as_bytes())?;
    Ok(roxmltree::Document::parse(text)?)
}

/// The member a book names as its cover picture: the EPUB 3 manifest item
/// whose properties say `cover-image`, else the item the EPUB 2
/// `<meta name="cover">` names (by id, or by href as some tools write it).
/// Only a picture: a cover page is a chapter, not a thumbnail.
fn cover(container: &mut Container, cancel: &CancelToken) -> anyhow::Result<Option<String>> {
    let (_, opf_path, opf) = package_document(container, cancel)?;
    let opf_dir = opf_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let text = inspection::decode(&opf).0;
    let doc = parse_xml(&text)?;
    let items: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("item"))
        .filter(|n| {
            n.attribute("media-type")
                .is_some_and(|m| m.starts_with("image/"))
        })
        .collect();
    let property = items.iter().find(|n| {
        n.attribute("properties")
            .is_some_and(|p| p.split_whitespace().any(|p| p == "cover-image"))
    });
    let named = || {
        let name = doc
            .descendants()
            .find(|n| n.has_tag_name("meta") && n.attribute("name") == Some("cover"))?
            .attribute("content")?;
        items
            .iter()
            .find(|n| n.attribute("id") == Some(name))
            .or_else(|| items.iter().find(|n| n.attribute("href") == Some(name)))
    };
    Ok(property
        .or_else(named)
        .and_then(|n| n.attribute("href"))
        .map(|href| resolve(opf_dir, href)))
}
/// Which engine reads a book (`epub.backend`). Either one sizes
/// what is read with the reader's text size, and zooms what is looked at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Engine {
    /// Reading mode for reflowing books, drawn pages for fixed ones.
    #[default]
    BuiltIn,
    /// Every book laid out into pages by `MuPDF`, in the document reader.
    Mupdf,
}
#[derive(Default)]
struct Provider {
    engine: Engine,
}
impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        "epub"
    }
    fn accepts(&self, req: &PreviewRequest) -> bool {
        inspection::extension(&req.entry.path) == "epub"
    }
    /// A picture a chapter named, by its member: read through the same
    /// bounded reader, and never outside the container.
    fn picture(
        &self,
        req: &PreviewRequest,
        _view: &InspectRequest,
        key: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<skwir_core::thumbs::Image>> {
        let mut container = Container::open(&req.entry.resolve()?, cancel)?;
        let bytes = container.read(key, "Embedded image", cancel)?;
        Ok(Some(inspection::image(&bytes)?))
    }
    /// A book of pages opens in the document reader: with `MuPDF`, every
    /// book, laid out into pages at the reader's text size and again when it
    /// changes; with the built-in engine, a fixed-layout book, its pages
    /// drawn as it lays them out (`pages::FixedBook`).
    fn open_document(
        &self,
        req: &PreviewRequest,
        lifetime: &CancelToken,
    ) -> anyhow::Result<Option<Arc<dyn skwir_core::document::DocumentSession>>> {
        if self.engine != Engine::Mupdf {
            let book = pages::FixedBook::open(req.entry.resolve()?, lifetime)?;
            return Ok(
                book.map(|book| Arc::new(book) as Arc<dyn skwir_core::document::DocumentSession>)
            );
        }
        let session = skwir_platform::document::open_book(
            req.entry.resolve()?,
            req.text_size,
            lifetime.clone(),
        )?;
        anyhow::ensure!(
            session.is_some(),
            "Reading books with MuPDF needs skwir-mupdf-worker; install it, or choose the built-in book engine"
        );
        Ok(session)
    }
    fn inspect(
        &self,
        req: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        // MuPDF's books are documents, opened by `open_document`.
        if self.engine == Engine::Mupdf {
            return Ok(None);
        }
        let path = req.entry.resolve()?;
        let mut container = Container::open(&path, cancel)?;
        let (book, fixed) = package(&mut container, cancel)?;
        // A book of pages is a document (`open_document`).
        if fixed {
            return Ok(None);
        }
        let (chapters, markups): (Vec<String>, Vec<rich_preview::Markup>) = book
            .spine()
            .iter()
            .filter_map(|s| s.manifest_entry())
            .map(|c| {
                let markup = if c.media_type() == "text/html" {
                    rich_preview::Markup::Html
                } else {
                    rich_preview::Markup::Xhtml
                };
                (c.href().as_str().to_owned(), markup)
            })
            .unzip();
        anyhow::ensure!(!chapters.is_empty(), "EPUB has no readable chapters");
        // A chapter is found by the member it names, however a link spells
        // it (`Text/../Text/ch2.xhtml`, escaped, with a fragment).
        let keys: Vec<_> = chapters.iter().map(|href| member_key(href)).collect();
        let mut index = view.offset.min(chapters.len().saturating_sub(1) as u64) as usize;
        let mut sheets = Sheets::new();
        let mut notice = vec![String::from(
            "Reading mode · the book's fonts, colours and page layout are not applied",
        )];
        if let Some(query) = view.query.as_deref().filter(|q| !q.is_empty()) {
            // A search starts at the requested chapter and wraps, so a caller
            // resuming at `position + 1` reaches the next matching chapter.
            // A chapter that cannot be read is passed over, not the search.
            let start = (view.offset % chapters.len() as u64) as usize;
            let query = query.to_lowercase();
            // A chapter is searched as it reads, with its stylesheets: what
            // the book hides is not a match.
            let found = (start..chapters.len()).chain(0..start).find(|&candidate| {
                !cancel.is_cancelled()
                    && container
                        .read(&chapters[candidate], "Chapter", cancel)
                        .and_then(|bytes| {
                            let resources = Chapter {
                                container: &mut container,
                                base: base(&chapters[candidate]),
                                cancel,
                                images: false,
                                sheets: &mut sheets,
                            };
                            rich_preview::html(
                                &inspection::decode(&bytes).0,
                                markups[candidate],
                                Bounds::of(view),
                                cancel,
                                resources,
                            )
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
        let base = base(&href);
        let mut rows = table_of_contents(&book);
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
        // Each row knows the chapter it opens, so the contents show where
        // the reader is: a row naming no chapter of the spine knows none.
        for row in &mut rows {
            let key = row.target.as_deref().and_then(member_key);
            row.section = keys
                .iter()
                .position(|k| k.is_some() && *k == key)
                .map(|section| section as u64);
        }
        // What the package says about the book, for the panel's Properties.
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
            sections: chapters.len() as u64,
            printed_pages: printed_pages(&book, &keys),
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
        let resources = Chapter {
            container: &mut container,
            base,
            cancel,
            images: true,
            sheets: &mut sheets,
        };
        let bounds = Bounds::of(view);
        let page = rich_preview::html(&source, markups[index], bounds, cancel, resources)?;
        if page.truncated {
            notice.push(inspection::blocks_notice(bounds));
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
            kind: skwir_core::inspection::InspectionKind::Book,
            blocks,
            rows,
            notice: notice.join(" · "),
            whole_available: page.truncated && bounds.cut(),
            ..frame
        }))
    }
}
/// A chapter's folder in the container, which its links start from.
fn base(href: &str) -> &str {
    href.rsplit_once('/').map_or("", |(base, _)| base)
}
/// What a chapter names inside the container, read through its bounded
/// reader: pictures (unless only the text is wanted) and stylesheets.
/// Nothing outside the container is ever named.
struct Chapter<'a> {
    container: &'a mut Container,
    base: &'a str,
    cancel: &'a CancelToken,
    images: bool,
    /// The book's stylesheets parsed, by member and the folder their URLs
    /// are found at: a search parses each once, not once a chapter.
    sheets: &'a mut Sheets,
}

/// The parsed stylesheets of the chapters read for one preview.
type Sheets = HashMap<(String, String), Option<Arc<rich_preview::Parsed>>>;
impl rich_preview::Resources for Chapter<'_> {
    /// Named by its member for the view to decode when it shows it, with
    /// the size it will be shown at, read from its header.
    fn picture(&mut self, src: &str) -> Option<Picture> {
        if !self.images || src.contains(':') {
            return None;
        }
        let key = member_key(&resolve(self.base, src))?;
        let head = self
            .container
            .read_head(&key, limits::PICTURE_HEADER, self.cancel)
            .ok()?;
        let (width, height) = inspection::picture_size(&head).or_else(|| {
            let whole = self
                .container
                .read(&key, "Embedded image", self.cancel)
                .ok()?;
            inspection::picture_size(&whole)
        })?;
        Some(Picture::Later { key, width, height })
    }
    fn file(&mut self, href: &str) -> Option<Vec<u8>> {
        if href.contains(':') {
            return None;
        }
        self.container.file(&resolve(self.base, href), self.cancel)
    }
    fn parsed_stylesheet(&mut self, href: &str, base: &str) -> Option<Arc<rich_preview::Parsed>> {
        if href.contains(':') {
            return None;
        }
        let key = (resolve(self.base, href), base.to_owned());
        if let Some(parsed) = self.sheets.get(&key) {
            return parsed.clone();
        }
        let parsed = self
            .stylesheet(href)
            .map(|css| Arc::new(rich_preview::Parsed::new(&css, base)));
        // A sheet unread for a cancel is not one the book lacks.
        if !self.cancel.is_cancelled() {
            self.sheets.insert(key, parsed.clone());
        }
        parsed
    }
    fn stylesheet(&mut self, href: &str) -> Option<String> {
        if href.contains(':') {
            return None;
        }
        let bytes = self
            .container
            .read(&resolve(self.base, href), "Stylesheet", self.cancel)
            .ok()?;
        (bytes.len() <= limits::TEXT_INPUT).then(|| inspection::decode(&bytes).0.into_owned())
    }
}
fn resolve(base: &str, href: &str) -> String {
    if href.starts_with('/') {
        href.into()
    } else {
        format!("{base}/{href}")
    }
}

/// The ZIP container, read as every package is
/// (`skwir_platform::package`: bounded, members by normalized name, so
/// hrefs never become archive lookups through a path the ZIP library
/// interprets).
struct Container {
    package: skwir_platform::package::Package,
    /// Members `META-INF/encryption.xml` lists, by the algorithm named.
    encrypted: HashMap<String, String>,
    /// The package's unique identifier, which obfuscated fonts are keyed
    /// by: read by `package`, else when a font first needs it (a
    /// fixed-layout page drawn, a search, a cover read without the rest).
    identifier: Option<String>,
}

/// How EPUB obfuscates embedded fonts: not encryption, a key every reading
/// system knows. Anything else `encryption.xml` names is DRM, and unread.
const IDPF_OBFUSCATION: &str = "http://www.idpf.org/2008/embedding";
const ADOBE_OBFUSCATION: &str = "http://ns.adobe.com/pdf/enc#RC";

/// Undo a font's obfuscation: its first bytes combined (exclusive or) with a key derived from
/// the package's unique identifier (IDPF: the SHA-1 of the identifier
/// without whitespace, over 1040 bytes; Adobe: the 16 bytes of its UUID,
/// over 1024).
fn deobfuscate(bytes: &mut [u8], algorithm: &str, identifier: &str) -> Option<()> {
    let (key, length): (Vec<u8>, usize) = match algorithm {
        IDPF_OBFUSCATION => {
            let identifier: String = identifier
                .chars()
                .filter(|c| !matches!(c, ' ' | '\t' | '\r' | '\n'))
                .collect();
            (
                sha1_smol::Sha1::from(identifier).digest().bytes().to_vec(),
                1040,
            )
        }
        ADOBE_OBFUSCATION => {
            let hex: String = identifier
                .trim()
                .trim_start_matches("urn:uuid:")
                .chars()
                .filter(char::is_ascii_hexdigit)
                .collect();
            let key: Vec<u8> = (0..hex.len() / 2)
                .filter_map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok())
                .collect();
            if key.len() != 16 {
                return None;
            }
            (key, 1024)
        }
        _ => return None,
    };
    for (i, byte) in bytes.iter_mut().take(length).enumerate() {
        *byte ^= key[i % key.len()];
    }
    Some(())
}
impl Container {
    fn open(path: &std::path::Path, cancel: &CancelToken) -> anyhow::Result<Self> {
        // Declared sizes are not checked here: a book's pictures, audio or
        // video are never read unless a chapter shows them, and `read`
        // bounds the bytes each read actually produces.
        let mut container = Self {
            package: skwir_platform::package::Package::open(path, "This EPUB", false, cancel)?,
            encrypted: HashMap::new(),
            identifier: None,
        };
        if let Ok(xml) = container.read("META-INF/encryption.xml", "EPUB encryption", cancel) {
            let text = inspection::decode(&xml).0;
            if let Ok(doc) = parse_xml(&text) {
                for data in doc
                    .descendants()
                    .filter(|n| n.has_tag_name("EncryptedData"))
                {
                    let algorithm = data
                        .descendants()
                        .find(|n| n.has_tag_name("EncryptionMethod"))
                        .and_then(|n| n.attribute("Algorithm"));
                    let uri = data
                        .descendants()
                        .find(|n| n.has_tag_name("CipherReference"))
                        .and_then(|n| n.attribute("URI"))
                        .and_then(member_key);
                    if let (Some(algorithm), Some(uri)) = (algorithm, uri) {
                        container.encrypted.insert(uri, algorithm.to_owned());
                    }
                }
            }
        }
        Ok(container)
    }
    /// A resource as a reading system reads it: an obfuscated font restored,
    /// and nothing for what is encrypted.
    fn file(&mut self, href: &str, cancel: &CancelToken) -> Option<Vec<u8>> {
        let mut bytes = self.read(href, "EPUB resource", cancel).ok()?;
        if let Some(algorithm) = member_key(href).and_then(|key| self.encrypted.get(&key).cloned())
        {
            let identifier = self.identifier(cancel)?;
            deobfuscate(&mut bytes, &algorithm, &identifier)?;
        }
        Some(bytes)
    }
    /// The package's unique identifier, read from its package document the
    /// first time it is needed.
    fn identifier(&mut self, cancel: &CancelToken) -> Option<String> {
        if self.identifier.is_none() {
            let (_, _, opf) = package_document(self, cancel).ok()?;
            let text = inspection::decode(&opf).0;
            self.identifier = Some(unique_identifier(&parse_xml(&text).ok()?)?);
        }
        self.identifier.clone()
    }
    /// The first `limit` bytes of a resource, or all of it if it is shorter:
    /// enough to learn a picture's size without inflating the rest.
    fn read_head(
        &mut self,
        href: &str,
        limit: usize,
        cancel: &CancelToken,
    ) -> anyhow::Result<Vec<u8>> {
        inspection::check(cancel)?;
        member_key(href)
            .and_then(|name| self.package.head(&name, limit))
            .ok_or_else(|| anyhow::anyhow!("Missing EPUB resource {href}"))
    }
    /// Read one resource by container href (absolute or root-relative, percent-encoded or not).
    fn read(&mut self, href: &str, what: &str, cancel: &CancelToken) -> anyhow::Result<Vec<u8>> {
        let name =
            member_key(href).ok_or_else(|| anyhow::anyhow!("Missing EPUB resource {href}"))?;
        anyhow::ensure!(self.package.has(&name), "Missing EPUB resource {href}");
        self.package.read(&name, limits::BINARY_INPUT, what, cancel)
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
use skwir_platform::package::normalize;
const CONTAINER: &str = "META-INF/container.xml";
/// The container document, the package document's path, and the package
/// document (the OPF) it names.
fn package_document(
    container: &mut Container,
    cancel: &CancelToken,
) -> anyhow::Result<(Vec<u8>, String, Vec<u8>)> {
    let container_xml = container.read(CONTAINER, "EPUB container", cancel)?;
    let opf_path = {
        let text = inspection::decode(&container_xml).0;
        let doc = parse_xml(&text)?;
        doc.descendants()
            .find(|n| n.has_tag_name("rootfile"))
            .and_then(|n| n.attribute("full-path"))
            .ok_or_else(|| anyhow::anyhow!("EPUB container names no package"))?
            .to_owned()
    };
    let opf = container.read(&opf_path, "EPUB package", cancel)?;
    Ok((container_xml, opf_path, opf))
}
/// The identifier obfuscated fonts are keyed by: the one the package
/// document names as its unique identifier.
fn unique_identifier(doc: &roxmltree::Document<'_>) -> Option<String> {
    let unique = doc.root_element().attribute("unique-identifier");
    doc.descendants()
        .find(|n| n.has_tag_name("identifier") && n.attribute("id") == unique)
        .and_then(|n| n.text())
        .map(|identifier| identifier.trim().to_owned())
}
/// Build the in-memory package rbook parses: container.xml, the OPF and the
/// contents documents (EPUB 3 nav, EPUB 2 NCX) it may read, all stored after
/// passing through the bounded reader.
/// Also says whether the book is laid out in fixed pages
/// (`rendition:layout` is `pre-paginated`).
fn package(container: &mut Container, cancel: &CancelToken) -> anyhow::Result<(rbook::Epub, bool)> {
    let (container_xml, opf_path, opf) = package_document(container, cancel)?;
    let opf_dir = opf_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut contents = Vec::new();
    let fixed;
    {
        let text = inspection::decode(&opf).0;
        let doc = parse_xml(&text)?;
        fixed = doc.descendants().any(|n| {
            n.has_tag_name("meta")
                && n.attribute("property") == Some("rendition:layout")
                && n.text().map(str::trim) == Some("pre-paginated")
        });
        if let Some(identifier) = unique_identifier(&doc) {
            container.identifier = Some(identifier);
        }
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
        // A contents document the container lacks is rbook's error to report;
        // one nested deeper than a parser's recursion can go is left out.
        if let Ok(bytes) = container.read(&href, "EPUB contents", cancel)
            && inspection::xml_depth(&bytes).is_ok()
        {
            zip.start_file(&name, stored)?;
            zip.write_all(&bytes)?;
        }
    }
    let bytes = zip.finish()?.into_inner();
    Ok((rbook::Epub::read(std::io::Cursor::new(bytes))?, fixed))
}

/// The book's table of contents, and only it: the EPUB 3 `toc` nav, else
/// the EPUB 2 NCX `navMap`. A book's navigation also lists its landmarks
/// and its printed pages (`landmarks`, `page-list`, the NCX `pageList`, the
/// OPF guide), and often the same chapters twice (nav and NCX): those are
/// not its contents. `rbook::Epub::toc` iterates all of them.
fn table_of_contents(book: &rbook::Epub) -> Vec<Row> {
    let mut rows = Vec::new();
    // The root is the `nav` (or the `navMap`) itself, its label the
    // contents' own heading: its entries are the book's.
    if let Some(root) = book.toc().contents() {
        for entry in root {
            contents(entry, 0, 1, &mut rows);
        }
    }
    rows
}

/// The printed book's pages, in order (`page-list`, else the NCX
/// `pageList`): each with the chapter that holds it, among `keys` (the
/// spine's members), and its link. A page in no chapter of the spine is
/// left out.
fn printed_pages(book: &rbook::Epub, keys: &[Option<String>]) -> Vec<PrintedPage> {
    /// Pages kept: more than any printed book has.
    const PAGES: usize = 20_000;
    fn walk(
        entry: rbook::epub::toc::EpubTocEntry<'_>,
        depth: usize,
        keys: &[Option<String>],
        pages: &mut Vec<PrintedPage>,
    ) {
        if depth > limits::NESTING || pages.len() >= PAGES {
            return;
        }
        if let Some(href) = entry.href() {
            let target = href.as_str();
            let key = member_key(target);
            if let Some(section) = keys.iter().position(|k| k.is_some() && *k == key) {
                pages.push(PrintedPage {
                    label: entry.label().trim().to_owned(),
                    section: section as u64,
                    target: target.to_owned(),
                });
            }
        }
        for child in entry {
            walk(child, depth + 1, keys, pages);
        }
    }
    let mut pages = Vec::new();
    if let Some(root) = book.toc().page_list() {
        walk(root, 0, keys, &mut pages);
    }
    pages.retain(|page| !page.label.is_empty());
    pages
}

/// Flatten a contents tree. An entry that leads nowhere but is named (an
/// EPUB 3 heading over the chapters below it: a part) is a row without a
/// target, its entries nested under it; one with neither a target nor a
/// name only wraps its entries: no row, no indentation.
fn contents(
    entry: rbook::epub::toc::EpubTocEntry<'_>,
    depth: usize,
    level: usize,
    rows: &mut Vec<Row>,
) {
    // `depth` counts the entries shown; `level` every one walked, wrappers
    // too, so a contents nested deep in wrappers is not walked as deep as
    // it goes.
    if depth > limits::NESTING || level > limits::XML_DEPTH || rows.len() >= limits::BLOCKS {
        return;
    }
    let mut child_depth = depth;
    let target = entry.href().map(|href| href.as_str().to_owned());
    if target.is_some() || !entry.label().trim().is_empty() {
        rows.push(Row {
            cells: vec![entry.label().into()],
            target,
            depth,
            ..Row::default()
        });
        child_depth += 1;
    }
    for child in entry {
        contents(child, child_depth, level + 1, rows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::op::EntryRef;

    const CONTAINER_XML: &str = r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="EPUB/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    const OPF: &str = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">urn:example:synthetic</dc:identifier><dc:title>Synthetic book</dc:title><dc:language>en</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="one" href="one.xhtml" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/><item id="three" href="sub/caf%C3%A9%20three.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/><itemref idref="three"/></spine></package>"#;
    const NAV: &str = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="one.xhtml">Start</a><ol><li><a href="two.xhtml">Middle</a></li></ol></li><li><a href="sub/caf%C3%A9%20three.xhtml">End</a></li></ol></nav></body></html>"#;

    /// A 2 × 3 picture.
    const PNG: &[u8] = b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a\x00\x00\x00\x0d\x49\x48\x44\x52\x00\x00\x00\x02\x00\x00\x00\x03\x08\x06\x00\x00\x00\xb9\xea\xde\x81\x00\x00\x00\x11\x49\x44\x41\x54\x78\x9c\x63\xf8\xcf\xc0\xf0\x1f\x84\x19\x30\x18\x00\xa1\x79\x0b\xf5\x4d\xc4\x9a\x07\x00\x00\x00\x00\x49\x45\x4e\x44\xae\x42\x60\x82";

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
            text_size: None,
            fit: None,
        };
        Provider::default()
            .inspect(&request, view, &CancelToken::new())
            .map(Option::unwrap)
    }

    #[test]
    fn each_engine_answers_through_its_own_surface() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic book.epub");
        book(&path);
        let request = PreviewRequest {
            entry: EntryRef::from_path(&path).unwrap(),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: Some(20.),
            fit: None,
        };
        let cancel = CancelToken::new();
        // The built-in engine inspects; it opens no document.
        let builtin = Provider::default();
        assert!(builtin.open_document(&request, &cancel).unwrap().is_none());
        assert!(
            builtin
                .inspect(&request, &InspectRequest::default(), &cancel)
                .unwrap()
                .is_some()
        );
        // MuPDF's books are documents: the reader opens them, never inspect.
        let mupdf = Provider {
            engine: Engine::Mupdf,
        };
        assert!(
            mupdf
                .inspect(&request, &InspectRequest::default(), &cancel)
                .unwrap()
                .is_none()
        );
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
                .map(|r| (r.cells[0].as_str(), r.depth, r.section))
                .collect::<Vec<_>>(),
            // Each knows the chapter it opens, its name escaped included.
            [
                ("Start", 0, Some(0)),
                ("Middle", 1, Some(1)),
                ("End", 0, Some(2))
            ]
        );
        // What the package says about the book reaches the panel's Properties;
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

    /// A part's heading, which leads nowhere, is in the contents and its
    /// chapters under it; an entry with neither a link nor a name nests
    /// nothing; the contents' own heading is not an entry.
    #[test]
    fn a_heading_over_chapters_holds_them() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic book in parts.epub");
        let nav = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><h1>Contents</h1><ol>
<li><span>Part one</span><ol><li><a href="one.xhtml">Start</a><ol><li><a href="one.xhtml#s">Section</a></li></ol></li></ol></li>
<li><a>Part two</a><ol><li><a href="two.xhtml">Middle</a></li></ol></li>
<li><ol><li><a href="sub/caf%C3%A9%20three.xhtml">End</a></li></ol></li>
</ol></nav></body></html>"#;
        write(
            &path,
            &[
                ("mimetype", b"application/epub+zip"),
                ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
                ("EPUB/book.opf", OPF.as_bytes()),
                ("EPUB/nav.xhtml", nav.as_bytes()),
                (
                    "EPUB/one.xhtml",
                    br#"<html><body><p>One.</p><h2 id="s">Section</h2></body></html>"#,
                ),
                ("EPUB/two.xhtml", br"<html><body><p>Two.</p></body></html>"),
                (
                    "EPUB/sub/café three.xhtml",
                    br"<html><body><p>Three.</p></body></html>",
                ),
            ],
        );
        let first = inspect(&path, &InspectRequest::default()).unwrap();
        assert_eq!(
            first
                .rows
                .iter()
                .map(|r| (r.cells[0].as_str(), r.depth, r.target.is_some(), r.section))
                .collect::<Vec<_>>(),
            [
                ("Part one", 0, false, None),
                ("Start", 1, true, Some(0)),
                ("Section", 2, true, Some(0)),
                ("Part two", 0, false, None),
                ("Middle", 1, true, Some(1)),
                ("End", 0, true, Some(2)),
            ]
        );
    }

    /// Contents are the table of contents: not the landmarks, not the list
    /// of printed pages, not the NCX's second copy of the chapters.
    #[test]
    fn contents_are_the_table_of_contents_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic paginated book.epub");
        let opf = OPF
            .replace(
                r#"<item id="nav""#,
                r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="nav""#,
            )
            .replace("<spine>", r#"<spine toc="ncx">"#);
        let nav = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>
<nav epub:type="toc"><ol><li><a href="one.xhtml">Start</a></li><li><a href="two.xhtml">Middle</a></li></ol></nav>
<nav epub:type="landmarks"><ol><li><a epub:type="bodymatter" href="one.xhtml">Begin reading</a></li></ol></nav>
<nav epub:type="page-list"><ol><li><a href="one.xhtml#p1">1</a></li><li><a href="two.xhtml#p2">2</a></li></ol></nav>
</body></html>"#;
        let ncx = r#"<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><head/><docTitle><text>Synthetic</text></docTitle>
<navMap><navPoint id="n1" playOrder="1"><navLabel><text>Start</text></navLabel><content src="one.xhtml"/></navPoint><navPoint id="n2" playOrder="2"><navLabel><text>Middle</text></navLabel><content src="two.xhtml"/></navPoint></navMap>
<pageList><pageTarget id="p1" type="normal" value="1" playOrder="3"><navLabel><text>1</text></navLabel><content src="one.xhtml#p1"/></pageTarget></pageList></ncx>"#;
        write(
            &path,
            &[
                ("mimetype", b"application/epub+zip"),
                ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
                ("EPUB/book.opf", opf.as_bytes()),
                ("EPUB/nav.xhtml", nav.as_bytes()),
                ("EPUB/toc.ncx", ncx.as_bytes()),
                ("EPUB/one.xhtml", br"<html><body><p>One.</p></body></html>"),
                ("EPUB/two.xhtml", br"<html><body><p>Two.</p></body></html>"),
            ],
        );
        let first = inspect(&path, &InspectRequest::default()).unwrap();
        let titles: Vec<_> = first.rows.iter().map(|r| r.cells[0].as_str()).collect();
        assert_eq!(titles, ["Start", "Middle"]);
        // The printed pages are the page list, each with its chapter.
        let pages = |inspection: &Inspection| {
            inspection
                .printed_pages
                .iter()
                .map(|p| (p.label.clone(), p.section, p.target.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            pages(&first),
            [
                ("1".to_owned(), 0, "/EPUB/one.xhtml#p1".to_owned()),
                ("2".to_owned(), 1, "/EPUB/two.xhtml#p2".to_owned())
            ]
        );
        // A book with only the EPUB 2 navigation reads its NCX `navMap`.
        let nav_free = opf.replace(
            r#"<item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#,
            "",
        );
        write(
            &path,
            &[
                ("mimetype", b"application/epub+zip"),
                ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
                ("EPUB/book.opf", nav_free.as_bytes()),
                ("EPUB/toc.ncx", ncx.as_bytes()),
                ("EPUB/one.xhtml", br"<html><body><p>One.</p></body></html>"),
                ("EPUB/two.xhtml", br"<html><body><p>Two.</p></body></html>"),
            ],
        );
        let first = inspect(&path, &InspectRequest::default()).unwrap();
        let titles: Vec<_> = first.rows.iter().map(|r| r.cells[0].as_str()).collect();
        assert_eq!(titles, ["Start", "Middle"]);
        // ... and its `pageList`.
        assert_eq!(
            pages(&first),
            [("1".to_owned(), 0, "/EPUB/one.xhtml#p1".to_owned())]
        );
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
        // zip 9 refuses a member past its declared size; were it to trust the
        // lie further, the bound on what is read would stop it.
        let error = inspect(&path, &InspectRequest::default())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("larger than its declared uncompressed size")
                || error.contains("EPUB container exceeds 16 MiB"),
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
        // An extra entry replaces the chapter of the same name.
        entries.retain(|(name, _)| !extra.iter().any(|(extra, _)| extra == name));
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
        assert_eq!(source.kind, skwir_core::inspection::InspectionKind::Code);
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

    /// A chapter reads with the book's own stylesheet, found inside the
    /// container through the chapter's relative link, and as XML: a
    /// `<title/>` closes itself instead of swallowing the chapter.
    #[test]
    fn a_chapter_reads_with_its_stylesheet_and_as_xhtml() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic styled book.epub");
        let chapter = br#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title/><link rel="stylesheet" type="text/css" href="../Styles/style.css"/></head>
<body><p class="c">CHAPTER ONE</p><p>She said <span class="i">never</span>.</p><p class="kf8">Kindle only needle</p></body></html>"#;
        sigil(
            &path,
            &[
                (
                    "OEBPS/Styles/style.css",
                    b".i { font-style: italic } .c { text-align: center } .kf8 { display: none }",
                ),
                ("OEBPS/Text/one.xhtml", chapter),
            ],
        );
        let first = inspect(&path, &InspectRequest::default()).unwrap();
        let texts: Vec<_> = first.blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, ["CHAPTER ONE", "She said never."]);
        assert_eq!(first.blocks[0].align, skwir_core::inspection::Align::Center);
        let never = &first.blocks[1].runs[1];
        assert!(never.italic && first.blocks[1].text[never.range.clone()] == *"never");
        // What the book hides is not a match either.
        let search = inspect(
            &path,
            &InspectRequest {
                query: Some("needle".into()),
                ..InspectRequest::default()
            },
        )
        .unwrap();
        assert_eq!(search.notice, "No matching chapter");
    }

    /// A fixed-layout book whose pages are pictures (a comic) shows a page
    /// as a page: the picture, the width of the panel.
    fn fixed(path: &std::path::Path, pages: &[(&str, &[u8])]) {
        fixed_with(path, NAV, pages);
    }

    fn fixed_with(path: &std::path::Path, nav: &str, pages: &[(&str, &[u8])]) {
        let opf = OPF.replace(
            "<dc:language>en</dc:language>",
            r#"<dc:language>en</dc:language><meta property="rendition:layout">pre-paginated</meta>"#,
        );
        let mut entries: Vec<(&str, &[u8])> = vec![
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
            ("EPUB/book.opf", opf.as_bytes()),
            ("EPUB/nav.xhtml", nav.as_bytes()),
            ("EPUB/images/p1.png", PNG),
        ];
        entries.extend_from_slice(pages);
        write(path, &entries);
    }

    fn session(path: &std::path::Path) -> Arc<dyn skwir_core::document::DocumentSession> {
        let request = PreviewRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: None,
            fit: None,
        };
        let provider = Provider::default();
        let cancel = CancelToken::new();
        assert!(
            provider
                .inspect(&request, &InspectRequest::default(), &cancel)
                .unwrap()
                .is_none(),
            "a book of pages is not read as chapters"
        );
        provider
            .open_document(&request, &cancel)
            .unwrap()
            .expect("a book of pages")
    }

    /// A chapter names its pictures, as many as it has, with the size they
    /// are shown at; each is decoded only when asked for by its member.
    #[test]
    fn pictures_are_named_for_later_and_decoded_by_their_member() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic illustrated book.epub");
        let count = limits::EMBEDDED_IMAGES + 4;
        let chapter = (0..count).fold(String::from("<html><body>"), |mut html, i| {
            use std::fmt::Write;
            let _ = write!(
                html,
                r#"<p>Plate {i}</p><img src="../Images/plate%20{i}.png" alt="Plate"/>"#
            );
            html
        }) + "</body></html>";
        let names: Vec<String> = (0..count)
            .map(|i| format!("OEBPS/Images/plate {i}.png"))
            .collect();
        let mut extra: Vec<(&str, &[u8])> = vec![("OEBPS/Text/one.xhtml", chapter.as_bytes())];
        extra.extend(names.iter().map(|name| (name.as_str(), PNG)));
        sigil(&path, &extra);
        let first = inspect(&path, &InspectRequest::default()).unwrap();
        let later: Vec<_> = first
            .blocks
            .iter()
            .filter_map(|b| match &b.picture {
                Some(Picture::Later { key, width, height }) => Some((key.clone(), *width, *height)),
                _ => None,
            })
            .collect();
        assert_eq!(later.len(), count, "no picture is left out");
        assert_eq!(later[3], ("OEBPS/Images/plate 3.png".to_owned(), 2, 3));
        let request = PreviewRequest {
            entry: EntryRef::from_path(&path).unwrap(),
            mime: None,
            page: 1,
            seconds: None,
            max_side: 64,
            text_size: None,
            fit: None,
        };
        let cancel = CancelToken::new();
        let view = InspectRequest::default();
        let decoded = Provider::default()
            .picture(&request, &view, &later[3].0, &cancel)
            .unwrap()
            .unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 3));
        assert!(
            Provider::default()
                .picture(&request, &view, "../outside.png", &cancel)
                .is_err(),
            "a key is a member, never a path out of the container"
        );
    }

    /// A font is restored whatever opened the book: a fixed-layout page's
    /// drawing and a search open the container afresh, without the
    /// package being read first, and found no identifier to key it by.
    #[test]
    fn an_obfuscated_font_is_restored_in_a_container_opened_afresh() {
        let font: Vec<u8> = (0..1500u32).map(|i| (i % 241) as u8).collect();
        let mut stored = font.clone();
        deobfuscate(&mut stored, IDPF_OBFUSCATION, "urn:example:synthetic").unwrap();
        let encryption = format!(
            r#"<?xml version="1.0"?><encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData><enc:EncryptionMethod Algorithm="{IDPF_OBFUSCATION}"/><enc:CipherData><enc:CipherReference URI="EPUB/fonts/Libre%20Été.otf"/></enc:CipherData></enc:EncryptedData></encryption>"#
        );
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fonts.epub");
        write(
            &path,
            &[
                ("mimetype", b"application/epub+zip"),
                ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
                ("META-INF/encryption.xml", encryption.as_bytes()),
                ("EPUB/book.opf", OPF.as_bytes()),
                ("EPUB/fonts/Libre Été.otf", &stored),
            ],
        );
        let cancel = CancelToken::new();
        let mut container = Container::open(&path, &cancel).unwrap();
        assert_eq!(
            container.file("EPUB/fonts/Libre%20Été.otf", &cancel),
            Some(font)
        );
    }

    /// Obfuscation is its own inverse, keyed by the package's identifier:
    /// IDPF's by its SHA-1 over 1040 bytes, Adobe's by its UUID over 1024.
    #[test]
    fn obfuscated_fonts_are_restored() {
        let font: Vec<u8> = (0..2000u32).map(|i| (i % 251) as u8).collect();
        for (algorithm, identifier, length) in [
            (IDPF_OBFUSCATION, " urn:isbn:9780000000000 \n", 1040),
            (
                ADOBE_OBFUSCATION,
                "urn:uuid:0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
                1024,
            ),
        ] {
            let mut bytes = font.clone();
            deobfuscate(&mut bytes, algorithm, identifier).unwrap();
            assert_ne!(bytes[..length], font[..length]);
            assert_eq!(bytes[length..], font[length..], "only the head is keyed");
            deobfuscate(&mut bytes, algorithm, identifier).unwrap();
            assert_eq!(bytes, font);
        }
        let mut drm = font.clone();
        assert!(
            deobfuscate(&mut drm, "http://www.w3.org/2001/04/xmlenc#aes128-cbc", "x").is_none()
        );
    }

    /// A fixed-layout book is a document of pages: each drawn at the side
    /// the reader asks, its own size what 100% is, its contents leading to
    /// its pages, its text found by page.
    #[test]
    fn a_fixed_layout_book_is_a_document_of_pages() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic drawn book.epub");
        let page = br#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title/><meta name="viewport" content="width=300,height=400"/>
<style>body { margin: 0 } img { position: absolute; left: 0; top: 0; width: 300px; height: 400px }
.t { position: absolute; left: 20px; top: 20px; color: #123456 }</style></head>
<body><img src="images/p1.png" alt=""/><div class="t">Lettering</div></body></html>"#;
        fixed(
            &path,
            &[
                ("EPUB/one.xhtml", page),
                (
                    "EPUB/two.xhtml",
                    br"<html><body><p>No size stated.</p></body></html>",
                ),
            ],
        );
        let book = session(&path);
        let cancel = CancelToken::new();
        assert_eq!(book.info().pages, 3);
        assert_eq!(
            book.info()
                .outlines
                .iter()
                .map(|o| (o.title.as_str(), o.depth, o.page))
                .collect::<Vec<_>>(),
            [
                ("Start", 0, Some(1)),
                ("Middle", 1, Some(2)),
                ("End", 0, Some(3))
            ]
        );
        for side in [800, 1600] {
            let drawn = book.render(1, side, &cancel).unwrap();
            assert_eq!(drawn.height, side, "drawn at the side asked");
            assert_eq!(drawn.width * 4, side * 3);
            assert_eq!(drawn.pixels.len() as u32, drawn.width * drawn.height * 4);
            assert_eq!((drawn.content.width, drawn.content.height), (300., 400.));
            assert!(
                drawn.content.chars.is_empty(),
                "a drawn page has no text layer"
            );
        }
        // A page that states no size is drawn at the book's.
        let second = book.render(2, 800, &cancel).unwrap();
        assert_eq!((second.content.width, second.content.height), (300., 400.));
        // The third page is missing from the archive: an error of its own.
        assert!(book.render(3, 800, &cancel).is_err());
        let pages = |query| {
            book.search(query, &cancel)
                .unwrap()
                .into_iter()
                .map(|hit| hit.page)
                .collect::<Vec<_>>()
        };
        assert_eq!(pages("LETTERING"), [1]);
        assert_eq!(pages("no size"), [2]);
        assert!(pages("absent").is_empty());
    }

    /// A book of pages names them as its page list does: a page without a
    /// printed number has none.
    #[test]
    fn a_fixed_layout_book_names_its_pages_as_printed() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic numbered book.epub");
        let nav = NAV.replace(
            "</body>",
            r#"<nav epub:type="page-list"><ol><li><a href="one.xhtml">i</a></li><li><a href="two.xhtml">ii</a></li></ol></nav></body>"#,
        );
        let page = br#"<html><head><meta name="viewport" content="width=30,height=40"/></head><body><p>Page</p></body></html>"#;
        fixed_with(
            &path,
            &nav,
            &[("EPUB/one.xhtml", page), ("EPUB/two.xhtml", page)],
        );
        assert_eq!(session(&path).info().labels, ["i", "ii", ""]);
    }

    /// A page that states no size and holds one picture (a comic's page) is
    /// that picture, at its own size.
    #[test]
    fn a_fixed_layout_picture_page_is_its_picture() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic comic book.epub");
        fixed(
            &path,
            &[(
                "EPUB/one.xhtml",
                br#"<html><body><img src="images/p1.png" alt=""/></body></html>"#,
            )],
        );
        let page = session(&path).render(1, 800, &CancelToken::new()).unwrap();
        assert_eq!((page.content.width, page.content.height), (2., 3.));
        assert_eq!(
            (page.width, page.height),
            (2, 3),
            "a picture is never enlarged"
        );
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

    /// A book whose package lists `manifest` (with a decoy picture first)
    /// and `metadata`, with the pictures it names, and `extra` members.
    fn covered(path: &std::path::Path, metadata: &str, manifest: &str, extra: &[(&str, &[u8])]) {
        const DECOY: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="10"/>"#;
        let opf = OPF
            .replace(
                "<dc:language>en</dc:language>",
                &format!("<dc:language>en</dc:language>{metadata}"),
            )
            .replace(
                "<manifest>",
                &format!(
                    r#"<manifest><item id="decoy" href="Images/decoy.svg" media-type="image/svg+xml"/>{manifest}"#
                ),
            );
        let mut entries: Vec<(&str, &[u8])> = vec![
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
            ("EPUB/book.opf", opf.as_bytes()),
            ("EPUB/nav.xhtml", NAV.as_bytes()),
            ("EPUB/one.xhtml", b"<html><body><p>One</p></body></html>"),
            ("EPUB/Images/decoy.svg", DECOY),
            ("EPUB/Images/c\u{f4}ver art.png", PNG),
        ];
        entries.extend_from_slice(extra);
        write(path, &entries);
    }

    fn cover_of(path: &std::path::Path) -> Option<(u32, u32)> {
        let req = ThumbRequest {
            entry: EntryRef::from_path(path).unwrap(),
            mime: None,
            mtime: None,
            size: skwir_core::thumbs::ThumbSize::Normal,
        };
        assert!(Cover.accepts(&req));
        Cover
            .thumbnail(&req, &CancelToken::new())
            .unwrap()
            .map(|image| (image.width, image.height))
    }

    #[test]
    fn a_cover_is_the_picture_the_package_names() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("Synthetic \u{2019}book\u{2019}.epub");
        let cover = r#"<item id="art" href="Images/c%C3%B4ver%20art.png" media-type="image/png""#;
        // EPUB 3: the manifest item whose properties say so.
        covered(
            &path,
            "",
            &format!(r#"{cover} properties="cover-image"/>"#),
            &[],
        );
        assert_eq!(cover_of(&path), Some((2, 3)));
        // EPUB 2: the item `<meta name="cover">` names, by id or by href.
        covered(
            &path,
            r#"<meta name="cover" content="art"/>"#,
            &format!("{cover}/>"),
            &[],
        );
        assert_eq!(cover_of(&path), Some((2, 3)));
        covered(
            &path,
            r#"<meta name="cover" content="Images/c%C3%B4ver%20art.png"/>"#,
            &format!("{cover}/>"),
            &[],
        );
        assert_eq!(cover_of(&path), Some((2, 3)));
        // Nothing named: no guess among the book's pictures.
        covered(&path, "", &format!("{cover}/>"), &[]);
        assert_eq!(cover_of(&path), None);
        // A cover page is a chapter, not a picture.
        covered(
            &path,
            "",
            r#"<item id="page" href="one.xhtml" media-type="application/xhtml+xml" properties="cover-image"/>"#,
            &[],
        );
        assert_eq!(cover_of(&path), None);
    }

    #[test]
    fn an_encrypted_cover_is_not_shown() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("locked.epub");
        let encryption = br#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:enc="http://www.w3.org/2001/04/xmlenc#"><enc:EncryptedData><enc:EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/><enc:CipherData><enc:CipherReference URI="EPUB/Images/c%C3%B4ver%20art.png"/></enc:CipherData></enc:EncryptedData></encryption>"#;
        covered(
            &path,
            "",
            r#"<item id="art" href="Images/c%C3%B4ver%20art.png" media-type="image/png" properties="cover-image"/>"#,
            &[("META-INF/encryption.xml", encryption)],
        );
        assert_eq!(cover_of(&path), None);
    }
}
