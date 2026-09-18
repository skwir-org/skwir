//! Context menu from the registry's `context` menu contributions, filtered by
//! `when` tokens evaluated against the selection.

use bibli_core::registry::MenuItem;
use gpui::{
    AnyElement, Context, MouseButton, Pixels, Point, anchored, deferred, div, prelude::*, px,
};

use crate::shell::Shell;
use crate::theme::Theme;

/// What the selection looks like, for `when` evaluation and key contexts.
#[derive(Clone, Copy, Debug, Default)]
pub struct SelectionCtx {
    pub count: usize,
    pub all_dirs: bool,
    pub all_files: bool,
}

impl SelectionCtx {
    /// Flags added to the pane key context.
    pub fn flags(self) -> Vec<&'static str> {
        let mut f = Vec::new();
        if self.count == 0 {
            f.push("none");
        } else {
            f.push("selection");
            if self.count == 1 {
                f.push("single");
            }
            if self.all_dirs {
                f.push("dir");
            }
            if self.all_files {
                f.push("file");
            }
        }
        f
    }
}

/// Space-separated tokens, all of which must hold. `None` or `none` always holds.
pub fn when_holds(when: Option<&str>, ctx: SelectionCtx) -> bool {
    let Some(when) = when else { return true };
    when.split_whitespace().all(|t| match t {
        "none" => true,
        "selection" => ctx.count > 0,
        "single" => ctx.count == 1,
        "dir" => ctx.count > 0 && ctx.all_dirs,
        "file" => ctx.count > 0 && ctx.all_files,
        _ => false,
    })
}

pub struct MenuState {
    pub position: Point<Pixels>,
    pub items: Vec<MenuItem>,
}

impl Shell {
    pub fn open_context_menu(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let ctx = self.selection_ctx();
        let mut items: Vec<MenuItem> = self
            .app
            .registry
            .menus
            .all()
            .into_iter()
            .filter(|m| m.menu == "context" && when_holds(m.when.as_deref(), ctx))
            .collect();
        items.sort_by_key(|m| m.order);
        if items.is_empty() {
            return;
        }
        self.menu = Some(MenuState { position, items });
        cx.notify();
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let menu = shell.menu.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let mut panel = div()
        .min_w(px(180.))
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow_md()
        .occlude()
        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
            this.menu = None;
            cx.notify();
        }));
    for (i, item) in menu.items.iter().enumerate() {
        // Separator between order groups (hundreds digit), per the convention
        // on `bibli_core::registry::MenuItem`.
        if i > 0 && menu.items[i - 1].order / 100 != item.order / 100 {
            panel = panel.child(div().my_1().h(px(1.)).bg(theme.border));
        }
        let inv = item.command.clone();
        panel = panel.child(
            div()
                .id(("menu", i))
                .px_3()
                .py_1()
                .cursor_pointer()
                .hover(|s| s.bg(theme.selection_bg).text_color(theme.selection_text))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.menu = None;
                    this.run(&inv.name, inv.args.clone(), window, cx);
                }))
                .child(item.title.clone()),
        );
    }
    Some(
        deferred(
            anchored()
                .position(menu.position)
                .snap_to_window_with_margin(px(8.))
                .child(panel),
        )
        .with_priority(2)
        .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::{SelectionCtx, when_holds};

    fn ctx(count: usize, all_dirs: bool, all_files: bool) -> SelectionCtx {
        SelectionCtx {
            count,
            all_dirs,
            all_files,
        }
    }

    #[test]
    fn when_evaluation() {
        assert!(when_holds(None, ctx(0, true, true)));
        assert!(when_holds(Some("none"), ctx(0, true, true)));
        assert!(!when_holds(Some("selection"), ctx(0, true, true)));
        assert!(when_holds(Some("selection"), ctx(2, false, true)));
        assert!(!when_holds(Some("single"), ctx(2, false, true)));
        assert!(when_holds(Some("single file"), ctx(1, false, true)));
        assert!(!when_holds(Some("single dir"), ctx(1, false, true)));
        assert!(!when_holds(Some("bogus"), ctx(1, true, true)));
    }

    #[test]
    fn flags_follow_selection() {
        assert_eq!(ctx(0, true, true).flags(), ["none"]);
        assert_eq!(ctx(1, true, false).flags(), ["selection", "single", "dir"]);
        assert_eq!(ctx(3, false, true).flags(), ["selection", "file"]);
    }
}
