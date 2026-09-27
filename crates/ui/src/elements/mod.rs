//! Custom GPUI elements. Anything whose layout depends on its own size is
//! decided here, in `prepaint`, where the bounds are real (see `view.rs`).

pub mod document;
pub mod treemap;
pub mod uniform_grid;

pub mod adaptive;
pub mod halo;
pub mod map;
pub mod marquee;
pub mod popover;
pub mod profile;
pub mod scrollbar;
pub mod terminal;
