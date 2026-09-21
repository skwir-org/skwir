//! bibli platform adapters.
//!
//! Every module exposes one cross-platform API. Linux has real
//! implementations; other targets get stubs that return nothing, so the crate
//! builds everywhere and Phase 5 fills them in.
//!
//! - [`mime`]: type detection by name (no I/O) or by content.
//! - [`icons`]: XDG icon theme lookup with caching.
//! - [`thumbnails`]: the freedesktop thumbnail cache: lookup, load, store.
//! - [`thumb_providers`]: thumbnail providers for the core service: cache, image decoder, external thumbnailers.
//! - [`places`]: home, XDG user dirs, GTK bookmarks, mounted volumes.
//! - [`open`]: open with the default application, open a terminal.

pub mod document;
pub mod exif;
pub mod icons;
pub mod media;
pub mod mime;
pub mod open;
pub mod places;
pub mod preview;
pub mod process;
pub mod thumb_providers;
pub mod thumbnails;
pub mod users;

pub mod inspection;
/// Every preview bound in one place (see `inspection::limits`).
pub use inspection::limits;

pub mod rich_preview;

pub mod archive;
