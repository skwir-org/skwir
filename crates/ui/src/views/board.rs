//! The Board: the folders of the one shown, or its places (a tag's tags),
//! side by side as columns, each listing what it holds on cards, and a
//! first column for what the place shown holds itself. A category of tags
//! is a board (`Status`: Draft, Review, Final): a card dragged to another
//! column moves there, its tag replaced by that column's (ADR-032), and in
//! a field the tag editor's choice. A folder's folders are one too, a card
//! dragged moving the file. The columns are opened in place (ADR-025:
//! `opens_in_place`), so the cards are the pane's entries, selected and
//! acted on as the list's rows; at most `MAX_COLUMNS` are. Grouped, a
//! column is a group, as many of them, the last holding every group left
//! (ADR-131, 3). A column
//! emptied stays while the board shows the place (a tag no file has is
//! no tag: it would vanish under the card dragged out of it), to take
//! cards back. The keys move along a column (Up, Down, the pages, Home,
//! End) and across to the card at the same height (Left, Right), each a
//! move in the pane's order, so Shift extends the selection as anywhere.

use crate::sizes::TextSizes;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
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
use crate::theme::Chosen;
use crate::theme::Theme;
use crate::thumbs::ThumbView;
use crate::view::{Nav, PaneView};

/// The most columns opened: a folder of hundreds of folders is no board.
/// Grouped, the most columns drawn: the last holds the groups left.
pub const MAX_COLUMNS: usize = 12;
/// A column's width and a card's height at 100 %, drawn at the files'
/// scale (ADR-117); a card is never shorter than its picture needs, which
/// keeps its size.
const COLUMN_W: f32 = 240.0;
const CARD_H: f32 = 48.0;
const PICTURE: f32 = 36.0;

/// A card's height at `scale`.
fn card_height(scale: crate::sizes::Scale) -> f32 {
    scale.f(CARD_H).max(CARD_H)
}

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
    /// A group's (ADR-131, 3): how many it holds, folded or not; it takes
    /// no card dragged in, a group being what a file is, not where.
    group: Option<usize>,
    /// The pane's groups it holds: one, or every group left past the
    /// columns a board draws (`MAX_COLUMNS`); none for a folder's.
    groups: Range<usize>,
}

#[derive(Default)]
pub struct BoardView {
    /// The columns asked to open, and what they were missing from, so each
    /// is asked once while nothing closes them. Leaving for a view that
    /// does not open folders in place closes them all (`Shell::sync`), as
    /// the tree closes some: coming back, the board asks again, or its
    /// columns stayed empty.
    asked: Option<(Against, Vec<EntryId>)>,
    /// The card at an index of the pane's order to bring into view, and
    /// how: found once the columns are drawn as they are now. Asked before
    /// a draw (the view chosen with a cursor), the columns and the order
    /// were those drawn last, a visit before, or none.
    wanted: Option<(usize, ScrollStrategy)>,
    scrolls: HashMap<Location, UniformListScrollHandle>,
    /// The row of columns, scrolled sideways.
    row: ScrollHandle,
    /// The columns drawn last, by their entry and location, with their
    /// cards: where the keys move.
    drawn: Vec<Drawn>,
    /// Where each of `drawn`'s entries is (`index_of_columns`), made the
    /// first time a key asks after the columns changed: a key walked every
    /// card of every column to find the cursor's.
    drawn_index: std::cell::OnceCell<HashMap<EntryId, (usize, Option<usize>)>>,
    /// The pane's order when last drawn, to find the card at an index.
    order: Arc<[EntryId]>,
    /// Every column shown since the board showed this place: one emptied
    /// is still drawn, empty, to take cards back.
    seen: Vec<(String, Location, Option<PathBuf>, Option<usize>)>,
    /// The columns worked out last, and what they were worked out of (the
    /// listing as it was, the folders opened, the sort, the filter, the
    /// settings for tags' colors): every frame walked the whole place.
    columns_of: Option<(ColumnsOf, Vec<Column>, Vec<EntryId>, Against)>,
    /// The files' scale when last drawn, for a page of cards.
    scale: crate::sizes::Scale,
    /// Grouped, the groups' columns and the order and groups they were
    /// made of (ADR-131, 3).
    groups_of: Option<(Arc<[EntryId]>, (usize, usize, usize), Vec<Column>)>,
    /// The listing whose folders opened in place a grouped board closed:
    /// once, the folders' files being no group's.
    closed_for: Option<u64>,
    /// Each column's rows as last drawn, by its location: its cards, and
    /// what arrives among them (ADR-135), which is no card.
    arriving: HashMap<Location, (crate::views::groups::RowMap, Vec<usize>)>,
    /// What `arriving` was placed for: the pane's rows (`RowMap::key`,
    /// which a new placement of what arrives makes anew) with the columns
    /// drawn (`drawn`); placed again only when one of them changes.
    arriving_for: Option<(usize, usize, usize)>,
}

type ColumnsOf = (u64, u64, u64, skwir_core::SortSpec, u64, u64);

/// What the columns not open yet were missing from: the listing
/// (`Listing::id`) and the folders opened in place
/// (`Pane::expanded_version`).
type Against = (u64, u64);

/// A column drawn: its entry, where it leads, its cards.
type Drawn = (Option<EntryId>, Location, Arc<[EntryId]>);

/// Where each entry of `drawn` is: its column and its card there, or `None`
/// for a column's own entry; the first column holding it, and in a column
/// its first card before its own entry, as walking them in order finds it.
fn index_of_columns(drawn: &[Drawn]) -> HashMap<EntryId, (usize, Option<usize>)> {
    let mut index = HashMap::with_capacity(drawn.iter().map(|(_, _, ids)| ids.len() + 1).sum());
    for (k, (at, _, ids)) in drawn.iter().enumerate() {
        for (card, id) in ids.iter().enumerate() {
            index.entry(*id).or_insert((k, Some(card)));
        }
        if let Some(at) = at {
            index.entry(*at).or_insert((k, None));
        }
    }
    index
}

impl BoardView {
    /// The column drawn holding `id`, and its card there; `None` for the
    /// card when `id` is the column's own entry.
    fn find(&self, id: EntryId) -> Option<(usize, Option<usize>)> {
        self.drawn_index
            .get_or_init(|| index_of_columns(&self.drawn))
            .get(&id)
            .copied()
    }

    /// The card at `ix` of the pane's order brought into view in its
    /// column, as `strategy` says, and its column across.
    fn scroll_card(&mut self, ix: usize, strategy: ScrollStrategy) {
        let Some(&id) = self.order.get(ix) else {
            return;
        };
        let Some((k, card)) = self.find(id) else {
            return;
        };
        let (_, location, _) = &self.drawn[k];
        if let (Some(card), Some(scroll)) = (card, self.scrolls.get(location)) {
            // Among its column's rows, what arrives among them.
            let row = self
                .arriving
                .get(location)
                .map_or(card, |(rows, _)| rows.row_of(card));
            scroll.scroll_to_item(row, strategy);
        }
        self.row.scroll_to_item(k);
    }

    /// Grouped, a column a group (ADR-131, 3), in the groups' order, its
    /// cards its entries in the sort's; a folded group's empty, its count
    /// said. As many columns as a folder's folders get (`MAX_COLUMNS`):
    /// past them, the last holds the cards of every group left, in order,
    /// saying how many groups, so none goes unseen and a search grouped by
    /// Folder lays out a dozen columns, never thousands. Made again only
    /// when the order or its groups change.
    fn group_columns(&mut self, shell: &Shell) -> Vec<Column> {
        let list = &shell.ui().list;
        let key = list.rows.key();
        if let Some((order, at, columns)) = &self.groups_of
            && Arc::ptr_eq(order, &list.sorted)
            && *at == key
        {
            return columns.clone();
        }
        let columns = group_columns(&list.rows, &list.sorted);
        self.groups_of = Some((Arc::clone(&list.sorted), key, columns.clone()));
        columns
    }

    /// The columns of the place `shell` shows, and those not open yet;
    /// those it showed and that went, empty.
    fn columns(&mut self, shell: &Shell) -> (Vec<Column>, Vec<EntryId>, Against) {
        // The folders opened or closed move `tree_version`: the columns
        // worked out keep what they were missing from.
        let of = {
            let ws = shell.app.workspace.read();
            let Some(pane) = ws.pane(shell.pane) else {
                return (Vec::new(), Vec::new(), (0, 0));
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
        if let Some((at, columns, missing, against)) = &self.columns_of
            && *at == of
        {
            return (columns.clone(), missing.clone(), *against);
        }
        let worked = self.work_out_columns(shell);
        self.columns_of = Some((of, worked.0.clone(), worked.1.clone(), worked.2));
        worked
    }

    fn work_out_columns(&mut self, shell: &Shell) -> (Vec<Column>, Vec<EntryId>, Against) {
        let ws = shell.app.workspace.read();
        let Some(pane) = ws.pane(shell.pane) else {
            return (Vec::new(), Vec::new(), (0, 0));
        };
        let spec = pane.sorting();
        let top = pane.listing.order_shown(&spec);
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
                .map_or_else(|| Arc::from([]), |listing| listing.order_shown(&spec));
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
                group: None,
                groups: 0..0,
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
                        group: None,
                        groups: 0..0,
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
                    group: None,
                    groups: 0..0,
                },
            );
        }
        (
            columns,
            missing,
            (pane.listing.id(), pane.expanded_version()),
        )
    }
}

/// The columns of a board grouped as `rows` says, `sorted` the order
/// (`BoardView::group_columns`): one a group, `MAX_COLUMNS` at most, the
/// last then every group left.
fn group_columns(rows: &crate::views::groups::RowMap, sorted: &[EntryId]) -> Vec<Column> {
    let spans = rows.spans();
    let cards = |from: usize, to: usize| -> Arc<[EntryId]> {
        sorted
            .get(from..to)
            .map_or_else(|| Arc::from([]), Arc::from)
    };
    let column = |title: String, location, ids, count, groups| Column {
        at: None,
        location,
        title,
        path: None,
        color: None,
        shown: Arc::from([]),
        ids,
        group: Some(count),
        groups,
    };
    let own = if spans.len() > MAX_COLUMNS {
        MAX_COLUMNS - 1
    } else {
        spans.len()
    };
    let mut columns: Vec<Column> = spans[..own]
        .iter()
        .enumerate()
        .map(|(n, span)| {
            let title = crate::views::groups::label(span).to_string();
            column(
                title.clone(),
                Location::new("board-group", title),
                cards(span.start, span.start + span.len),
                span.count,
                n..n + 1,
            )
        })
        .collect();
    if let (Some(first), Some(last)) = (spans.get(own), spans.last()) {
        columns.push(column(
            crate::format::count(spans.len() - own, "more group"),
            Location::new("board-groups", "more"),
            cards(first.start, last.start + last.len),
            spans[own..].iter().map(|span| span.count).sum(),
            own..spans.len(),
        ));
    }
    columns
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
    let scale = crate::views::list::files_scale(theme);
    let scroll = view.scrolls.get(&c.location).cloned().unwrap_or_default();
    let ids = Arc::clone(&c.ids);
    let shown = Arc::clone(&c.shown);
    // Its cards, and what arrives among them (ADR-135).
    let (rows, arriving) = view
        .arriving
        .get(&c.location)
        .cloned()
        .unwrap_or_else(|| (crate::views::groups::RowMap::flat(ids.len()), Vec::new()));
    let count = rows.len();
    let list = uniform_list(
        crate::elements::known_by("board-column", &c.location),
        count,
        cx.processor(move |this: &mut Shell, range: Range<usize>, _, cx| {
            this.with_pane(pane, |this| {
                let theme = cx.global::<Theme>().clone();
                let held = rows.entries_in(range.clone());
                let mut cards = cards(this, &ids[held.clone()]);
                // The column says its tag: its cards leave it out.
                for card in &mut cards {
                    card.look.tags.retain(|(tag, _)| {
                        !shown.iter().any(|s| skwir_core::metadata::same_tag(s, tag))
                    });
                }
                let mut cards = cards.into_iter().peekable();
                range
                    .map(|at| match rows.at(at) {
                        Some(crate::views::groups::Row::Arriving(j)) => {
                            arriving_card(this, arriving.get(j).copied(), &theme, cx)
                        }
                        Some(crate::views::groups::Row::Entry(card_at)) => {
                            match cards.next_if(|card| held.start + card.at == card_at) {
                                // A card stands among its column's.
                                Some(card) => {
                                    let seat = crate::views::entry::Seat {
                                        index: card_at,
                                        tree: None,
                                        group: None,
                                    };
                                    card_element(this, &card, seat, &theme, cx)
                                }
                                None => div().into_any_element(),
                            }
                        }
                        _ => div().into_any_element(),
                    })
                    .collect::<Vec<_>>()
            })
        }),
    )
    .track_scroll(&scroll)
    .size_full();
    let count = c.group.unwrap_or(c.ids.len());
    let tone = c.color.map(|color| theme.tag(color));
    let header = div()
        .flex_none()
        .h(scale.px(32.))
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
                .text_small()
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
        .w(scale.px(COLUMN_W))
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
    col = match (c.group, c.path) {
        (Some(_), _) => col,
        (None, Some(dir)) => dnd::droppable_dir(col, dir, cx),
        (None, None) => dnd::droppable_place(col, c.location, cx),
    };
    col.into_any_element()
}

/// What arrives in column `c` (the `k`th), among its cards (ADR-135): in
/// its group where the board groups, else in the folder it is. Its rows (a
/// card a row, what arrives among them, as `ListState::rows` has it), and
/// which of `ListState::arriving` each that arrives is.
fn arriving_columns(
    shell: &Shell,
    columns: &[Column],
    grouped: bool,
) -> HashMap<Location, (crate::views::groups::RowMap, Vec<usize>)> {
    let list = &shell.ui().list;
    let mut mine: Vec<Vec<usize>> = vec![Vec::new(); columns.len()];
    if grouped {
        // The column holding each of the pane's groups.
        let mut column_of = vec![None; list.rows.spans().len()];
        for (k, c) in columns.iter().enumerate() {
            for n in c.groups.clone() {
                if let Some(slot) = column_of.get_mut(n) {
                    *slot = Some(k);
                }
            }
        }
        for (i, placed) in list.arriving_placed.iter().enumerate() {
            if let Some(k) = placed
                .group
                .and_then(|n| column_of.get(n).copied().flatten())
            {
                mine[k].push(i);
            }
        }
    } else {
        let column_of: HashMap<&Path, usize> = columns
            .iter()
            .enumerate()
            .filter_map(|(k, c)| Some((c.path.as_deref()?, k)))
            .collect();
        for (i, arriving) in list.arriving.iter().enumerate() {
            if let Some(&k) = column_of.get(arriving.arrival.folder.as_path()) {
                mine[k].push(i);
            }
        }
    }
    columns
        .iter()
        .zip(mine)
        .map(|(c, mine)| (c.location.clone(), arriving_rows(list, c, mine)))
        .collect()
}

/// Column `c`'s rows: its cards, and `mine` of what arrives among them.
fn arriving_rows(
    list: &crate::list::ListState,
    c: &Column,
    mine: Vec<usize>,
) -> (crate::views::groups::RowMap, Vec<usize>) {
    if mine.is_empty() {
        return (crate::views::groups::RowMap::flat(c.ids.len()), mine);
    }
    // Before the first card that comes after it in the pane's order.
    let placed: Vec<crate::views::groups::Placed> = mine
        .iter()
        .map(|&i| {
            let at = list.arriving_placed.get(i).map_or(usize::MAX, |p| p.at);
            let slot = c
                .ids
                .partition_point(|id| list.index_of(*id).is_some_and(|ix| ix < at));
            crate::views::groups::Placed {
                at: slot,
                ..Default::default()
            }
        })
        .collect();
    (
        crate::views::groups::RowMap::arriving(Arc::from([]), c.ids.len(), &placed, &[]),
        mine,
    )
}

/// What arrives as a card (ADR-135): its picture and its name, dimmed,
/// the tint behind it, its pill; no card the keys reach nor one selected,
/// its press its action's card in Activity.
fn arriving_card(
    shell: &mut Shell,
    i: Option<usize>,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let Some(arriving) = i.and_then(|i| shell.ui().list.arriving.get(i).cloned()) else {
        return div().into_any_element();
    };
    let i = i.unwrap_or_default();
    let look = shell.arriving_look(&arriving, PICTURE as u16);
    let scale = crate::views::list::files_scale(theme);
    let height = card_height(scale);
    let dim = crate::views::arriving::OPACITY;
    let el = div()
        .id(("arriving-card", i))
        .relative()
        .h(px(height))
        .flex_1()
        .min_w_0()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded_md()
        .overflow_hidden()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(crate::views::arriving::tint(&arriving, theme))
        .child(
            div()
                .flex_none()
                .opacity(dim)
                .child(look.picture(PICTURE, theme)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap_1()
                .child(crate::views::entry::name(
                    div().id("arriving-card-name").text_body().opacity(dim),
                    &look,
                ))
                .child(crate::views::arriving::pill(&arriving, theme)),
        );
    let el = crate::views::arriving::acting(el, &arriving, shell, cx);
    let el = crate::views::arriving::expose(el, &look.name, &arriving, shell.a11y);
    div()
        .w_full()
        .h(px(height + 4.))
        .px_1()
        .py(px(2.))
        .flex()
        .child(el)
        .into_any_element()
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
    let scale = crate::views::list::files_scale(theme);
    let height = card_height(scale);
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
        .h(px(height))
        .flex_1()
        .min_w_0()
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(match (look.cursor, look.ring) {
            (true, _) if look.selected => theme.selection_text,
            (true, _) => theme.accent,
            (false, Some(ring)) => theme.tag(ring),
            (false, None) => theme.border,
        })
        .map(|d| match (look.selected, look.ring) {
            (true, _) => d.chosen(theme),
            (false, Some(ring)) => d.bg(theme.tag_fill(ring)),
            (false, None) => d.bg(theme.surface),
        })
        .when(!look.selected, |d| d.hover(|s| s.bg(theme.hover)))
        .child(picture)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(crate::views::entry::name(
                    div().id("card-name").text_body(),
                    look,
                ))
                // A strip of tags takes what its row leaves: under the
                // name, a row of its own.
                .when(!look.tags.is_empty(), |d| {
                    d.child(
                        div()
                            .flex()
                            .w_full()
                            .h(scale.px(crate::tags::CHIP_HEIGHT))
                            .child(crate::views::entry::tags(
                                look,
                                crate::tags::StripOn::Row {
                                    selected: look.selected,
                                },
                                theme,
                                act,
                            )),
                    )
                }),
        );
    let el = if card.ix.is_some() {
        crate::views::entry::behave(el, look, seat, shell, cx)
    } else {
        // Not one of the pane's entries: said by its name, its tags and
        // stars, and its place.
        el.role(gpui::Role::ListBoxOption).when(shell.a11y, |d| {
            d.aria_label(look.name.clone())
                .when_some(crate::tags::said(&look.tags, look.rating), |d, said| {
                    d.aria_description(said)
                })
                .aria_position_in_set(seat.index)
        })
    };
    // A gutter around each card, the card as wide as its column.
    div()
        .w_full()
        .h(px(height + 4.))
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
        self.scale = crate::views::list::files_scale(&theme);
        let grouped = shell.ui().list.rows.grouped();
        let (columns, missing, against) = if grouped {
            // The folders this board opened in place are closed, once: their
            // files would be in their folder's group.
            let pane = shell.pane;
            let listing = shell
                .app
                .workspace
                .read()
                .pane(pane)
                .and_then(|p| (!p.expanded().is_empty()).then(|| p.listing.id()));
            if let Some(listing) = listing
                && self.closed_for != Some(listing)
            {
                self.closed_for = Some(listing);
                self.asked = None;
                if let Err(error) = shell.app.dispatch(&Invocation::with_args(
                    "expand",
                    &serde_json::json!({ "mode": "collapse_all", "pane": pane }),
                )) {
                    tracing::debug!("board: {error:#}");
                }
            }
            (self.group_columns(shell), Vec::new(), (0, 0))
        } else {
            self.closed_for = None;
            self.columns(shell)
        };
        // Each column is opened in place once while it stays open: its
        // cards are the pane's.
        if !missing.is_empty() && self.asked.as_ref() != Some(&(against, missing.clone())) {
            self.asked = Some((against, missing.clone()));
            let pane = shell.pane;
            if let Err(error) = shell.app.dispatch(&Invocation::with_args(
                "expand",
                &serde_json::json!({ "ids": missing, "mode": "expand", "pane": pane }),
            )) {
                tracing::debug!("board: {error:#}");
            }
        }
        let drawn: Vec<Drawn> = columns
            .iter()
            .map(|c| (c.at, c.location.clone(), Arc::clone(&c.ids)))
            .collect();
        // The same columns frame after frame (`columns_of`) keep their index.
        let same = drawn.len() == self.drawn.len()
            && drawn
                .iter()
                .zip(&self.drawn)
                .all(|(a, b)| a.0 == b.0 && a.1 == b.1 && Arc::ptr_eq(&a.2, &b.2));
        if !same {
            self.drawn = drawn;
            self.drawn_index = std::cell::OnceCell::new();
            // The scrolls of columns gone go with them.
            let shown: HashSet<&Location> = columns.iter().map(|c| &c.location).collect();
            self.scrolls.retain(|location, _| shown.contains(location));
        }
        self.order = Arc::clone(&shell.ui().list.sorted);
        for c in &columns {
            self.scrolls.entry(c.location.clone()).or_default();
        }
        // What arrives in each column, among its cards (ADR-135): placed
        // once per placement of what arrives and per columns drawn.
        let placed = shell.ui().list.rows.key();
        if !same || self.arriving_for != Some(placed) {
            self.arriving = arriving_columns(shell, &columns, grouped);
            self.arriving_for = Some(placed);
        }
        if let Some((ix, strategy)) = self.wanted.take() {
            self.scroll_card(ix, strategy);
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

    /// The card at `ix` of the pane's order, in its column and across,
    /// once drawn (`wanted`), as the list's handle scrolls at its layout.
    fn scroll_to(&mut self, ix: usize) {
        self.wanted = Some((ix, ScrollStrategy::Nearest));
    }

    fn scroll_to_middle(&mut self, ix: usize) {
        self.wanted = Some((ix, ScrollStrategy::Center));
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
            ((height / (card_height(self.scale) + 4.)) as usize)
                .saturating_sub(1)
                .max(1)
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
            // Past a column with neither a card nor an entry of its own
            // (a folded group's).
            Nav::Left | Nav::Right => {
                let mut beside = k;
                loop {
                    beside = if nav == Nav::Left {
                        beside.checked_sub(1)?
                    } else {
                        beside + 1
                    };
                    let (at, _, ids) = self.drawn.get(beside)?;
                    let reached = ids
                        .get(card.unwrap_or(0).min(ids.len().saturating_sub(1)))
                        .copied()
                        .or(*at);
                    if reached.is_some() {
                        break reached;
                    }
                }
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
        self.wanted = None;
        self.drawn.clear();
        self.drawn_index = std::cell::OnceCell::new();
        self.seen.clear();
        self.arriving_for = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skwir_core::FileId;

    /// The index finds what walking the columns found: the first column
    /// holding an entry, its first card there before its own entry.
    #[test]
    fn a_card_is_found_where_walking_the_columns_finds_it() {
        let id = |ino| EntryId {
            file: FileId::new(1, ino),
            at: ino,
        };
        let column = |name: &str| Location::from_path(std::path::Path::new("/data").join(name));
        let drawn: Vec<Drawn> = vec![
            (None, column("here"), Arc::from([id(1), id(2)])),
            (
                Some(id(2)),
                column("photos"),
                Arc::from([id(3), id(4), id(3)]),
            ),
            (Some(id(5)), column("empty"), Arc::from([])),
            (Some(id(6)), column("again"), Arc::from([id(4), id(6)])),
        ];
        let walked = |want: EntryId| {
            drawn.iter().enumerate().find_map(|(k, (at, _, ids))| {
                match ids.iter().position(|card| *card == want) {
                    Some(card) => Some((k, Some(card))),
                    None => (*at == Some(want)).then_some((k, None)),
                }
            })
        };
        let index = index_of_columns(&drawn);
        for ino in 0..8 {
            assert_eq!(index.get(&id(ino)).copied(), walked(id(ino)), "entry {ino}");
        }
        assert_eq!(
            index[&id(2)],
            (0, Some(1)),
            "a card before a column's own entry"
        );
        assert_eq!(index[&id(3)], (1, Some(0)), "its first card");
        assert_eq!(index[&id(5)], (2, None), "an empty column's own entry");
        assert_eq!(index[&id(6)], (3, Some(1)), "a card in its own column");
    }

    /// Grouped, a column a group, `MAX_COLUMNS` at most: past them, the
    /// last holds every group left, in order, saying how many, so no card
    /// goes unseen; a folded group's column is empty, its count said.
    #[test]
    fn past_the_columns_drawn_the_last_holds_every_group_left() {
        use skwir_core::grouping::Span;
        let id = |ino| EntryId {
            file: FileId::new(1, ino),
            at: ino,
        };
        let span = |start, len, count| Span {
            start,
            len,
            count,
            group: None,
        };
        // Twenty groups of one, the second folded (two files).
        let mut spans = vec![span(0, 1, 1), span(1, 0, 2)];
        spans.extend((1..19).map(|n| span(n, 1, 1)));
        let sorted: Vec<EntryId> = (0..19).map(id).collect();
        let rows = crate::views::groups::RowMap::new(Arc::from(spans), sorted.len());
        assert!(rows.grouped());
        let columns = group_columns(&rows, &sorted);
        assert_eq!(columns.len(), MAX_COLUMNS);
        assert_eq!(columns[1].ids.len(), 0, "folded");
        assert_eq!(columns[1].group, Some(2), "its count said");
        let last = &columns[MAX_COLUMNS - 1];
        assert_eq!(last.title, "9 more groups");
        assert_eq!(last.groups, MAX_COLUMNS - 1..20);
        assert_eq!(&*last.ids, &sorted[10..]);
        let cards: usize = columns.iter().map(|c| c.ids.len()).sum();
        assert_eq!(cards, sorted.len(), "every card in a column");
        // As many groups as columns: one each.
        let few = crate::views::groups::RowMap::new(
            Arc::from((0..MAX_COLUMNS).map(|n| span(n, 1, 1)).collect::<Vec<_>>()),
            MAX_COLUMNS,
        );
        let columns = group_columns(&few, &sorted[..MAX_COLUMNS]);
        assert_eq!(columns.len(), MAX_COLUMNS);
        assert!(columns.iter().all(|c| c.groups.len() == 1));
    }
}
