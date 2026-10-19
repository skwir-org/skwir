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
