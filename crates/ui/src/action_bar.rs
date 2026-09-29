//! The selection's action bar: while a pane's selection was picked (a
//! Ctrl+click, a range, a rubber band, Select all), a pill floats over the
//! bottom of its list with what the context menu offers for it, laid flat.
//! A selection that follows the cursor (an arrow, a plain click) has none:
//! it changes at every step, and a bar that came and went with it would be
//! noise. `ui.selection_bar` turns it off.
//!
//! The items are the menu's own (`Shell::context_items`: the registered
//! items whose `when` holds, with what the menu sources answer), read when
//! the selection changes, never per frame. The first item of each group
//! (the hundreds of `order`: open, edit, create, destructive, misc) gets a
//! button, then the second of each, as far as `MOST`; they are drawn in the
//! menu's order, and ⋯ opens the whole menu. The pill shows the most that
//! fit the pane (`AdaptiveRow`): with labels, fewer down to three, then
//! icons alone, then fewer.
//!
//! The buttons take no focus: they go with the selection and leave with
//! it, and the keyboard has the same through the Menu key.

use std::rc::Rc;
use std::time::Duration;

use bibli_core::registry::MenuItem;
use gpui::{
    Animation, AnimationExt, AnyElement, Context, MouseButton, MouseDownEvent, SharedString, div,
    prelude::*, px, svg,
};

use crate::elements::adaptive::AdaptiveRow;
use crate::elements::popover::PopoverAnchor;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar;

/// The pill's height.
pub const HEIGHT: f32 = 40.;
/// Between the pill and the bottom of the list.
const MARGIN: f32 = 12.;
/// What a bar takes from the bottom of a pane: a toast rises above it.
pub const CLEARANCE: f32 = HEIGHT + MARGIN;
/// The most buttons beside ⋯.
const MOST: usize = 5;
/// The fewest buttons drawn with their words: with less room, all of them
/// as pictures.
const WORDED: usize = 3;

/// What the items were read for: the selection, the listing and the
/// folders opened in place (what the selection's entries are), and the
/// menus registered.
type Key = (u64, u64, u64, u64, u64);

/// A pane's actions for its selection, read once per change of it.
#[derive(Default)]
pub struct Actions {
    key: Option<Key>,
    items: Rc<[MenuItem]>,
    /// ⋯, which the whole menu stands on.
    more: PopoverAnchor,
    /// Drawn in the last frame: toasts rise above it.
    shown: bool,
}

impl Shell {
    /// The context menu's items for the selection of the pane being
    /// rendered, read again only when the selection or the menus changed.
    fn selection_actions(&mut self) -> Rc<[MenuItem]> {
        let key = {
            let ws = self.app.workspace.read();
            let Some(pane) = ws.pane(self.pane) else {
                return Rc::from([]);
            };
            (
                pane.selection.revision(),
                pane.listing.version(),
                pane.tree_version(),
                self.app.registry.menus.revision(),
                self.app.registry.menu_sources.revision(),
            )
        };
        if self.ui().actions.key == Some(key) {
            return self.ui().actions.items.clone();
        }
        let items: Rc<[MenuItem]> = self.context_items().into();
        let actions = &mut self.ui_mut().actions;
        actions.key = Some(key);
        actions.items = items.clone();
        items
    }
}

/// Which of `items` get a button first, as indices into them, best first:
/// the first of each group, then the second of each, and so on, up to
/// `most`. Only top-level items with an icon qualify: a submenu's
/// are the rarely used ones, and a button needs a picture when it has no
/// room for its words.
fn ranked(items: &[MenuItem], has_icon: impl Fn(&str) -> bool, most: usize) -> Vec<usize> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group = None;
    for (index, item) in items.iter().enumerate() {
        if item.submenu.is_some() || !item.icon.as_deref().is_some_and(&has_icon) {
            continue;
        }
        if group != Some(item.order / 100) {
            group = Some(item.order / 100);
            groups.push(Vec::new());
        }
        if let Some(last) = groups.last_mut() {
            last.push(index);
        }
    }
    let mut chosen = Vec::new();
    let deepest = groups.iter().map(Vec::len).max().unwrap_or(0);
    'tiers: for tier in 0..deepest {
        for group in &groups {
            if let Some(&index) = group.get(tier) {
                if chosen.len() == most {
                    break 'tiers;
                }
                chosen.push(index);
            }
        }
    }
    chosen
}

fn has_icon(name: &str) -> bool {
    toolbar::asset(&format!("toolbar/{name}.svg")).is_some()
}

/// The bar of the pane being rendered, or nothing: the setting is off, the
/// selection follows the cursor, or a rubber band is being drawn there.
pub(crate) fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let bar = bar(shell, cx);
    shell.ui_mut().actions.shown = bar.is_some();
    bar
}

impl Shell {
    /// Whether a pane on screen draws its selection's bar.
    pub(crate) fn action_bar_shown(&self) -> bool {
        self.panes.iter().any(|p| p.actions.shown)
    }
}

fn bar(shell: &mut Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let pane = shell.pane;
    let count = {
        let ws = shell.app.workspace.read();
        let p = ws.pane(pane)?;
        (p.selection.picked() && !p.selection.is_empty()).then(|| p.selection.len())?
    };
    if !shell.prefs.selection_bar || shell.marquee.as_ref().is_some_and(|d| d.pane == pane) {
        return None;
    }
    let items = shell.selection_actions();
    let ranked = ranked(&items, has_icon, MOST);
    // Richest first: every button with its words, fewer down to `WORDED`,
    // then every one as a picture alone, then fewer. More actions are
    // worth more than their words. Measured in prepaint by the row, never
    // here.
    let mut variants = Vec::new();
    for shown in (ranked.len().min(WORDED)..=ranked.len()).rev() {
        variants.push(centred(pill(
            shell,
            &items,
            &top(&ranked, shown),
            true,
            count,
            cx,
        )));
    }
    for shown in (0..=ranked.len()).rev() {
        variants.push(centred(pill(
            shell,
            &items,
            &top(&ranked, shown),
            false,
            count,
            cx,
        )));
    }
    Some(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(MARGIN))
            .px_3()
            .child(AdaptiveRow {
                variants,
                height: HEIGHT,
                chosen: None,
            })
            // The entrance, as a toast's: it ends by itself, and starts
            // again when the bar comes back (its state goes with it).
            .with_animation(
                ("action-bar-in", pane.0 as usize),
                Animation::new(Duration::from_millis(140)).with_easing(gpui::ease_out_quint()),
                |bar, delta| bar.opacity(delta).mb(px(-8. * (1. - delta))),
            )
            .into_any_element(),
    )
}

/// The `shown` best of `ranked`, back in the menu's order.
fn top(ranked: &[usize], shown: usize) -> Vec<usize> {
    let mut best: Vec<usize> = ranked.iter().copied().take(shown).collect();
    best.sort_unstable();
    best
}

fn centred(pill: AnyElement) -> AnyElement {
    div()
        .w_full()
        .h(px(HEIGHT))
        .flex()
        .justify_center()
        .child(pill)
        .into_any_element()
}

/// The pill: how many are picked and the way to let them go, the buttons
/// for `shown`, and ⋯.
fn pill(
    shell: &Shell,
    items: &[MenuItem],
    shown: &[usize],
    labels: bool,
    count: usize,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    let font = shell.font.clone();
    let more = shell.ui().actions.more.clone();
    let tip = |text: String| {
        let font = font.clone();
        let text: SharedString = text.into();
        move |_: &mut gpui::Window, cx: &mut gpui::App| {
            toolbar::tooltip(text.clone(), font.clone(), cx)
        }
    };
    let clear_tip = match shell.keys_for("select_none", &serde_json::json!({})) {
        Some(keys) => format!("Deselect all · {keys}"),
        None => "Deselect all".to_owned(),
    };
    let mut row = div()
        .id(("action-bar", pane.0 as usize))
        .h(px(HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(2.))
        .pl(px(4.))
        .pr(px(4.))
        .rounded_full()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .shadow_md()
        .occlude()
        // The pane's right button opens the folder's menu and deselects:
        // not over the bar.
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .id("action-clear")
                .size(px(28.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .hover(|s| s.bg(theme.hover))
                .tooltip(tip(clear_tip))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.activate_pane(pane, cx);
                    this.run(
                        "select_none",
                        serde_json::json!({ "pane": pane }),
                        window,
                        cx,
                    );
                }))
                .child(crate::controls::glyph("close", theme.text_muted, 12.)),
        )
        .child(
            div()
                .pl(px(2.))
                .pr(px(8.))
                .text_xs()
                .whitespace_nowrap()
                .text_color(theme.text)
                .child(format!("{} selected", crate::format::thousands(count))),
        );
    let mut previous: Option<i32> = None;
    for &index in shown {
        let item = &items[index];
        // A line between the menu's groups, as the menu draws them, and
        // between the count and the first.
        if previous.is_none_or(|order| order / 100 != item.order / 100) {
            row = row.child(toolbar::separator(&theme));
        }
        previous = Some(item.order);
        let title = match shell.keys_for(&item.command.name, &item.command.args) {
            Some(keys) => format!("{} · {keys}", item.title),
            None => item.title.clone(),
        };
        let command = item.command.clone();
        let icon = item.icon.clone().unwrap_or_default();
        row = row.child(
            div()
                .id(("action", index))
                .h(px(28.))
                .min_w(px(28.))
                .px(px(if labels { 8. } else { 6. }))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .rounded_full()
                .cursor_pointer()
                .text_xs()
                .whitespace_nowrap()
                .text_color(theme.text)
                .hover(|s| s.bg(theme.hover))
                .tooltip(tip(title))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.activate_pane(pane, cx);
                    this.note_pointer_use(&command.name, &command.args, cx);
                    this.run(&command.name, command.args.clone(), window, cx);
                }))
                .child(
                    svg()
                        .path(format!("toolbar/{icon}.svg"))
                        .size(px(16.))
                        .flex_none()
                        .text_color(theme.text_muted),
                )
                .when(labels, |d| d.child(item.title.clone())),
        );
    }
    if !shown.is_empty() {
        row = row.child(toolbar::separator(&theme));
    }
    row.child(
        div()
            .id("action-more")
            .size(px(28.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .hover(|s| s.bg(theme.hover))
            .tooltip(tip(
                match shell.keys_for("ui.context_menu", &serde_json::json!({})) {
                    Some(keys) => format!("All actions · {keys}"),
                    None => "All actions".to_owned(),
                },
            ))
            // On the press, like the right button: the menu takes the
            // keyboard there, and a press that closed it leaves it closed.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    this.activate_pane(pane, cx);
                    if this.just_dismissed("context-menu") {
                        return;
                    }
                    let anchor = this.ui().actions.more.clone();
                    this.open_context_menu_over(anchor, window, cx);
                }),
            )
            .child(more.marker())
            .child(
                svg()
                    .path("toolbar/more.svg")
                    .size(px(16.))
                    .text_color(theme.text_muted),
            ),
    )
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use bibli_core::command::Invocation;
    use bibli_core::registry::MenuItem;

    use super::{ranked, top};

    fn item(title: &str, order: i32, icon: Option<&str>, submenu: Option<&str>) -> MenuItem {
        MenuItem {
            menu: "context".into(),
            title: title.into(),
            command: Invocation::new("noop"),
            when: None,
            order,
            icon: icon.map(str::to_owned),
            submenu: submenu.map(str::to_owned),
        }
    }

    #[test]
    fn each_group_s_first_item_comes_before_any_group_s_second() {
        let items = [
            item("Open", 0, Some("open"), None),
            item("Open With…", 1, Some("open"), None),
            item("Cut", 110, Some("cut"), None),
            item("Copy", 120, Some("copy"), None),
            item("Copy Paths", 135, Some("copy"), Some("More")),
            item("Tags…", 150, None, None),
            item("Move to Trash", 300, Some("trash"), None),
            item("Delete Permanently", 310, Some("close"), None),
        ];
        let titles = |most| -> Vec<&str> {
            top(&ranked(&items, |_| true, most), most)
                .into_iter()
                .map(|i| items[i].title.as_str())
                .collect()
        };
        assert_eq!(titles(3), ["Open", "Cut", "Move to Trash"]);
        assert_eq!(
            titles(5),
            ["Open", "Open With…", "Cut", "Copy", "Move to Trash"],
            "in the menu's order, whatever their rank"
        );
        assert_eq!(
            titles(10),
            [
                "Open",
                "Open With…",
                "Cut",
                "Copy",
                "Move to Trash",
                "Delete Permanently"
            ],
            "a submenu's item and one without a picture have no button"
        );
        assert_eq!(ranked(&items, |name| name != "open", 1), [2]);
        assert_eq!(
            top(&ranked(&items, |_| true, 10), 2),
            [0, 2],
            "fewer shown: the best of them"
        );
    }
}
