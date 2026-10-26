//! What every view draws of an entry and what it does with it, one way.
//! `Look` is read under the lock once per entry drawn (`Shell::look`);
//! `highlight` draws a row's or a tile's selection, cursor and hover; and
//! `behave` gives the element what every entry answers: dimmed while it
//! waits to be pasted elsewhere, the menu's marker on the cursor, pressed
//! and clicked as the pane's entry, dragged as the selection, taking what
//! a dragged place gives (a tag), and taking files dropped where it leads,
//! and what assistive technology hears of it: its name, whether it is
//! selected, where it stands among the entries (`Seat`), and, on the
//! cursor, that it is where the keys are. A view draws its own shape
//! around these (a row, a tile, a card), never its own copy of them.

use std::path::PathBuf;

use gpui::{AnyElement, Context, Div, Stateful, prelude::*};
use skwir_core::workspace::Pane;
use skwir_core::{EntryId, Location};

use crate::dnd;
use crate::elements::chips::ChipStrip;
use crate::icons;
use crate::shell::Shell;
use crate::theme::Theme;

/// What every view knows of an entry it draws.
#[allow(clippy::struct_excessive_bools)]
pub struct Look {
    pub id: EntryId,
    pub name: String,
    pub is_dir: bool,
    pub selected: bool,
    pub cursor: bool,
    /// Cut, waiting for a paste: drawn dimmed, as the file is on its way out.
    pub cut: bool,
    /// Its icon, the theme's or the system's, at the size asked.
    pub icon: Option<icons::Pic>,
    pub link: icons::Link,
    /// Each tag with its color (`tags::TagColors`).
    pub tags: Vec<(String, usize)>,
    /// A place (a tag's), or a folder that does not open at its path (a
    /// trashed one): where it leads, and a tag's color.
    pub place: Option<(Location, Option<usize>)>,
    /// A folder's own path: where files dropped on it go.
    pub path: Option<PathBuf>,
}

impl Look {
    /// What its picture is marked with.
    pub fn mark(&self) -> icons::Mark {
        icons::Mark::of(self.link, self.place.as_ref().and_then(|(_, tag)| *tag))
    }

    /// Its picture when it has no thumbnail, `size` px (`icons::entry_picture`).
    pub fn picture(&self, size: f32, theme: &Theme) -> AnyElement {
        icons::entry_picture(self.icon.as_ref(), self.is_dir, size, self.mark(), theme)
    }
}

impl Shell {
    /// Entry `id` of `pane` as every view draws it, its icon at `icon_px`.
    /// Under the caller's lock: `pane` is read from it.
    pub fn look(&mut self, pane: &Pane, id: EntryId, icon_px: u16) -> Option<Look> {
        let e = pane.entry(id)?;
        let place = pane.location_of(id).map(|place| {
            let tag = e
                .metadata
                .value(skwir_core::entry::keys::PLACE_TAG)
                .and_then(skwir_core::Value::as_str);
            (place, tag.map(|tag| self.tag_colors.of(tag)))
        });
        Some(Look {
            id,
            name: e.name_str().into_owned(),
            is_dir: e.is_dir(),
            selected: pane.selection.contains(id),
            cursor: pane.selection.cursor == Some(id),
            cut: self.pending.cut && self.pending.holds(e.file(), &e.name),
            icon: self.icons.for_entry(e, || pane.path_of(id), icon_px),
            link: icons::Link::of(e),
            tags: crate::tags::entry_tags(
                e,
                &self.tag_colors,
                &self.tag_pins,
                &self.ui().place.tags_shown,
            ),
            place,
            path: if e.is_dir() { pane.path_of(id) } else { None },
        })
    }
}

/// A row's or a tile's selection, cursor and hover.
pub fn highlight(el: Stateful<Div>, look: &Look, theme: &Theme) -> Stateful<Div> {
    el.border_1()
        .border_color(if look.cursor {
            theme.accent
        } else {
            gpui::transparent_black()
        })
        .when(look.selected, |d| {
            d.bg(theme.selection_bg).text_color(theme.selection_text)
        })
        .when(!look.selected, |d| d.hover(|s| s.bg(theme.hover)))
}

/// Where an entry stands among those its view shows, as assistive
/// technology says it: "readme.txt, 3 of 120", the count from the list box
/// that holds it (`aria_size_of_set` on the container, as AccessKit reads
/// it; the position counts from 0).
#[derive(Clone, Copy)]
pub struct Seat {
    /// Its index in the order its list box holds.
    pub index: usize,
    /// In a tree: its depth, from 0, and for a folder whether it is opened
    /// in place. Its place among its siblings is then left to the levels.
    pub tree: Option<(usize, Option<bool>)>,
}

/// What every entry answers, given to `el`, an entry of the pane being
/// drawn at `seat`. Its handlers name it by `look.id`, never by where it
/// was drawn: the order can change between a frame and the press on it.
pub fn behave(
    el: Stateful<Div>,
    look: &Look,
    seat: Seat,
    shell: &Shell,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let pane = shell.pane;
    let el = el
        .when(look.cut, |d| d.opacity(crate::views::list::CUT_OPACITY))
        .when(shell.a11y, |d| expose(d, look, seat))
        .children(shell.menu_marker(pane, look.cursor));
    let mut el = crate::list::entry_mouse(el, pane, look.id, cx);
    el = dnd::draggable(el, pane, cx);
    el = dnd::takes_gifts(el, pane, look.id, cx);
    // Where it leads takes what is dropped on it, a folder of the trash
    // as the trash does: its path is not the user's to drop into.
    if let Some((place, _)) = &look.place {
        el = dnd::droppable_place(el, place.clone(), cx);
    } else if let Some(dir) = &look.path {
        el = dnd::droppable_dir(el, dir.clone(), cx);
    }
    el
}

/// An entry as assistive technology hears it, an option of the pane's list
/// box or an item of its tree. The cursor's is the active descendant: the
/// pane container keeps the keyboard focus, and a screen reader says the
/// entry the keys move as the focused one.
fn expose(el: Stateful<Div>, look: &Look, seat: Seat) -> Stateful<Div> {
    let el = el
        .role(if seat.tree.is_some() {
            gpui::Role::TreeItem
        } else {
            gpui::Role::ListBoxOption
        })
        .aria_label(look.name.clone())
        .aria_selected(look.selected)
        .when(
            look.cursor,
            StatefulInteractiveElement::aria_active_descendant,
        );
    // An item of a tree stands among its siblings, which a screen reader
    // counts from the levels: the order's index would count them all.
    match seat.tree {
        Some((depth, open)) => el
            .aria_level(depth + 1)
            .when_some(open, StatefulInteractiveElement::aria_expanded),
        None => el.aria_position_in_set(seat.index),
    }
}

/// Its name, cut in the middle when too long (the extension tells files
/// apart), which says itself whole when pointed at while cut.
pub fn name(el: Stateful<Div>, look: &Look) -> AnyElement {
    crate::elements::clip::clipped(
        el.min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis_middle(),
        look.name.clone(),
    )
    .into_any_element()
}

/// Its tags as a strip that takes what it is given, giving way in steps
/// (`tags::strip`): chips, then dots, then "+N". It grows along a row: a
/// view that puts it under the name gives it a row of its own; one over
/// a picture gives it the picture's width.
pub fn tags(look: &Look, on: crate::tags::StripOn, theme: &Theme) -> ChipStrip {
    crate::tags::strip(&look.tags, on, theme)
}
