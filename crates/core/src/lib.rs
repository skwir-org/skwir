//! bibli core.
//!
//! Only what two plugins must agree on lives here:
//!
//! - [`entry`]: entries with stable identity and provenance-tagged metadata.
//! - [`location`]: locations, the `LocationProvider` trait, and streaming listings.
//! - [`fs`]: the built-in path provider (getdents streaming, lazy stat, watching).
//! - [`metadata`]: providers that fill entry fields during hydration;
//!   [`notes`]: a file's tags, rating and comment, and the stores that keep them.
//! - [`command`]: commands as data, the registry, and dispatch.
//! - [`op`]: operations with inverses, plans, the executor, and the undo journal.
//! - [`registry`]: views, panels, places, menus, keymap, settings schema;
//!   [`keymap`]: the user's key bindings, the keymap's last layer.
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
pub mod companions;
pub mod document;
pub mod entry;
pub mod fields;
pub mod fs;
pub mod geo;
pub mod hints;
pub mod keymap;
pub mod location;
pub mod lru;
pub mod media;
pub mod metadata;
pub mod notes;
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
pub use entry::{Entry, EntryId, EntryKind, LinkTarget, Metadata, Provenance, Value};
pub use location::{
    Crumb, EmptyState, Listing, ListingEvent, Location, LocationProvider, Narrowing, OpenLocation,
    SortKey, SortSpec, Toggle, Trail, Verb, Verbs,
};
pub use op::{Operation, Plan};

pub mod inspection;
