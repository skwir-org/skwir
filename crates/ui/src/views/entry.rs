//! What every view draws of an entry and what it does with it, one way.
//! `Look` is read under the lock once per entry drawn (`Shell::look`);
//! `highlight` draws a row's or a tile's selection, cursor and hover; and
//! `behave` gives the element what every entry answers: dimmed while it
//! waits to be pasted elsewhere, the menu's marker on the cursor, pressed
//! and clicked as the pane's entry, dragged as the selection, taking what
//! a dragged place gives (a tag), and taking files dropped where it leads,
//! and what assistive technology hears of it: its name, its tags and
//! stars, whether it is selected, where it stands among the entries
//! (`Seat`), and, on the cursor, that it is where the keys are. A view
//! draws its own shape around these (a row, a tile, a card), never its
//! own copy of them.

use crate::theme::Chosen;
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
    /// A folder shown only as the way to what a filter keeps below it
    /// (`Pane::only_leads`): drawn dimmed.
    pub only_leads: bool,
    /// Hidden though its name does not say so (`Entry::hidden_unsaid`):
    /// marked, never dimmed, as what a file is.
    pub hidden: bool,
    /// Its icon, the theme's or the system's, at the size asked.
    pub icon: Option<icons::Pic>,
    pub link: icons::Link,
    /// Each tag with its color (`tags::TagColors`).
    pub tags: Vec<(String, usize)>,
    /// Its rating in half stars, where its notes keep one: what a screen
    /// reader hears with its tags (`tags::said`).
    pub rating: Option<u64>,
    /// The color of its ring while a place of the sidebar is pointed at
    /// whose tags it has (`Shell::ring_of`).
    pub ring: Option<usize>,
    /// A place (a tag's), or a folder that does not open at its path (a
    /// trashed one): where it leads, and a tag's color.
    pub place: Option<(Location, Option<usize>)>,
    /// A folder's own path: where files dropped on it go.
    pub path: Option<PathBuf>,
    /// The file the player plays, playing or paused (`Shell::plays`).
    pub plays: Option<icons::Plays>,
    /// What its repository says of it (ADR-134, `keys::VCS`): marked on its
    /// picture, said with its name.
    pub vcs: Option<skwir_core::vcs::State>,
    /// Another program writes it still (ADR-135, `keys::WRITING`): a pill
    /// says so, its size growing as it comes.
    pub writing: bool,
}

impl Look {
    /// What its picture is marked with.
    pub fn mark(&self) -> icons::Marks {
        icons::Marks {
            mark: icons::Mark::of(self.link, self.place.as_ref().and_then(|(_, tag)| *tag)),
            hidden: self.hidden,
            plays: self.plays,
            vcs: self.vcs,
        }
    }

    /// Its picture when it has no thumbnail, `size` px (`icons::entry_picture`).
    pub fn picture(&self, size: f32, theme: &Theme) -> AnyElement {
        icons::entry_picture(
            self.icon.as_ref(),
            self.is_dir,
            size,
            self.mark(),
            self.selected,
            theme,
        )
    }
}

impl Shell {
    /// Entry `id` of `pane` as every view draws it, its icon at `icon_px`.
    /// Under the caller's lock: `pane` is read from it.
    pub fn look(&mut self, pane: &Pane, id: EntryId, icon_px: u16) -> Option<Look> {
        let e = pane.drawn_entry(id)?;
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
            // While the keys are on a heading, no entry is where they are.
            cursor: pane.selection.cursor == Some(id) && self.ui().list.heading.is_none(),
            cut: self.pending.cut && self.pending.holds(e.file(), &e.name),
            only_leads: pane.only_leads(id),
            hidden: e.hidden_unsaid(),
            icon: self.icons.for_entry(e, || pane.path_of(id), icon_px),
            link: icons::Link::of(e),
            tags: crate::tags::entry_tags(
                e,
                &self.tag_colors,
                &self.tag_pins,
                &self.ui().place.tags_shown,
            ),
            rating: e
                .metadata
                .value(skwir_core::entry::keys::RATING)
                .and_then(skwir_core::Value::as_u64),
            ring: self.ring_of(e),
            place,
            path: if e.is_dir() { pane.path_of(id) } else { None },
            plays: self.plays(&e.name, || pane.path_of(id)),
            vcs: skwir_core::vcs::State::of(e),
            writing: matches!(
                e.metadata.value(skwir_core::entry::keys::WRITING),
                Some(skwir_core::Value::Bool(true))
            ),
        })
    }
}

/// A row's or a tile's selection, cursor and hover, and its ring while a
/// place of the sidebar whose tags it has is pointed at: the tag's color
/// around it and its tint under it.
pub fn highlight(el: Stateful<Div>, look: &Look, theme: &Theme) -> Stateful<Div> {
    el.border_1()
        // The cursor's ring is a mark: on the selection, in its text.
        .border_color(match (look.cursor, look.ring) {
            (true, _) if look.selected => theme.selection_text,
            (true, _) => theme.accent,
            (false, Some(ring)) => theme.tag(ring),
            (false, None) => gpui::transparent_black(),
        })
        .when(look.selected, |d| d.chosen(theme))
        .when_some(look.ring.filter(|_| !look.selected), |d, ring| {
            d.bg(theme.tag_fill(ring))
        })
        .when(!look.selected, |d| d.hover(|s| s.bg(theme.hover)))
}

/// Where an entry stands among those its view shows, as assistive
/// technology says it: "readme.txt, 3 of 120", the count from the list box
/// that holds it (`aria_size_of_set` on the container, as AccessKit reads
/// it; the position counts from 0).
#[derive(Clone)]
pub struct Seat {
    /// Its index in the order its list box holds.
    pub index: usize,
    /// In a tree: its depth, from 0, and for a folder whether it is opened
    /// in place. Its place among its siblings is then left to the levels.
    pub tree: Option<(usize, Option<bool>)>,
    /// The first entry of a group drawn under its heading: what is heard
    /// of the group ("Today, 3 items", `groups::said`), said first among
    /// what describes it, as the arrows cross into the group.
    pub group: Option<String>,
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
        .when(look.only_leads && !look.cut && !look.selected, |d| {
            d.opacity(crate::views::list::ONLY_LEADS_OPACITY)
        })
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
    // Its group where it is the first of it, then its tags and stars, as
    // the views draw them.
    let described = match (seat.group, said(look)) {
        (Some(group), Some(said)) => Some(format!("{group}; {said}")),
        (group, said) => group.or(said),
    };
    let el = el
        .role(if seat.tree.is_some() {
            gpui::Role::TreeItem
        } else {
            gpui::Role::ListBoxOption
        })
        .aria_label(look.name.clone())
        .when_some(described, StatefulInteractiveElement::aria_description)
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

/// What is said of an entry beside its name: whether it plays, what dims
/// or marks it (cut, hidden though its name does not say so, only the way
/// to what a filter keeps), its tags and stars; what is drawn is heard.
fn said(look: &Look) -> Option<String> {
    let states = [
        (look.writing, "being written"),
        (look.cut, "cut"),
        (look.hidden, "hidden"),
        (look.only_leads, "leads to what the filter keeps"),
    ];
    let words: Vec<String> = look
        .plays
        .map(icons::Plays::said)
        .into_iter()
        .map(str::to_owned)
        .chain(look.vcs.and_then(|state| vcs_said(state, look.is_dir)))
        .chain(
            states
                .into_iter()
                .filter(|(on, _)| *on)
                .map(|(_, word)| word.to_owned()),
        )
        .chain(crate::tags::said(&look.tags, look.rating))
        .collect();
    (!words.is_empty()).then(|| words.join(", "))
}

/// What a screen reader hears of the mark its repository's state puts on
/// an entry (`icons::vcs_mark`): a file's state, what a folder holds below
/// it; nothing for what is ignored, which is not marked.
fn vcs_said(state: skwir_core::vcs::State, is_dir: bool) -> Option<String> {
    if state == skwir_core::vcs::State::Ignored {
        return None;
    }
    let words = if is_dir { state.below() } else { state.title() };
    Some(words.to_lowercase())
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
/// (`tags::strip`): chips, then dots, then "+N", each answering as `act`
/// says (`Shell::tag_act`: a right press, the tag's menu). It grows along
/// a row: a view that puts it under the name gives it a row of its own;
/// one over a picture gives it the picture's width.
pub fn tags(
    look: &Look,
    on: crate::tags::StripOn,
    theme: &Theme,
    act: Option<crate::tags::TagAct>,
) -> ChipStrip {
    crate::tags::strip_acting(&look.tags, on, theme, act)
}

#[cfg(test)]
mod tests {
    use super::{Look, said};
    use skwir_core::{EntryId, FileId};

    fn look() -> Look {
        Look {
            id: EntryId {
                file: FileId::new(1, 1),
                at: 0,
            },
            name: "desktop.ini".into(),
            is_dir: false,
            selected: false,
            cursor: false,
            cut: false,
            only_leads: false,
            hidden: false,
            icon: None,
            link: crate::icons::Link::None,
            tags: Vec::new(),
            rating: None,
            ring: None,
            place: None,
            path: None,
            plays: None,
            vcs: None,
            writing: false,
        }
    }

    /// What fades or marks an entry is heard beside its name, before its
    /// tags: what is drawn of it is said.
    #[test]
    fn what_fades_or_marks_an_entry_is_heard() {
        assert_eq!(said(&look()), None);
        let cut = Look {
            cut: true,
            ..look()
        };
        assert_eq!(said(&cut).as_deref(), Some("cut"));
        let hidden = Look {
            hidden: true,
            tags: vec![("work".into(), 0)],
            ..look()
        };
        assert_eq!(said(&hidden).as_deref(), Some("hidden, Tagged work"));
        let leads = Look {
            is_dir: true,
            only_leads: true,
            cut: true,
            ..look()
        };
        assert_eq!(
            said(&leads).as_deref(),
            Some("cut, leads to what the filter keeps")
        );
        // What plays first: it is what the badge on its picture says.
        let playing = Look {
            plays: Some(crate::icons::Plays::Paused),
            tags: vec![("work".into(), 0)],
            ..look()
        };
        assert_eq!(said(&playing).as_deref(), Some("paused, Tagged work"));
        assert_eq!(
            playing.mark().plays,
            Some(crate::icons::Plays::Paused),
            "and its picture is marked with it"
        );
        // What its repository says: a file's state, what a folder holds,
        // nothing for what is ignored, as its picture is marked.
        let changed = Look {
            vcs: Some(skwir_core::vcs::State::Changed),
            ..look()
        };
        assert_eq!(said(&changed).as_deref(), Some("changed"));
        assert_eq!(changed.mark().vcs, Some(skwir_core::vcs::State::Changed));
        let holds = Look {
            is_dir: true,
            vcs: Some(skwir_core::vcs::State::Staged),
            ..look()
        };
        assert_eq!(said(&holds).as_deref(), Some("holds staged changes"));
        let ignored = Look {
            vcs: Some(skwir_core::vcs::State::Ignored),
            ..look()
        };
        assert_eq!(said(&ignored), None);
    }

    /// The hidden mark goes with the entry's other one, not in its place.
    #[test]
    fn a_hidden_link_keeps_both_its_marks() {
        let link = Look {
            hidden: true,
            link: crate::icons::Link::Leads,
            ..look()
        };
        assert_eq!(
            link.mark(),
            crate::icons::Marks {
                mark: crate::icons::Mark::Link(crate::icons::Link::Leads),
                hidden: true,
                plays: None,
                vcs: None,
            }
        );
    }
}
