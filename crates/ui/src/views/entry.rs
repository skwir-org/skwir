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
use std::ffi::OsStr;
use std::path::PathBuf;

use gpui::{AnyElement, Context, Div, SharedString, Stateful, prelude::*};
use skwir_core::workspace::Pane;
use skwir_core::{EntryId, Location};

use crate::dnd;
use crate::elements::chips::ChipStrip;
use crate::icons;
use crate::shell::Shell;
use crate::theme::Theme;

/// A file's or a folder's name as it is drawn on its own (a row, a tile,
/// a card, a column's heading, a label on a map, a drag's): its
/// bidirectional controls shown as tags, never obeyed (ADR-139, 5), so
/// `Hello_` + U+202E + `gnp.js` reads `Hello_⟨RLO⟩gnp.js`, not as a
/// picture; nothing else added. Made from the name alone (`of`), so no
/// surface that draws one draws it raw. What is drawn never changes the
/// name: an operation, a copy or a search takes `Entry::name`. A name in a
/// sentence is a `Name` or `Folder` argument, a location's title
/// `App::location_title`, a crumb's `address::crumbs`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrawnName(SharedString);

impl DrawnName {
    /// `name`, a file's or a folder's, as it is drawn.
    pub fn of(name: &OsStr) -> Self {
        Self::of_str(&name.to_string_lossy())
    }

    /// `path` written whole, as it is drawn alone (a tooltip, a folder's
    /// row in a menu): made of names, each's controls shown.
    pub fn of_path(path: &std::path::Path) -> Self {
        Self::of_str(&path.display().to_string())
    }

    /// `name`, already text, as it is drawn.
    pub fn of_str(name: &str) -> Self {
        DrawnName(SharedString::from(
            skwir_core::words::shown_controls(name).into_owned(),
        ))
    }

    /// `name` as it is drawn, with `marks` (byte ranges of it: what a
    /// pattern matched) moved to where they fall in what is drawn: a mark
    /// on a control covers its tag.
    pub fn marked(
        name: &str,
        marks: &[std::ops::Range<usize>],
    ) -> (Self, Vec<std::ops::Range<usize>>) {
        let drawn = crate::input::Drawn::of(&SharedString::from(name.to_owned()), false);
        let marks = marks
            .iter()
            .map(|mark| drawn.shown(mark.start)..drawn.shown(mark.end))
            .collect();
        (DrawnName(drawn.text), marks)
    }

    /// What is drawn, to hand to an element.
    pub fn shared(&self) -> SharedString {
        self.0.clone()
    }
}

impl std::ops::Deref for DrawnName {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl From<DrawnName> for SharedString {
    fn from(name: DrawnName) -> Self {
        name.0
    }
}

/// What every view knows of an entry it draws.
#[allow(clippy::struct_excessive_bools)]
pub struct Look {
    pub id: EntryId,
    /// Its name as it is drawn (`DrawnName`), and as a screen reader
    /// hears it.
    pub name: DrawnName,
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
            name: DrawnName::of(&e.name),
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
        (Some(group), Some(said)) => Some(
            skwir_core::words!(
                views_entry_described = "{ $group }; { $said }",
                group = group,
                said = said,
                note = "What a screen reader hears after the name of the first file of a group: the group (Today, 3 items), then what marks the file (cut, hidden, its tags)."
            )
            .say(),
        ),
        (group, said) => group.or(said),
    };
    let el = el
        .role(if seat.tree.is_some() {
            gpui::Role::TreeItem
        } else {
            gpui::Role::ListBoxOption
        })
        .aria_label(look.name.shared())
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
        look.writing.then(|| {
            skwir_core::words!(
                views_entry_writing = "being written",
                note = "What a screen reader hears after a file's name: another program is still writing it."
            )
        }),
        look.cut.then(|| {
            skwir_core::words!(
                views_entry_cut = "cut",
                note = "What a screen reader hears after a file's name: it was cut, to be moved where it is pasted."
            )
        }),
        look.hidden.then(|| {
            skwir_core::words!(
                views_entry_hidden = "hidden",
                note = "What a screen reader hears after a file's name: it is a hidden file, though its name does not begin with a dot."
            )
        }),
        look.only_leads.then(|| {
            skwir_core::words!(
                views_entry_only_leads = "leads to what the filter keeps",
                note = "What a screen reader hears after a folder's name: it is shown only because files the filter keeps are inside it."
            )
        }),
    ];
    let marks: Vec<skwir_core::words::Words> = look
        .plays
        .map(crate::icons::Plays::said)
        .into_iter()
        .chain(look.vcs.and_then(|state| vcs_said(state, look.is_dir)))
        .chain(states.into_iter().flatten())
        .chain(crate::tags::said_words(&look.tags, look.rating))
        .collect();
    // Joined as the reader's language joins a list.
    (!marks.is_empty()).then(|| {
        skwir_core::words!(
            views_entry_marks = "{ $marks }",
            marks = skwir_core::words::List::and(marks),
            note = "What a screen reader hears after a file's name: what marks it, joined (cut, hidden and tagged print)."
        )
        .say()
    })
}

/// What a screen reader hears of the mark its repository's state puts on
/// an entry (`icons::vcs_mark`): a file's state, what a folder holds below
/// it; nothing for what is ignored, which is not marked.
fn vcs_said(state: skwir_core::vcs::State, is_dir: bool) -> Option<skwir_core::words::Words> {
    use skwir_core::vcs::State;
    use skwir_core::words;
    let words = match (state, is_dir) {
        (State::Ignored, _) => return None,
        (State::Conflict, false) => words!(
            views_entry_vcs_conflict = "in conflict",
            note = "What a screen reader hears after a file's name: its state in Git, a merge left it with both sides."
        ),
        (State::Changed, false) => words!(
            views_entry_vcs_changed = "changed",
            note = "What a screen reader hears after a file's name: its state in Git, changed since the last commit."
        ),
        (State::Staged, false) => words!(
            views_entry_vcs_staged = "staged",
            note = "What a screen reader hears after a file's name: its state in Git, its changes added for the next commit."
        ),
        (State::New, false) => words!(
            views_entry_vcs_new = "new",
            note = "What a screen reader hears after a file's name: its state in Git, in no commit yet."
        ),
        (State::Conflict, true) => words!(
            views_entry_vcs_holds_conflict = "holds a conflict",
            note = "What a screen reader hears after a folder's name: its state in Git, a file below it is in conflict."
        ),
        (State::Changed, true) => words!(
            views_entry_vcs_holds_changes = "holds changes",
            note = "What a screen reader hears after a folder's name: its state in Git, files below it changed."
        ),
        (State::Staged, true) => words!(
            views_entry_vcs_holds_staged = "holds staged changes",
            note = "What a screen reader hears after a folder's name: its state in Git, files below it have changes added for the next commit."
        ),
        (State::New, true) => words!(
            views_entry_vcs_holds_new = "holds new files",
            note = "What a screen reader hears after a folder's name: its state in Git, files below it are in no commit yet."
        ),
    };
    Some(words)
}

/// Its name, cut in the middle when too long (the extension tells files
/// apart), which says itself whole when pointed at while cut.
pub fn name(el: Stateful<Div>, look: &Look) -> AnyElement {
    crate::elements::clip::clipped(
        el.min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis_middle(),
        look.name.shared(),
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
            name: super::DrawnName::of_str("desktop.ini"),
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
        assert_eq!(said(&hidden).as_deref(), Some("hidden and tagged work"));
        let leads = Look {
            is_dir: true,
            only_leads: true,
            cut: true,
            ..look()
        };
        assert_eq!(
            said(&leads).as_deref(),
            Some("cut and leads to what the filter keeps")
        );
        // What plays first: it is what the badge on its picture says.
        let playing = Look {
            plays: Some(crate::icons::Plays::Paused),
            tags: vec![("work".into(), 0)],
            ..look()
        };
        assert_eq!(said(&playing).as_deref(), Some("paused and tagged work"));
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

    /// A name drawn alone (a row, a tile, a card, a column, the Folder
    /// column, a drag's label) shows each bidirectional control as a tag,
    /// never obeys it (ADR-139, 5): an override, an embedding, an isolate;
    /// a Hebrew name and the marks right-to-left names use are drawn as
    /// they are. What a view draws and a screen reader hears is the look's.
    #[test]
    fn a_name_drawn_alone_shows_its_controls_never_obeys_them() {
        use super::DrawnName;
        use std::ffi::OsStr;
        let drawn = |name: &str| DrawnName::of(OsStr::new(name)).to_string();
        assert_eq!(drawn("Hello_\u{202e}gnp.js"), "Hello_⟨RLO⟩gnp.js");
        assert_eq!(
            drawn("a\u{202d}b\u{202b}c\u{202c}d\u{2066}e\u{2069}"),
            "a⟨LRO⟩b⟨RLE⟩c⟨PDF⟩d⟨LRI⟩e⟨PDI⟩"
        );
        assert_eq!(drawn("שלום.txt"), "שלום.txt");
        assert_eq!(drawn("\u{200f}שלום\u{200e} 2"), "\u{200f}שלום\u{200e} 2");
        assert_eq!(drawn("readme.txt"), "readme.txt");
        assert_eq!(
            DrawnName::of_path(std::path::Path::new("/a/b\u{202e}c/d")).to_string(),
            "/a/b⟨RLO⟩c/d"
        );
        let spoof = Look {
            name: DrawnName::of(OsStr::new("Hello_\u{202e}gnp.js")),
            ..look()
        };
        assert_eq!(&*spoof.name, "Hello_⟨RLO⟩gnp.js");
        assert_eq!(spoof.name.shared(), "Hello_⟨RLO⟩gnp.js");
    }

    /// Marks on a name (what a pattern matched in the bulk rename's
    /// preview) fall on what is drawn: past a tag, moved by it; on a
    /// control, over its tag.
    #[test]
    fn marks_on_a_name_fall_where_it_is_drawn() {
        use super::DrawnName;
        let (drawn, marks) = DrawnName::marked("Hello_\u{202e}gnp.js", &[0..5, 6..9, 9..12]);
        assert_eq!(&*drawn, "Hello_⟨RLO⟩gnp.js");
        let tag = "⟨RLO⟩".len();
        assert_eq!(marks, [0..5, 6..6 + tag, 6 + tag..9 + tag]);
        assert_eq!(&drawn[marks[2].clone()], "gnp");
        let (plain, marks) = DrawnName::marked("a.txt", &[0..1, 2..5]);
        assert_eq!((&*plain, marks), ("a.txt", vec![0..1, 2..5]));
    }
}
