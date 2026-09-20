//! Pane views: list, grid, columns. One trait, registered as `ViewSpec`s.
//!
//! Rule: never derive layout from a measured size in `render`; size-dependent
//! layout lives in an element's `prepaint`, where the bounds are real (see
//! `elements::uniform_grid` and `elements::treemap`). The only size a view may
//! read outside prepaint is what its own element recorded in its last
//! prepaint (column count, visible rows), and only for keyboard navigation.
//!
//! This trait is the future `ui-api` surface for out-of-crate views; it is
//! kept in-crate until a third-party view exists (see the plan).

use std::collections::HashMap;
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
    order: i32,
    f: impl Fn() -> Box<dyn PaneView> + Send + Sync + 'static,
) -> ViewSpec {
    let factory: ViewFactory = Arc::new(f);
    let factory: Factory = Arc::new(factory);
    ViewSpec {
        id: id.into(),
        title: title.into(),
        icon: id.into(),
        order,
        revision: Arc::default(),
        factory,
    }
}

/// Instantiate a registered view, if its factory is one of ours.
pub fn instantiate(spec: &ViewSpec) -> Option<Box<dyn PaneView>> {
    if let Some(factory) = spec.factory.downcast_ref::<ViewFactory>() {
        return Some(factory());
    }
    let renderer = spec
        .factory
        .downcast_ref::<bibli_core::registry::ViewRenderer>()?;
    crate::views::named_renderer(renderer.0)
}

pub const LIST: &str = "list";
pub const GRID: &str = "grid";
pub const COLUMNS: &str = "columns";

/// Reconcile by registration identity, including disable/re-enable between two frames.
/// Returns removed/replaced ids so their associated pane state can be discarded.
pub(crate) fn reconcile(
    views: &mut HashMap<&'static str, Box<dyn PaneView>>,
    registrations: &mut HashMap<String, Factory>,
    specs: &[ViewSpec],
) -> Vec<String> {
    let mut replaced = Vec::new();
    registrations.retain(|id, factory| {
        let retained = specs
            .iter()
            .any(|s| &*s.id == id && Arc::ptr_eq(&s.factory, factory));
        if !retained {
            views.remove(id.as_str());
            replaced.push(id.clone());
        }
        retained
    });
    for spec in specs {
        if !registrations.contains_key(&*spec.id) {
            if let Some(view) = instantiate(spec) {
                views.insert(view.id(), view);
            }
            registrations.insert(spec.id.to_string(), spec.factory.clone());
        }
    }
    replaced
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_views_follow_registration_lifetime() {
        let mut views = HashMap::new();
        let mut registrations = HashMap::new();
        let usage = || ViewSpec::named("usage", "Usage", "usage", Arc::default());
        let spec = usage();
        reconcile(&mut views, &mut registrations, std::slice::from_ref(&spec));
        assert!(views.contains_key("usage"));
        assert!(reconcile(&mut views, &mut registrations, &[spec]).is_empty());
        assert_eq!(
            reconcile(&mut views, &mut registrations, &[usage()]),
            ["usage"]
        );
        assert_eq!(reconcile(&mut views, &mut registrations, &[]), ["usage"]);
        assert!(views.is_empty());
        reconcile(&mut views, &mut registrations, &[usage()]);
        assert!(views.contains_key("usage"));
        let unknown = ViewSpec::named("unavailable-renderer", "Unknown", "grid", Arc::default());
        reconcile(&mut views, &mut registrations, &[unknown]);
        assert!(views.is_empty());
    }
}
