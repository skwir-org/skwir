//! The built-in filesystem provider: getdents streaming, lazy statx, watching.

mod hydrate;
mod path_provider;
pub mod stat;
pub mod volume;
mod watch;

pub use hydrate::{hydrate_path, hydrate_paths};
pub use path_provider::PathProvider;
pub use stat::{Statter, content_path, stat_entry, stat_path};
