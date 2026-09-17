//! bibli core.
//!
//! Only what two plugins must agree on lives here:
//!
//! - [`entry`]: entries with stable identity and provenance-tagged metadata.
//! - [`location`]: locations, the `LocationProvider` trait, and streaming listings.
//! - [`fs`]: the built-in path provider (getdents streaming, lazy stat, watching).
//! - [`command`]: commands as data, the registry, and dispatch.
//! - [`op`]: operations with inverses, plans, the executor, and the undo journal.
//! - [`registry`]: views, panels, places, menus, keymap, settings schema.
//! - [`plugin`]: the plugin lifecycle with runtime toggles.
//! - [`scheduler`]: the priority work queue with cancellation.
//! - [`cache`] and [`settings`]: the throwaway cache and namespaced settings.
//! - [`workspace`]: panes, selection, focus.
//! - [`app`]: the object that ties them together and hosts the built-in commands.
//!
//! The crate is toolkit-agnostic and has no async runtime: work runs on the
//! scheduler's threads and results flow through channels.

pub mod app;
pub mod cache;
pub mod cancel;
pub mod command;
pub mod entry;
pub mod fs;
pub mod location;
pub mod op;
pub mod plugin;
pub mod registry;
pub mod scheduler;
pub mod settings;
pub mod workspace;

pub use app::App;
pub use cancel::CancelToken;
pub use entry::{Entry, EntryId, EntryKind, Metadata, Provenance, Value};
pub use location::{Listing, ListingEvent, Location, LocationProvider, OpenLocation};
pub use op::{Operation, Plan};
