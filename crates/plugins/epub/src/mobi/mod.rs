//! Mobipocket books (ADR-106): MOBI 6, KF8 (AZW3) and `PalmDOC` texts, all
//! Palm databases. A book whose text is encrypted (DRM) is refused, saying
//! so, and so is Amazon's KFX, which is not one.
use skwir_core::{CancelToken, preview::FileSource};

pub(crate) mod palmdb;

use palmdb::{Compression, Database, Header, Kind};

/// Bytes of a book's first record read to learn how it is stored: its
/// `PalmDOC` header and the start of its MOBI header.
const FIRST: usize = 64;

/// Whether `MuPDF` reads the book in `file` (ADR-106): a MOBI 6 book or a
/// `PalmDOC` text, stored as is or with `PalmDOC`'s compression, and not
/// encrypted. A file holding MOBI 6 and KF8 is read through its MOBI 6 part;
/// `MuPDF` reads nothing of a KF8 book (AZW3) and refuses Huffman-coded
/// text. Told from the file's first record, before anything opens it.
pub(crate) fn mupdf_reads<S: FileSource + ?Sized>(file: &S, cancel: &CancelToken) -> bool {
    let read = || -> anyhow::Result<bool> {
        let database = Database::open(file, cancel)?;
        if !matches!(database.kind, Kind::Mobi | Kind::Text) {
            return Ok(false);
        }
        let header = Header::parse(&database.record_head(file, 0, FIRST, cancel)?)?;
        Ok(!header.encrypted
            && matches!(header.compression, Compression::None | Compression::PalmDoc)
            && header.mobi.as_ref().is_none_or(|mobi| mobi.version < 8))
    };
    read().unwrap_or(false)
}

/// Whether `head`, a file's first bytes, is a Mobipocket book or a `PalmDOC`
/// text: what a Palm database (`.pdb`, `.prc`) must be to be read as one.
pub(crate) fn is_book(head: &[u8]) -> bool {
    matches!(Kind::of(head), Kind::Mobi | Kind::Text)
}
