//! Custom GPUI elements. Anything whose layout depends on its own size is
//! decided here, in `prepaint`, where the bounds are real (see `view.rs`).

pub mod document;
pub mod treemap;
pub mod uniform_grid;

pub mod adaptive;
pub mod chips;
pub mod clip;
pub mod edge;
pub mod halo;
pub mod map;
pub mod marquee;
pub mod model;
pub mod picture;
pub mod pill;
pub mod popover;
pub mod profile;
pub mod rows;
pub mod scrollbar;
pub mod terminal;
pub mod zoom_wheel;

/// The id of an element showing `what`, the same from one frame to the
/// next while it shows that, wherever it stands: gpui keeps an element's
/// tooltip as it was built, its press and its screen reader's node by its
/// id, so an element known by its position hands them to whatever comes
/// to stand there (`docs/invariants/shell.md`). `what` tells it from its
/// siblings: where two may show the same, it holds their position too.
pub fn known_by(name: &'static str, what: impl std::hash::Hash) -> gpui::ElementId {
    gpui::ElementId::NamedInteger(name.into(), key_of(what))
}

/// What [`known_by`] knows `what` by, for an id that is a name (a
/// toolbar button's, which its popup is anchored to).
pub fn key_of(what: impl std::hash::Hash) -> u64 {
    use std::hash::BuildHasher;
    std::hash::BuildHasherDefault::<std::hash::DefaultHasher>::default().hash_one(what)
}
