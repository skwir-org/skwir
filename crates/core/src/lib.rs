//! bibli core.
//!
//! Only what two plugins must agree on lives here:
//!
//! - [`entry`]: entries with stable identity and provenance-tagged metadata.
//! - [`location`]: locations, the `LocationProvider` trait, and streaming listings.
//! - [`fs`]: the built-in path provider (getdents streaming, lazy stat, watching).
//! - [`metadata`]: providers that fill entry fields during hydration.
//! - [`command`]: commands as data, the registry, and dispatch.
//! - [`op`]: operations with inverses, plans, the executor, and the undo journal.
//! - [`registry`]: views, panels, places, menus, keymap, settings schema.
//! - [`plugin`]: the plugin lifecycle with runtime toggles.
//! - [`scheduler`]: the priority work queue with cancellation.
//! - [`cache`] and [`settings`]: the throwaway cache and namespaced settings.
//! - [`terminal`]: the screen a terminal shows and what it is sent (ADR-019).
//! - [`thumbs`]: the thumbnail service: providers, LRU of decoded images, viewport priority.
//! - [`workspace`]: panes, selection, focus; [`tabs`]: the tabs that group panes.
//! - [`collate`] and [`lru`]: the natural name order and the byte-budgeted LRU every cache shares.
//! - [`app`]: the object that ties them together and hosts the built-in commands.
//!
//! The crate is toolkit-agnostic and has no async runtime: work runs on the
//! scheduler's threads and results flow through channels.

pub mod app;
pub mod cache;
pub mod cancel;
pub mod collate;
pub mod command;
pub mod document;
pub mod entry;
pub mod fields;
pub mod fs;
pub mod geo;
pub mod location;
pub mod lru;
pub mod media;
pub mod metadata;
pub mod op;
pub mod os_path;
pub mod pending;
pub mod playing;
pub mod plugin;
pub mod preview;
pub mod registry;
pub mod scheduler;
pub mod settings;
pub mod tabs;
pub mod tally;
pub mod terminal;
pub mod thumbs;
pub mod tiles;
pub mod workspace;

pub use app::App;
pub use cancel::CancelToken;
pub use entry::{Entry, EntryId, EntryKind, Metadata, Provenance, Value};
pub use location::{
    Listing, ListingEvent, Location, LocationProvider, OpenLocation, SortKey, SortSpec,
};
pub use op::{Operation, Plan};

pub mod inspection;
