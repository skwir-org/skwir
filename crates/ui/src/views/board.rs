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
//! cards back. The keys move along a column (Up, Down, the pages, Home,
//! End) and across to the card at the same height (Left, Right), each a
//! move in the pane's order, so Shift extends the selection as anywhere.

use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::{
    AnyElement, Context, ExternalPaths, ScrollHandle, ScrollStrategy, UniformListScrollHandle,
    Window, div, prelude::*, px, uniform_list,
};
use skwir_core::command::Invocation;
use skwir_core::thumbs::ThumbSize;
use skwir_core::{EntryId, Location};

use crate::dnd;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::thumbs::ThumbView;
use crate::view::{Nav, PaneView};

/// The most columns opened: a folder of hundreds of folders is no board.
pub const MAX_COLUMNS: usize = 12;
const COLUMN_W: f32 = 240.0;
const CARD_H: f32 = 48.0;
const PICTURE: f32 = 36.0;

/// One column: what it lists, where it leads, and its cards.
#[derive(Clone)]
struct Column {
    /// Its entry in the place shown; `None` for the place's own files.
    at: Option<EntryId>,
    title: String,
    location: Location,
    /// The folder it is, for a folder's column.
    path: Option<PathBuf>,
    /// Its tag's color, for a tag's.
    color: Option<usize>,
    /// The tags its place says of every card it holds (a tag's own,
    /// `LocationProvider::tags_shown`), which its cards leave out.
    shown: Arc<[String]>,
    ids: Arc<[EntryId]>,
}

#[derive(Default)]
pub struct BoardView {
    /// The listing and the columns asked to open, so each is asked once.
    asked: Option<(u64, Vec<EntryId>)>,
    scrolls: HashMap<Location, UniformListScrollHandle>,
    /// The row of columns, scrolled sideways.
    row: ScrollHandle,
    /// The columns drawn last, by their entry and location, with their
    /// cards: where the keys move.
    drawn: Vec<(Option<EntryId>, Location, Arc<[EntryId]>)>,
    /// The pane's order when last drawn, to find the card at an index.
    order: Arc<[EntryId]>,
    /// Every column shown since the board showed this place: one emptied
    /// is still drawn, empty, to take cards back.
    seen: Vec<(String, Location, Option<PathBuf>, Option<usize>)>,
    /// The columns worked out last, and what they were worked out of (the
    /// listing as it was, the folders opened, the sort, the filter, the
    /// settings for tags' colors): every frame walked the whole place.
    columns_of: Option<(ColumnsOf, Vec<Column>, Vec<EntryId>, u64)>,
}

type ColumnsOf = (u64, u64, u64, skwir_core::SortSpec, u64, u64);

impl BoardView {
    /// The column drawn holding `id`, and its card there; `None` for the
    /// card when `id` is the column's own entry.
    fn find(&self, id: EntryId) -> Option<(usize, Option<usize>)> {
        self.drawn.iter().enumerate().find_map(|(k, (at, _, ids))| {
            match ids.iter().position(|card| *card == id) {
                Some(card) => Some((k, Some(card))),
                None => (*at == Some(id)).then_some((k, None)),
            }
        })
    }

    /// The columns of the place `shell` shows, and those not open yet;
    /// those it showed and that went, empty.
    fn columns(&mut self, shell: &Shell) -> (Vec<Column>, Vec<EntryId>, u64) {
        let of = {
            let ws = shell.app.workspace.read();
            let Some(pane) = ws.pane(shell.pane) else {
                return (Vec::new(), Vec::new(), 0);
            };
            (
                pane.listing.id(),
                pane.listing.version(),
                pane.tree_version(),
                pane.sort,
                pane.filter_version(),
                shell.app.settings.revision(),
            )
        };
        if let Some((at, columns, missing, listing)) = &self.columns_of
            && *at == of
        {
            return (columns.clone(), missing.clone(), *listing);
        }
        let worked = self.work_out_columns(shell);
        self.columns_of = Some((of, worked.0.clone(), worked.1.clone(), worked.2));
        worked
    }

    fn work_out_columns(&mut self, shell: &Shell) -> (Vec<Column>, Vec<EntryId>, u64) {
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
                .value(skwir_core::entry::keys::PLACE_TAG)
                .and_then(skwir_core::Value::as_str)
                .map(|tag| shell.tag_colors.of(tag));
            let opened = pane.expanded().iter().find(|o| o.at == id);
            if opened.is_none() {
                missing.push(id);
            }
            let ids = opened
                .and_then(|o| pane.kept(&o.location))
                .map_or_else(|| Arc::from([]), |listing| listing.sorted(spec));
            let shown = if path.is_none() {
                shell.app.tags_shown(&location).into()
            } else {
                Arc::from([])
            };
            columns.push(Column {
                at: Some(id),
                title: e.name_str().into_owned(),
                location,
                path,
                color,
                shown,
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
                        shown: Arc::from([]),
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
                    shown: Arc::from([]),
                    ids: loose.into(),
                },
            );
        }
        (columns, missing, pane.listing.id())
    }
}

/// What a card shows, read under the workspace lock.
/// A card: what every view draws of its entry, and where it is in the
/// pane's order (`Pane::order`), which the tree and the board share.
struct Card {
    ix: Option<usize>,
    /// Its place among the ids it was read from.
    at: usize,
    look: crate::views::entry::Look,
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
    let shown = Arc::clone(&c.shown);
    let list = uniform_list(
        crate::elements::known_by("board-column", &c.location),
        ids.len(),
        cx.processor(move |this: &mut Shell, range: Range<usize>, _, cx| {
            this.with_pane(pane, |this| {
                let theme = cx.global::<Theme>().clone();
                let mut cards = cards(this, &ids[range.clone()]);
                // The column says its tag: its cards leave it out.
                for card in &mut cards {
                    card.look.tags.retain(|(tag, _)| {
                        !shown.iter().any(|s| skwir_core::metadata::same_tag(s, tag))
                    });
                }
                cards
                    .into_iter()
                    .map(|card| {
                        // A card stands among its column's.
                        let seat = crate::views::entry::Seat {
                            index: range.start + card.at,
                            tree: None,
                        };
                        card_element(this, &card, seat, &theme, cx)
                    })
                    .collect::<Vec<_>>()
            })
        }),
    )
    .track_scroll(&scroll)
    .size_full();
    let count = c.ids.len();
    let tone = c.color.map(|color| theme.tag(color));
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
        .id(crate::elements::known_by("board-col", &c.location))
        .role(gpui::Role::ListBox)
        .when(shell.a11y, |d| {
            d.aria_label(c.title.clone()).aria_size_of_set(c.ids.len())
        })
        .relative()
        .w(px(COLUMN_W))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme.border)
        .child(header)
        .child(
            div().flex_1().min_h_0().relative().child(list).child(
                crate::elements::scrollbar::vertical(
                    ("board-scrollbar", k),
                    crate::elements::scrollbar::Scrolls::Offset(
                        scroll.0.borrow().base_handle.clone(),
                    ),
                )
                // The columns' bar runs along the bottom of them all.
                .across(view.row.clone()),
            ),
        );
    // A card from another column moves here (`drop_on_place` tells the
    // column it leaves), files from elsewhere come in as they come in a
    // folder or a tag.
    col = match c.path {
        Some(dir) => dnd::droppable_dir(col, dir, cx),
        None => dnd::droppable_place(col, c.location, cx),
    };
    col.into_any_element()
}

/// What the cards of `ids` show, read under the lock.
fn cards(shell: &mut Shell, ids: &[EntryId]) -> Vec<Card> {
    // Each card's place in the pane's order, from the list's map: each
    // column walked the whole order for its few cards, every frame.
    let index: HashMap<EntryId, usize> = ids
        .iter()
        .filter_map(|id| Some((*id, shell.ui().list.index_of(*id)?)))
        .collect();
    let app = Arc::clone(&shell.app);
    let ws = app.workspace.read();
    let Some(pane) = ws.pane(shell.pane) else {
        return Vec::new();
    };
    let out: Vec<Card> = ids
        .iter()
        .enumerate()
        .filter_map(|(at, &id)| {
            Some(Card {
                ix: index.get(&id).copied(),
                at,
                look: shell.look(pane, id, PICTURE as u16)?,
            })
        })
        .collect();
    drop(ws);
    // What the cards lack is read in a batch, as the list does.
    shell.request_hydrate(ids.to_vec());
    shell.icons.request(&app);
    out
}

/// A card: its picture (a thumbnail of every kind the grid draws one of,
/// else its icon), its name and its tags, drawn and acting as every view's
/// entries do (`views::entry`).
fn card_element(
    shell: &mut Shell,
    card: &Card,
    seat: crate::views::entry::Seat,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let look = &card.look;
    let thumb = if look.is_dir {
        ThumbView::Unavailable
    } else {
        shell.thumb(look.id, ThumbSize::Normal)
    };
    let picture: AnyElement = match thumb {
        ThumbView::Ready(thumb) => crate::icons::marked(
            div()
                .size(px(PICTURE))
                .flex_none()
                .overflow_hidden()
                .rounded_sm()
                .child(
                    thumb
                        .img(theme)
                        .size_full()
                        .object_fit(gpui::ObjectFit::Cover),
                )
                .into_any_element(),
            PICTURE,
            look.mark(),
            theme,
        ),
        ThumbView::Pending => div()
            .size(px(PICTURE))
            .flex_none()
            .rounded_sm()
            .bg(theme.placeholder)
            .into_any_element(),
        ThumbView::Unavailable => look.picture(PICTURE, theme),
    };
    // The pane's own cards answer a press on a tag; another's, nothing.
    let act = card
        .ix
        .map(|_| Shell::tag_act(shell.pane, Some(look.id), false, cx));
    let el = div()
        // Two names of one file are two cards (ADR-049).
        .id(crate::elements::known_by("card", look.id))
        .relative()
        .h(px(CARD_H))
        .flex_1()
        .min_w_0()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(if look.cursor {
            theme.accent
        } else {
            theme.border
        })
        .bg(if look.selected {
            theme.selection_bg
        } else {
            theme.surface
        })
        .when(look.selected, |d| d.text_color(theme.selection_text))
        .when(!look.selected, |d| d.hover(|s| s.bg(theme.hover)))
        .child(picture)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(crate::views::entry::name(
                    div().id("card-name").text_sm(),
                    look,
                ))
                // A strip of tags takes what its row leaves: under the
                // name, a row of its own.
                .when(!look.tags.is_empty(), |d| {
                    d.child(div().flex().w_full().h(px(crate::tags::CHIP_HEIGHT)).child(
                        crate::views::entry::tags(
                            look,
                            crate::tags::StripOn::Row {
                                selected: look.selected,
                            },
                            theme,
                            act,
                        ),
                    ))
                }),
        );
    let el = if card.ix.is_some() {
        crate::views::entry::behave(el, look, seat, shell, cx)
    } else {
        // Not one of the pane's entries: said by its name and place alone.
        el.role(gpui::Role::ListBoxOption).when(shell.a11y, |d| {
            d.aria_label(look.name.clone())
                .aria_position_in_set(seat.index)
        })
    };
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
        self.order = Arc::clone(&shell.ui().list.sorted);
        for c in &columns {
            self.scrolls.entry(c.location.clone()).or_default();
        }
        let mut row = div()
            .id(("board", shell.pane.0 as usize))
            .size_full()
            .flex()
            .flex_row()
            .overflow_x_scroll()
            .track_scroll(&self.row);
        for (k, c) in columns.into_iter().enumerate() {
            row = row.child(column(shell, self, c, k, &theme, cx));
        }
        // Another application's files dropped beside the columns land in
        // the place shown, as on the list.
        let pane = shell.pane;
        let row = row.on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
            this.activate_pane(pane, cx);
            this.drop_external(paths.paths(), window, cx);
        }));
        div()
            .relative()
            .size_full()
            .child(row)
            .child(crate::elements::scrollbar::horizontal(
                ("board-columns-bar", shell.pane.0 as usize),
                self.row.clone(),
            ))
            .into_any_element()
    }

    /// The card at `ix` of the pane's order, in its column and across.
    fn scroll_to(&mut self, ix: usize) {
        let Some(&id) = self.order.get(ix) else {
            return;
        };
        let Some((k, card)) = self.find(id) else {
            return;
        };
        let (_, location, _) = &self.drawn[k];
        if let (Some(card), Some(scroll)) = (card, self.scrolls.get(location)) {
            scroll.scroll_to_item(card, ScrollStrategy::Top);
        }
        self.row.scroll_to_item(k);
    }

    /// Every key is a move in the pane's order to the card it reaches: up
    /// and down its column, across to the same height in the column
    /// beside (or that column's own entry, when it has no card).
    fn nav_delta(&self, nav: Nav, shell: &Shell) -> Option<isize> {
        let rows = &shell.ui().list;
        let cursor = shell.cursor_id();
        let here = cursor.and_then(|id| rows.index_of(id));
        let (k, card) = cursor.and_then(|id| self.find(id)).unwrap_or((0, None));
        let (at, location, ids) = self.drawn.get(k)?;
        let last = ids.len().checked_sub(1);
        // A page is what the column shows, read from its last layout.
        let page = self.scrolls.get(location).map_or(1, |scroll| {
            let height = f32::from(scroll.0.borrow().base_handle.bounds().size.height);
            ((height / CARD_H) as usize).saturating_sub(1).max(1)
        });
        let along = |by: isize| {
            let to = match card {
                Some(card) => card.saturating_add_signed(by),
                None if by > 0 => 0,
                None => return None,
            };
            last.map(|last| ids[to.min(last)])
        };
        let half = (page / 2).max(1) as isize;
        let target = match nav {
            Nav::Left | Nav::Right => {
                let beside = if nav == Nav::Left {
                    k.checked_sub(1)
                } else {
                    Some(k + 1)
                };
                let (at, _, ids) = self.drawn.get(beside?)?;
                ids.get(card.unwrap_or(0).min(ids.len().saturating_sub(1)))
                    .copied()
                    .or(*at)
            }
            // In a column with no card, its own entry.
            _ => match nav {
                Nav::Up => along(-1),
                Nav::Down => along(1),
                Nav::PageUp => along(-(page as isize)),
                Nav::PageDown => along(page as isize),
                Nav::HalfPageUp => along(-half),
                Nav::HalfPageDown => along(half),
                Nav::Home => ids.first().copied(),
                _ => ids.last().copied(),
            }
            .or(*at),
        }?;
        // From the cursor, or from before the first without one
        // (`Shell::cursor_move`).
        let to = rows.index_of(target)? as isize;
        Some(to - here.map_or(-1, |here| here as isize))
    }

    fn reset(&mut self) {
        self.asked = None;
        self.drawn.clear();
        self.seen.clear();
    }
}
