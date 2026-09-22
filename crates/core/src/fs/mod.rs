//! The built-in filesystem provider: getdents streaming, lazy statx, watching.

mod hydrate;
mod path_provider;
pub mod stat;
mod watch;

pub use hydrate::hydrate_paths;
pub use path_provider::PathProvider;
pub use stat::{stat_entry, stat_path};
