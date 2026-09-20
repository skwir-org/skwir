//! Context menu from the registry's `context` menu contributions, filtered by
//! `when` tokens evaluated against the selection.

use bibli_core::registry::MenuItem;
use gpui::{
    AnyElement, Context, FocusHandle, MouseButton, Pixels, Point, Window, anchored, deferred, div,
    prelude::*, px,
};

use crate::shell::Shell;
use crate::theme::Theme;

/// What the selection looks like, for `when` evaluation and key contexts.
#[derive(Clone, Copy, Debug, Default)]
pub struct SelectionCtx {
    pub count: usize,
    pub all_dirs: bool,
    pub all_files: bool,
    /// Total size of the selected files whose size is known.
    pub bytes: u64,
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
    /// The menu owns the keyboard while it is open: otherwise keys reach the
    /// pane behind it and act on a selection the user can no longer see.
    pub focus: FocusHandle,
    /// Item under the keyboard cursor; the pointer moves it too.
    pub highlighted: Option<usize>,
}

impl Shell {
    pub fn open_context_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        // A row and its pane both see the right-click, and the pane's handler
        // focuses the pane first: keep the one menu and give it focus back.
        if let Some(menu) = self.menu.as_ref().filter(|menu| menu.position == position) {
            window.focus(&menu.focus);
            window.prevent_default();
            return;
        }
        let focus = cx.focus_handle();
        window.focus(&focus);
        // GPUI focuses the focusable element under a mouse-down after our
        // handler ran (the pane, here) unless the event is marked handled;
        // without this the menu opens unfocused and keys reach the pane.
        window.prevent_default();
        self.menu = Some(MenuState {
            position,
            items,
            focus,
            highlighted: None,
        });
        cx.notify();
    }

    pub fn close_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            window.focus(&self.ui().focus);
            cx.notify();
        }
    }

    /// `ui.menu_*`: keyboard control of the open context menu.
    pub fn menu_command(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        let last = menu.items.len().saturating_sub(1);
        match name {
            "menu_close" => self.close_context_menu(window, cx),
            "menu_down" => {
                menu.highlighted = Some(menu.highlighted.map_or(0, |i| (i + 1).min(last)));
            }
            "menu_up" => {
                menu.highlighted = Some(menu.highlighted.map_or(last, |i| i.saturating_sub(1)));
            }
            "menu_first" => menu.highlighted = Some(0),
            "menu_last" => menu.highlighted = Some(last),
            "menu_activate" => {
                let Some(command) = menu
                    .highlighted
                    .and_then(|i| menu.items.get(i))
                    .map(|item| item.command.clone())
                else {
                    return;
                };
                self.close_context_menu(window, cx);
                self.run(&command.name, command.args, window, cx);
            }
            _ => {}
        }
        cx.notify();
    }
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let menu = shell.menu.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let mut panel = div()
        .min_w(px(220.))
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow_md()
        .occlude()
        .track_focus(&menu.focus)
        .key_context(crate::keymap::MENU)
        .on_mouse_down_out(cx.listener(|this, _, window, cx| {
            this.close_context_menu(window, cx);
        }));
    for (i, item) in menu.items.iter().enumerate() {
        // Separator between order groups (hundreds digit), per the convention
        // on `bibli_core::registry::MenuItem`.
        if i > 0 && menu.items[i - 1].order / 100 != item.order / 100 {
            panel = panel.child(div().my_1().h(px(1.)).bg(theme.border));
        }
        let inv = item.command.clone();
        let highlighted = menu.highlighted == Some(i);
        let keys = shell
            .key_hints
            .iter()
            .find(|hint| *hint.command == *inv.name && hint.args == inv.args)
            .map(|hint| crate::keymap::display_keys(&hint.keys));
        panel = panel.child(
            div()
                .id(("menu", i))
                .mx_1()
                .px_2()
                .py_1()
                .flex()
                .items_center()
                .justify_between()
                .gap_6()
                .rounded_sm()
                .cursor_pointer()
                .when(highlighted, |d| {
                    d.bg(theme.selection_bg).text_color(theme.selection_text)
                })
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if let Some(menu) = this.menu.as_mut()
                        && menu.highlighted != Some(i)
                    {
                        menu.highlighted = Some(i);
                        cx.notify();
                    }
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.close_context_menu(window, cx);
                    this.run(&inv.name, inv.args.clone(), window, cx);
                }))
                .child(item.title.clone())
                .children(keys.map(|keys| {
                    div()
                        .text_xs()
                        .text_color(if highlighted {
                            theme.selection_text
                        } else {
                            theme.text_muted
                        })
                        .child(keys)
                })),
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
            bytes: 0,
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
