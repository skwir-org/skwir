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
        "empty" => ctx.count == 0,
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
    /// Row under the keyboard cursor; the pointer moves it too.
    pub highlighted: Option<usize>,
    /// The submenu whose items are shown, in place, under its parent row.
    pub expanded: Option<String>,
}

/// What the menu shows, top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// Index into `MenuState::items`; `nested` items belong to a submenu.
    Item { index: usize, nested: bool },
    /// The parent row of a submenu.
    Parent(String),
}

impl MenuState {
    pub fn rows(&self) -> Vec<Row> {
        rows_of(&self.items, self.expanded.as_deref())
    }
}

/// Top-level items in order, then each submenu's parent row, followed by its
/// items while it is expanded. Expanding in place keeps one panel and one
/// linear keyboard order; a second popup would have to be a deferred draw
/// inside this one, which GPUI cannot nest.
pub fn rows_of(items: &[MenuItem], expanded: Option<&str>) -> Vec<Row> {
    let mut rows: Vec<Row> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.submenu.is_none())
        .map(|(index, _)| Row::Item {
            index,
            nested: false,
        })
        .collect();
    let mut parents: Vec<&str> = Vec::new();
    for name in items.iter().filter_map(|item| item.submenu.as_deref()) {
        if !parents.contains(&name) {
            parents.push(name);
        }
    }
    for name in parents {
        rows.push(Row::Parent(name.to_owned()));
        if expanded == Some(name) {
            rows.extend(
                items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| item.submenu.as_deref() == Some(name))
                    .map(|(index, _)| Row::Item {
                        index,
                        nested: true,
                    }),
            );
        }
    }
    rows
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
            expanded: None,
        });
        cx.notify();
    }

    /// The list header's own menu: which detail columns it shows. Choosing
    /// columns belongs where they are, not three clicks away in Settings, so
    /// the header answers a right-click with this instead of the dialog.
    pub fn open_column_menu(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut items: Vec<MenuItem> = crate::settings::COLUMN_KEYS
            .iter()
            .enumerate()
            .map(|(i, key)| {
                let shown = self.columns[i];
                MenuItem {
                    menu: "columns".into(),
                    title: self
                        .app
                        .settings
                        .spec(key)
                        .map_or_else(|| (*key).to_owned(), |spec| spec.title.clone()),
                    command: bibli_core::command::Invocation::with_args(
                        "ui.columns",
                        &serde_json::json!({ "key": key }),
                    ),
                    when: None,
                    order: i as i32,
                    icon: shown.then(|| "check".to_owned()),
                    submenu: None,
                }
            })
            .collect();
        items.push(MenuItem {
            menu: "columns".into(),
            title: "Reset widths".into(),
            command: bibli_core::command::Invocation::new("ui.columns_reset"),
            when: None,
            order: 100,
            icon: Some("revert".into()),
            submenu: None,
        });
        let focus = cx.focus_handle();
        window.focus(&focus);
        window.prevent_default();
        self.menu = Some(MenuState {
            position,
            items,
            focus,
            highlighted: None,
            expanded: None,
        });
        cx.notify();
    }

    pub fn close_context_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            window.focus(&self.ui().focus);
            cx.notify();
        }
    }

    /// Run the row, or open and close a submenu's parent.
    fn menu_activate(&mut self, row: &Row, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        match row {
            Row::Parent(name) => {
                menu.expanded = (menu.expanded.as_ref() != Some(name)).then(|| name.clone());
                cx.notify();
            }
            Row::Item { index, .. } => {
                let Some(command) = menu.items.get(*index).map(|item| item.command.clone()) else {
                    return;
                };
                self.close_context_menu(window, cx);
                self.run(&command.name, command.args, window, cx);
            }
        }
    }

    /// `ui.menu_*`: keyboard control of the open context menu.
    pub fn menu_command(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        let rows = menu.rows();
        let last = rows.len().saturating_sub(1);
        let current = menu.highlighted.and_then(|i| rows.get(i).cloned());
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
            "menu_expand" => {
                if let Some(Row::Parent(name)) = current {
                    menu.expanded = Some(name);
                }
            }
            "menu_collapse" => {
                if let Some(name) = menu.expanded.take() {
                    menu.highlighted = menu
                        .rows()
                        .iter()
                        .position(|row| *row == Row::Parent(name.clone()));
                }
            }
            "menu_activate" => {
                if let Some(row) = current {
                    self.menu_activate(&row, window, cx);
                }
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
    let mut previous: Option<&MenuItem> = None;
    for (i, row) in menu.rows().into_iter().enumerate() {
        let highlighted = menu.highlighted == Some(i);
        let muted = if highlighted {
            theme.selection_text
        } else {
            theme.text_muted
        };
        let (title, icon, keys, nested, separated) = match &row {
            Row::Item { index, nested } => {
                let item = &menu.items[*index];
                // Separator between order groups (hundreds digit), per the
                // convention on `bibli_core::registry::MenuItem`.
                let separated = !nested
                    && previous.is_some_and(|before| before.order / 100 != item.order / 100);
                if !nested {
                    previous = Some(item);
                }
                let keys = shell
                    .key_hints
                    .iter()
                    .find(|hint| {
                        *hint.command == *item.command.name && hint.args == item.command.args
                    })
                    .map(|hint| crate::keymap::display_keys(&hint.keys));
                (
                    item.title.clone(),
                    item.icon.clone(),
                    keys,
                    *nested,
                    separated,
                )
            }
            Row::Parent(name) => (name.clone(), None, None, false, true),
        };
        if separated {
            panel = panel.child(div().my_1().h(px(1.)).bg(theme.border));
        }
        let expanded = matches!(&row, Row::Parent(name) if menu.expanded.as_ref() == Some(name));
        let is_parent = matches!(row, Row::Parent(_));
        let icon =
            icon.filter(|name| crate::toolbar::asset(&format!("toolbar/{name}.svg")).is_some());
        panel = panel.child(
            div()
                .id(("menu", i))
                .mx_1()
                .px_2()
                .py_1()
                .when(nested, gpui::Styled::pl_6)
                .flex()
                .items_center()
                .gap_2()
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
                    if let (Row::Item { index, .. }, Some(menu)) = (&row, this.menu.as_ref())
                        && let Some(command) =
                            menu.items.get(*index).map(|item| item.command.clone())
                    {
                        this.note_pointer_use(&command.name, &command.args, cx);
                    }
                    this.menu_activate(&row, window, cx);
                }))
                // An icon column, empty where an item has none, keeps titles aligned.
                .child(div().size(px(15.)).flex_none().children(icon.map(|name| {
                    gpui::svg()
                        .path(format!("toolbar/{name}.svg"))
                        .size(px(15.))
                        .text_color(muted)
                })))
                .child(div().flex_1().child(title))
                .children(keys.map(|keys| div().pl_6().text_xs().text_color(muted).child(keys)))
                .when(is_parent, |d| {
                    d.child(
                        gpui::svg()
                            .path(if expanded {
                                "toolbar/chevron.svg"
                            } else {
                                "toolbar/chevron-right.svg"
                            })
                            .size(px(13.))
                            .text_color(muted),
                    )
                }),
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
    use super::{Row, SelectionCtx, rows_of, when_holds};
    use bibli_core::command::Invocation;
    use bibli_core::registry::MenuItem;

    fn item(title: &str, submenu: Option<&str>) -> MenuItem {
        MenuItem {
            menu: "context".into(),
            title: title.into(),
            command: Invocation::new("noop"),
            when: None,
            order: 0,
            icon: None,
            submenu: submenu.map(str::to_owned),
        }
    }

    #[test]
    fn submenu_items_wait_under_their_parent_until_it_is_expanded() {
        let items = [
            item("Open", None),
            item("Copy Paths", Some("More")),
            item("Rename", None),
            item("Run Command…", Some("More")),
        ];
        let top = |index| Row::Item {
            index,
            nested: false,
        };
        let nested = |index| Row::Item {
            index,
            nested: true,
        };
        assert_eq!(
            rows_of(&items, None),
            [top(0), top(2), Row::Parent("More".into())]
        );
        assert_eq!(
            rows_of(&items, Some("More")),
            [
                top(0),
                top(2),
                Row::Parent("More".into()),
                nested(1),
                nested(3)
            ]
        );
    }

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
        assert!(when_holds(Some("empty"), ctx(0, true, true)));
        assert!(!when_holds(Some("empty"), ctx(1, true, true)));
    }

    #[test]
    fn flags_follow_selection() {
        assert_eq!(ctx(0, true, true).flags(), ["none"]);
        assert_eq!(ctx(1, true, false).flags(), ["selection", "single", "dir"]);
        assert_eq!(ctx(3, false, true).flags(), ["selection", "file"]);
    }
}
