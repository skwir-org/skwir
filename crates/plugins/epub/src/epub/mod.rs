//! EPUB 2/3: the package parsed by rbook, chapters read as XHTML, the
//! table of contents and printed pages, covers.
//!
//! Every byte decompressed from the container passes through one bounded
//! reader. rbook never touches the file: it parses a small in-memory package
//! holding only the entries it needs (container.xml, the OPF and its contents
//! documents), each extracted here with the same bound as every chapter.
use std::{collections::HashMap, io::Write, sync::Arc};

use skwir_core::{
    CancelToken,
    inspection::{Picture, PrintedPage, Row},
    preview::FileSource,
};
use skwir_platform::{
    inspection::{self, limits},
    package::normalize,
    rich_preview::{self, Markup},
};

use crate::book::{Book, ChapterRef, Sections, Sheets, member_key, parse_xml, resolve};

#[cfg(test)]
mod tests;

/// A reflowing EPUB as reading mode reads it: its container, its package
/// and its spine.
pub(crate) struct EpubBook {
    container: Container,
    book: rbook::Epub,
    chapters: Vec<ChapterRef>,
}

impl EpubBook {
    /// The book in `file`; `None` for one laid out in pages, a document
    /// (`pages::FixedBook`).
    pub(crate) fn open<S: FileSource + ?Sized>(
        file: &S,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Self>> {
        let mut container = Container::open(file, cancel)?;
        let (book, fixed) = package(&mut container, cancel)?;
        if fixed {
            return Ok(None);
        }
        let chapters = book
            .spine()
            .iter()
            .filter_map(|s| s.manifest_entry())
            .map(|c| ChapterRef {
                href: c.href().as_str().to_owned(),
                markup: if c.media_type() == "text/html" {
                    Markup::Html
                } else {
                    Markup::Xhtml
                },
            })
            .collect();
        Ok(Some(Self {
            container,
            book,
            chapters,
        }))
    }
}

impl Book for EpubBook {
    type Resources<'a> = Chapter<'a>;
    const FORMAT: &'static str = "EPUB";
    const SOURCE: &'static str = "xhtml";
    fn chapters(&self) -> &[ChapterRef] {
        &self.chapters
    }
    fn text(
        &mut self,
        href: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<(String, Option<String>)> {
        let bytes = self.container.read(href, "Chapter", cancel)?;
        let (text, encoding) = inspection::decode(&bytes);
        Ok((text.into_owned(), encoding.notice()))
    }
    fn resources<'a>(
        &'a mut self,
        base: &'a str,
        images: bool,
        sheets: &'a mut Sheets,
        cancel: &'a CancelToken,
    ) -> Chapter<'a> {
        Chapter {
            container: &mut self.container,
            base,
            cancel,
            images,
            sheets,
        }
    }
    fn contents(&mut self) -> Vec<Row> {
        table_of_contents(&self.book)
    }
    fn printed_pages(&mut self, sections: &Sections) -> Vec<PrintedPage> {
        printed_pages(&self.book, sections)
    }
    /// What the package says about the book.
    fn details(&mut self) -> Vec<(&'static str, Option<String>)> {
        let metadata = self.book.metadata();
        let people = |names: Vec<&str>| names.join(", ");
        vec![
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
        ]
    }
}

/// The picture a chapter named by its member, read through the same
/// bounded reader, never outside the container.
pub(crate) fn picture<S: FileSource + ?Sized>(
    file: &S,
    key: &str,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<u8>> {
    Container::open(file, cancel)?.read(key, "Embedded image", cancel)
}

/// The book's cover picture, as the package names it: nothing for a book
/// that names none, or encrypts it.
pub(crate) fn cover_picture<S: FileSource + ?Sized>(
    file: &S,
    cancel: &CancelToken,
) -> anyhow::Result<Option<Vec<u8>>> {
    let mut container = Container::open(file, cancel)?;
    let Some(href) = cover(&mut container, cancel)? else {
        return Ok(None);
    };
    Ok(container.file(&href, cancel))
}

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

/// What a chapter names inside the container, read through its bounded
/// reader: pictures (unless only the text is wanted) and stylesheets.
/// Nothing outside the container is ever named.
pub(crate) struct Chapter<'a> {
    pub(crate) container: &'a mut Container,
    pub(crate) base: &'a str,
    pub(crate) cancel: &'a CancelToken,
    pub(crate) images: bool,
    /// The book's stylesheets parsed, by member and the folder their URLs
    /// are found at: a search parses each once, not once a chapter.
    pub(crate) sheets: &'a mut Sheets,
}

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

/// The ZIP container, read as every package is
/// (`skwir_platform::package`: bounded, members by normalized name, so
/// hrefs never become archive lookups through a path the ZIP library
/// interprets).
pub(crate) struct Container {
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
    /// The book `file`: what a preview holds of it, or its path (a
    /// cover's).
    pub(crate) fn open<S: FileSource + ?Sized>(
        file: &S,
        cancel: &CancelToken,
    ) -> anyhow::Result<Self> {
        // Declared sizes are not checked here: a book's pictures, audio or
        // video are never read unless a chapter shows them, and `read`
        // bounds the bytes each read actually produces.
        let mut container = Self {
            package: skwir_platform::package::Package::open(file, "This EPUB", false, cancel)?,
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
    /// One `encryption.xml` lists as encrypted (a protected book's
    /// chapters) is refused, saying so: read raw, it showed as garbage. A
    /// font only obfuscated is read, for `file` to restore.
    pub(crate) fn read(
        &mut self,
        href: &str,
        what: &str,
        cancel: &CancelToken,
    ) -> anyhow::Result<Vec<u8>> {
        let name =
            member_key(href).ok_or_else(|| anyhow::anyhow!("Missing EPUB resource {href}"))?;
        anyhow::ensure!(self.package.has(&name), "Missing EPUB resource {href}");
        if self.encrypted.get(&name).is_some_and(|algorithm| {
            ![IDPF_OBFUSCATION, ADOBE_OBFUSCATION].contains(&algorithm.as_str())
        }) {
            anyhow::bail!(
                "This EPUB is protected: its {} is encrypted",
                what.to_lowercase()
            );
        }
        self.package.read(&name, limits::BINARY_INPUT, what, cancel)
    }
}
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
pub(crate) fn package(
    container: &mut Container,
    cancel: &CancelToken,
) -> anyhow::Result<(rbook::Epub, bool)> {
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
pub(crate) fn table_of_contents(book: &rbook::Epub) -> Vec<Row> {
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
pub(crate) fn printed_pages(book: &rbook::Epub, sections: &Sections) -> Vec<PrintedPage> {
    /// Pages kept: more than any printed book has.
    const PAGES: usize = 20_000;
    fn walk(
        entry: rbook::epub::toc::EpubTocEntry<'_>,
        depth: usize,
        sections: &Sections,
        pages: &mut Vec<PrintedPage>,
    ) {
        if depth > limits::NESTING || pages.len() >= PAGES {
            return;
        }
        if let Some(href) = entry.href() {
            let target = href.as_str();
            let key = member_key(target);
            if let Some(section) = sections.of_key(key.as_deref()) {
                pages.push(PrintedPage {
                    label: entry.label().trim().to_owned(),
                    section: section as u64,
                    target: target.to_owned(),
                });
            }
        }
        for child in entry {
            walk(child, depth + 1, sections, pages);
        }
    }
    let mut pages = Vec::new();
    if let Some(root) = book.toc().page_list() {
        walk(root, 0, sections, &mut pages);
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
