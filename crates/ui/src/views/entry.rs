//! What every view draws of an entry and what it does with it, one way.
//! `Look` is read under the lock once per entry drawn (`Shell::look`);
//! `highlight` draws a row's or a tile's selection, cursor and hover; and
//! `behave` gives the element what every entry answers: dimmed while it
//! waits to be pasted elsewhere, the menu's marker on the cursor, pressed
//! and clicked as the pane's entry, dragged as the selection, taking what
//! a dragged place gives (a tag), and taking files dropped where it leads.
//! A view draws its own shape around these (a row, a tile, a card), never
//! its own copy of them.

use std::path::PathBuf;

use bibli_core::workspace::Pane;
use bibli_core::{EntryId, Location};
use gpui::{AnyElement, Context, Div, Stateful, prelude::*};

use crate::dnd;
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
                .value(bibli_core::entry::keys::PLACE_TAG)
                .and_then(bibli_core::Value::as_str);
            (place, tag.map(|tag| self.tag_colors.of(tag)))
        });
        Some(Look {
            id,
            name: e.name_str().into_owned(),
            is_dir: e.is_dir(),
            selected: pane.selection.contains(id),
            cursor: pane.selection.cursor == Some(id),
            cut: self.pending.cut && self.pending.entries.contains(&id),
            icon: self.icons.for_entry(e, icon_px),
            link: icons::Link::of(e),
            tags: crate::tags::entry_tags(e, &self.tag_colors),
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

/// What every entry answers, given to `el`, the entry at `ix` of the
/// pane being drawn.
pub fn behave(
    el: Stateful<Div>,
    look: &Look,
    ix: usize,
    shell: &Shell,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let pane = shell.pane;
    let el = el
        .when(look.cut, |d| d.opacity(crate::views::list::CUT_OPACITY))
        .children(shell.menu_marker(pane, look.cursor));
    let mut el = crate::list::entry_mouse(el, pane, ix, cx);
    el = dnd::draggable(el, pane, cx);
    el = dnd::takes_gifts(el, pane, ix, cx);
    // Where it leads takes what is dropped on it, a folder of the trash
    // as the trash does: its path is not the user's to drop into.
    if let Some((place, _)) = &look.place {
        el = dnd::droppable_place(el, place.clone(), cx);
    } else if let Some(dir) = &look.path {
        el = dnd::droppable_dir(el, dir.clone(), cx);
    }
    el
}

/// Its name, cut in the middle when too long (the extension tells files
/// apart), which says itself whole when pointed at while cut.
pub fn name(el: Stateful<Div>, look: &Look, shell: &Shell) -> AnyElement {
    crate::elements::clip::clipped(
        el.min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis_middle(),
        look.name.clone(),
        shell.font.clone(),
    )
    .into_any_element()
}

/// Its tags as chips in a line that takes what it is given: those that do
/// not fit drop whole, last first, "+N" saying how many. It grows along a
/// row: a view that puts it under the name gives it a row of its own.
pub fn tags(look: &Look, theme: &Theme) -> crate::elements::chips::ChipLine {
    let (text, muted) = if look.selected {
        (theme.selection_text, theme.selection_text)
    } else {
        (theme.text, theme.text_muted)
    };
    crate::elements::chips::ChipLine {
        chips: look
            .tags
            .iter()
            .map(|(tag, color)| {
                crate::tags::chip(tag, *color, theme)
                    .text_color(text)
                    .into_any_element()
            })
            .collect(),
        more: Box::new(move |n| crate::tags::more_chip(n, muted).into_any_element()),
        height: crate::tags::CHIP_HEIGHT,
        gap: 4.,
    }
}
