//! Pane views: list, grid, columns. One trait, registered as `ViewSpec`s.
//!
//! This trait is the future `ui-api` surface for out-of-crate views; it is
//! kept in-crate until a third-party view exists (see the plan).

use std::sync::Arc;

use bibli_core::registry::{Factory, ViewSpec};
use gpui::{AnyElement, Context, Window};

use crate::shell::Shell;

/// Keyboard navigation a view interprets in its own geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
}

/// A way to render the active pane's listing.
pub trait PaneView {
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    /// Render the pane. The view is taken out of the shell for the call, so
    /// it gets the whole shell mutably.
    fn render(
        &mut self,
        shell: &mut Shell,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> AnyElement;
    /// Bring index `ix` of the sorted order into view.
    fn scroll_to(&mut self, ix: usize);
    /// Index delta for a navigation key (`None`: the view handles it in `nav`
    /// or ignores it).
    fn nav_delta(&self, nav: Nav, shell: &Shell) -> Option<isize>;
    /// A chance to handle navigation the view's own way. Returns true if handled.
    fn nav(
        &mut self,
        _nav: Nav,
        _shell: &mut Shell,
        _window: &mut Window,
        _cx: &mut Context<Shell>,
    ) -> bool {
        false
    }
    /// The pane's location changed.
    fn reset(&mut self) {}
}

/// What a view's `ViewSpec::factory` holds: a constructor.
pub type ViewFactory = Arc<dyn Fn() -> Box<dyn PaneView> + Send + Sync>;

pub fn spec(
    id: &'static str,
    title: &'static str,
    f: impl Fn() -> Box<dyn PaneView> + Send + Sync + 'static,
) -> ViewSpec {
    let factory: ViewFactory = Arc::new(f);
    let factory: Factory = Arc::new(factory);
    ViewSpec {
        id: id.into(),
        title: title.into(),
        factory,
    }
}

/// Instantiate a registered view, if its factory is one of ours.
pub fn instantiate(spec: &ViewSpec) -> Option<Box<dyn PaneView>> {
    spec.factory.downcast_ref::<ViewFactory>().map(|f| f())
}

pub const LIST: &str = "list";
pub const GRID: &str = "grid";
pub const COLUMNS: &str = "columns";
