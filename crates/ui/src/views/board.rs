//! The Board: the folders of the one shown, or its places (a tag's tags),
//! side by side as columns, each listing what it holds on cards, and a
//! first column for what the place shown holds itself. A category of tags
//! is a board (`Status`: Draft, Review, Final): a card dragged to another
//! column moves there, its tag replaced by that column's (ADR-032), and in
//! a field the tag editor's choice. A folder's folders are one too, a card
//! dragged moving the file. The columns are opened in place (ADR-025:
//! `opens_in_place`), so the cards are the pane's entries, selected and
//! acted on as the list's rows; at most `MAX_COLUMNS` are. A column
//! emptied stays while the board shows the place (a tag no file has is
//! no tag: it would vanish under the card dragged out of it), to take
//! cards back.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use bibli_core::command::Invocation;
use bibli_core::thumbs::ThumbSize;
use bibli_core::workspace::PaneId;
use bibli_core::{EntryId, Location};
use gpui::{
    AnyElement, Context, ExternalPaths, ScrollStrategy, UniformListScrollHandle, Window, div, img,
    prelude::*, px, uniform_list,
};

use crate::dnd::{self, DragPreview};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::thumbs::ThumbView;
use crate::view::{Nav, PaneView};

/// The most columns opened: a folder of hundreds of folders is no board.
pub const MAX_COLUMNS: usize = 12;
const COLUMN_W: f32 = 240.0;
const CARD_H: f32 = 48.0;
const PICTURE: f32 = 36.0;

/// What a card dragged carries: its pane, and the column it leaves, so a
/// drop on another column moves it from that one.
#[derive(Clone, Debug)]
pub struct BoardDrag {
    pub pane: PaneId,
    pub from: Location,
}

/// One column: what it lists, where it leads, and its cards.
struct Column {
    /// Its entry in the place shown; `None` for the place's own files.
    at: Option<EntryId>,
    title: String,
    location: Location,
    /// The folder it is, for a folder's column.
    path: Option<PathBuf>,
    /// Its tag's color, for a tag's.
    color: Option<usize>,
    ids: Arc<[EntryId]>,
}

#[derive(Default)]
pub struct BoardView {
    /// The listing and the columns asked to open, so each is asked once.
    asked: Option<(u64, Vec<EntryId>)>,
    scrolls: HashMap<Location, UniformListScrollHandle>,
    /// The columns drawn last, by their entry and location, with their
    /// cards: where the keys move.
    drawn: Vec<(Option<EntryId>, Location, Arc<[EntryId]>)>,
    /// Every column shown since the board showed this place: one emptied
    /// is still drawn, empty, to take cards back.
    seen: Vec<(String, Location, Option<PathBuf>, Option<usize>)>,
}

impl BoardView {
    /// The columns of the place `shell` shows, and those not open yet;
    /// those it showed and that went, empty.
    fn columns(&mut self, shell: &Shell) -> (Vec<Column>, Vec<EntryId>, u64) {
        let ws = shell.app.workspace.read();
        let Some(pane) = ws.pane(shell.pane) else {
            return (Vec::new(), Vec::new(), 0);
        };
        let spec = pane.sort;
        let top = pane.listing.sorted(spec);
        let shown = pane.location().cloned();
        let mut columns = Vec::new();
        let mut missing = Vec::new();
        let mut loose: Vec<EntryId> = Vec::new();
        for &id in top.iter() {
            let Some(e) = pane.entry(id) else { continue };
            if !e.is_dir() || columns.len() >= MAX_COLUMNS {
                loose.push(id);
                continue;
            }
            let (location, path) = if let Some(place) = pane.location_of(id) {
                (place, None)
            } else if let Some(dir) = pane.path_of(id) {
                (Location::from_path(&dir), Some(dir))
            } else {
                loose.push(id);
                continue;
            };
            let color = e
                .metadata
                .value(bibli_core::entry::keys::PLACE_TAG)
                .and_then(bibli_core::Value::as_str)
                .map(|tag| shell.tag_colors.of(tag));
            let opened = pane.expanded().iter().find(|o| o.at == id);
            if opened.is_none() {
                missing.push(id);
            }
            let ids = opened
                .and_then(|o| pane.kept(&o.location))
                .map_or_else(|| Arc::from([]), |listing| listing.sorted(spec));
            columns.push(Column {
                at: Some(id),
                title: e.name_str().into_owned(),
                location,
                path,
                color,
                ids,
            });
        }
        // Those shown before and gone since (their last card dragged
        // out), empty where they were.
        for c in &columns {
            if !self.seen.iter().any(|(_, l, _, _)| *l == c.location) {
                self.seen
                    .push((c.title.clone(), c.location.clone(), c.path.clone(), c.color));
            }
        }
        for (k, (title, location, path, color)) in self.seen.iter().enumerate() {
            if !columns.iter().any(|c| c.location == *location) {
                columns.insert(
                    k.min(columns.len()),
                    Column {
                        at: None,
                        title: title.clone(),
                        location: location.clone(),
                        path: path.clone(),
                        color: *color,
                        ids: Arc::from([]),
                    },
                );
            }
        }
        if let Some(shown) = shown.filter(|_| !loose.is_empty() || columns.is_empty()) {
            columns.insert(
                0,
                Column {
                    at: None,
                    title: shell.app.location_title(&shown),
                    path: shown.path(),
                    location: shown,
                    color: None,
                    ids: loose.into(),
                },
            );
        }
        (columns, missing, pane.listing.id())
    }
}

/// What a card shows, read under the workspace lock.
struct Card {
    id: EntryId,
    ix: Option<usize>,
    name: String,
    selected: bool,
    cursor: bool,
    icon: Option<Arc<std::path::Path>>,
    picture: Option<(bibli_core::op::EntryRef, Option<std::time::SystemTime>)>,
    tags: Vec<(String, usize)>,
}

fn column(
    shell: &Shell,
    view: &BoardView,
    c: Column,
    k: usize,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let pane = shell.pane;
    let scroll = view.scrolls.get(&c.location).cloned().unwrap_or_default();
    let ids = Arc::clone(&c.ids);
    let from = c.location.clone();
    let list = uniform_list(
        ("board-column", k),
        ids.len(),
        cx.processor(move |this: &mut Shell, range: Range<usize>, window, cx| {
            this.with_pane(pane, |this| {
                let theme = cx.global::<Theme>().clone();
                let cards = cards(this, &ids[range], window, cx);
                cards
                    .into_iter()
                    .map(|card| card_element(this, &card, &from, &theme, window, cx))
                    .collect::<Vec<_>>()
            })
        }),
    )
    .track_scroll(scroll.clone())
    .size_full();
    let count = c.ids.len();
    let tone = c.color.map(|color| theme.tags[color % theme.tags.len()]);
    let header = div()
        .flex_none()
        .h(px(32.))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .border_b_1()
        .border_color(theme.border)
        .children(tone.map(|tone| div().size(px(10.)).rounded_full().bg(tone)))
        .child(div().flex_1().min_w_0().truncate().child(c.title.clone()))
        .child(
            div()
                .text_xs()
                .text_color(theme.text_muted)
                .child(crate::format::thousands(count)),
        );
    let mut col = div()
        .id(("board-col", k))
        .relative()
        .w(px(COLUMN_W))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme.border)
        .child(header)
        .child(div().flex_1().min_h_0().relative().child(list).child(
            crate::elements::scrollbar::vertical(
                ("board-scrollbar", k),
                crate::elements::scrollbar::Scrolls::Offset(scroll.0.borrow().base_handle.clone()),
            ),
        ));
    // A card from another column moves here; files from elsewhere come in
    // as they come in a folder or a tag.
    let (tint, edge) = (theme.drop_target, theme.accent);
    let target = c.location.clone();
    let dir = c.path.clone();
    col = col
        .drag_over::<BoardDrag>(move |s, _, _, _| s.bg(tint).border_color(edge))
        .on_drop(cx.listener(move |this, drag: &BoardDrag, window, cx| {
            cx.stop_propagation();
            if drag.from == target {
                return;
            }
            match &dir {
                Some(dir) => this.drop_into(drag.pane, dir, window, cx),
                None => this.move_on_board(drag, &target, window, cx),
            }
        }));
    col = match c.path {
        Some(dir) => dnd::droppable_dir(col, dir, cx),
        None => dnd::droppable_place(col, c.location, cx),
    };
    col.into_any_element()
}

/// What the cards of `ids` show, read under the lock.
fn cards(
    shell: &mut Shell,
    ids: &[EntryId],
    _window: &mut Window,
    _cx: &mut Context<Shell>,
) -> Vec<Card> {
    let index: HashMap<EntryId, usize> = shell
        .ui()
        .list
        .sorted
        .iter()
        .enumerate()
        .filter(|(_, id)| ids.contains(id))
        .map(|(ix, id)| (*id, ix))
        .collect();
    let app = Arc::clone(&shell.app);
    let ws = app.workspace.read();
    let Some(pane) = ws.pane(shell.pane) else {
        return Vec::new();
    };
    let out = ids
        .iter()
        .filter_map(|&id| {
            let e = pane.entry(id)?;
            let picture = (!e.is_dir())
                .then(|| bibli_platform::mime::of_entry(e))
                .flatten()
                .filter(|m| bibli_platform::mime::is_kind(m, bibli_platform::mime::Kind::Image))
                .and_then(|_| pane.path_of(id))
                .map(|path| {
                    (
                        bibli_core::op::EntryRef::new(id, path),
                        e.metadata
                            .value(bibli_core::entry::keys::MODIFIED)
                            .and_then(bibli_core::Value::as_time),
                    )
                });
            Some(Card {
                id,
                ix: index.get(&id).copied(),
                name: e.name_str().into_owned(),
                selected: pane.selection.contains(id),
                cursor: pane.selection.cursor == Some(id),
                icon: shell.icons.for_entry(e, PICTURE as u16),
                picture,
                tags: crate::tags::entry_tags(e, &shell.tag_colors),
            })
        })
        .collect();
    drop(ws);
    // What the cards lack is read in a batch, as the list does.
    let need: Vec<EntryId> = ids
        .iter()
        .copied()
        .filter(|id| {
            app.workspace
                .read()
                .pane(shell.pane)
                .and_then(|p| p.entry(*id))
                .is_some_and(|e| !e.metadata.is_hydrated())
        })
        .collect();
    shell.request_hydrate(need);
    shell.icons.request(&app);
    out
}

fn card_element(
    shell: &mut Shell,
    card: &Card,
    from: &Location,
    theme: &Theme,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let pane = shell.pane;
    let picture: AnyElement = match &card.picture {
        Some((entry, mtime)) => {
            match shell.thumb_of(entry.clone(), *mtime, ThumbSize::Normal, window, cx) {
                ThumbView::Ready(ri) => div()
                    .size(px(PICTURE))
                    .flex_none()
                    .overflow_hidden()
                    .rounded_sm()
                    .child(img(ri).size_full().object_fit(gpui::ObjectFit::Cover))
                    .into_any_element(),
                _ => div()
                    .size(px(PICTURE))
                    .flex_none()
                    .rounded_sm()
                    .bg(theme.placeholder)
                    .into_any_element(),
            }
        }
        None => match &card.icon {
            Some(icon) => crate::icons::element(icon, PICTURE),
            None => div()
                .size(px(PICTURE))
                .flex_none()
                .rounded_sm()
                .bg(theme.text_muted)
                .into_any_element(),
        },
    };
    let mut el = div()
        .id(("card", card.id.ino as usize ^ card.id.dev as usize))
        .h(px(CARD_H))
        .flex_1()
        .min_w_0()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(if card.cursor {
            theme.accent
        } else {
            theme.border
        })
        .bg(if card.selected {
            theme.selection_bg
        } else {
            theme.surface
        })
        .when(card.selected, |d| d.text_color(theme.selection_text))
        .when(!card.selected, |d| d.hover(|s| s.bg(theme.hover)))
        .child(picture)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().text_sm().child(card.name.clone()))
                .child(
                    div().flex().gap_1().overflow_hidden().children(
                        card.tags
                            .iter()
                            .take(3)
                            .map(|(tag, color)| crate::tags::chip(tag, *color, theme)),
                    ),
                ),
        );
    if let Some(ix) = card.ix {
        el = crate::list::entry_mouse(el, pane, ix, cx);
    }
    // Dragged, it says which column it leaves.
    let drag = BoardDrag {
        pane,
        from: from.clone(),
    };
    let name = card.name.clone();
    let el = el.on_drag(drag, move |_, position, _, cx| {
        cx.new(|_| DragPreview::named(name.clone(), position))
    });
    // A gutter around each card, the card as wide as its column.
    div()
        .w_full()
        .h(px(CARD_H + 4.))
        .px_1()
        .py(px(2.))
        .flex()
        .child(el)
        .into_any_element()
}

impl Shell {
    /// A card dragged from a column of places (a tag's) onto another:
    /// what the target says a drop is (`Verbs::drop`), told the column it
    /// left, so it moves out of it (Ctrl keeps both).
    fn move_on_board(
        &mut self,
        drag: &BoardDrag,
        target: &Location,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drop) = self.app.verbs(target).drop else {
            return;
        };
        let mut args = drop.args.clone();
        if !args.is_object() {
            args = serde_json::json!({});
        }
        args["pane"] = serde_json::json!(drag.pane);
        args["move"] = serde_json::json!(!window.modifiers().control);
        args["from"] = serde_json::json!(drag.from.to_string());
        self.run(&drop.name, args, window, cx);
    }
}

impl PaneView for BoardView {
    fn id(&self) -> &'static str {
        crate::view::BOARD
    }

    fn opens_in_place(&self) -> bool {
        true
    }

    fn render(&mut self, shell: &mut Shell, _: &mut Window, cx: &mut Context<Shell>) -> AnyElement {
        let theme = cx.global::<Theme>().clone();
        let (columns, missing, listing) = self.columns(shell);
        // Each column is opened in place once: its cards are the pane's.
        if !missing.is_empty() && self.asked.as_ref() != Some(&(listing, missing.clone())) {
            self.asked = Some((listing, missing.clone()));
            let pane = shell.pane;
            if let Err(error) = shell.app.dispatch(&Invocation::with_args(
                "expand",
                &serde_json::json!({ "ids": missing, "mode": "expand", "pane": pane }),
            )) {
                tracing::debug!("board: {error:#}");
            }
        }
        self.drawn = columns
            .iter()
            .map(|c| (c.at, c.location.clone(), Arc::clone(&c.ids)))
            .collect();
        for c in &columns {
            self.scrolls.entry(c.location.clone()).or_default();
        }
        let mut row = div()
            .id(("board", shell.pane.0 as usize))
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .overflow_x_scroll();
        for (k, c) in columns.into_iter().enumerate() {
            row = row.child(column(shell, self, c, k, &theme, cx));
        }
        // Another application's files dropped beside the columns land in
        // the place shown, as on the list.
        let pane = shell.pane;
        row.on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
            this.activate_pane(pane, cx);
            this.drop_external(paths.paths(), window, cx);
        }))
        .into_any_element()
    }

    fn scroll_to(&mut self, ix: usize) {
        let _ = ix;
    }

    fn nav_delta(&self, nav: Nav, _shell: &Shell) -> Option<isize> {
        match nav {
            Nav::Up => Some(-1),
            Nav::Down => Some(1),
            _ => None,
        }
    }

    /// Left and Right go to the card at the same height in the column
    /// beside.
    fn nav(
        &mut self,
        nav: Nav,
        shell: &mut Shell,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> bool {
        let step: isize = match nav {
            Nav::Left => -1,
            Nav::Right => 1,
            _ => return false,
        };
        let cursor = shell.cursor_id();
        let here = self
            .drawn
            .iter()
            .position(|(at, _, ids)| cursor.is_some_and(|c| ids.contains(&c) || *at == Some(c)));
        let (k, row) = match here {
            Some(k) => {
                let row = cursor
                    .and_then(|c| self.drawn[k].2.iter().position(|id| *id == c))
                    .unwrap_or(0);
                (k, row)
            }
            None => (0, 0),
        };
        let Some(next) = k.checked_add_signed(step).filter(|n| *n < self.drawn.len()) else {
            return true;
        };
        let (at, location, ids) = &self.drawn[next];
        let target = ids
            .get(row.min(ids.len().saturating_sub(1)))
            .copied()
            .or(*at);
        if let Some(id) = target
            && let Some(ix) = shell.ui().list.index_of(id)
        {
            shell.cursor_to(ix as isize, crate::list::Step::Select, window, cx);
            if let Some(scroll) = self.scrolls.get(location)
                && let Some(pos) = ids.iter().position(|i| *i == id)
            {
                scroll.scroll_to_item(pos, ScrollStrategy::Center);
            }
        }
        true
    }

    fn reset(&mut self) {
        self.asked = None;
        self.drawn.clear();
        self.seen.clear();
    }
}
