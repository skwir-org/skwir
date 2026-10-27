//! The search designer: a pane that shows a search draws it in its address
//! field as pills, one per facet the query plugin describes as set
//! (`query.describe`): where it looks first, then the words, the kinds of
//! file, when, size, tag, contents, and any part of the search no facet
//! holds. "+ Filter" lists the facets not set. A pill opens its choices in
//! a popover anchored to it; a choice runs the invocation the plugin gave
//! (`query.refine`, or `ui.ask` for a value to type), which moves the pane
//! to the refined search, so Back undoes a choice. A set pill has a button
//! that takes it out.
//!
//! A tag's pill reads as a path: its categories, each going back up to it
//! (`Places › France`), and "+" between two tags every match has.
//!
//! The chips are read once per location a pane shows, never per frame, and
//! the UI knows nothing of the query language: the text stays the one
//! truth, which the address field edits as it is (Ctrl+L, or a click
//! beside the pills).
//!
//! What narrows a folder is the same chips after its trail
//! (`query.describe_filter`), each with a funnel, as no search's has: the
//! location's own (a tag's `with`), Narrow by's, then the words typed over
//! it, each opening its choices, which change it in place; "+ Filter"
//! adds to the words typed.

use gpui::{AnyElement, Context, SharedString, Window, div, prelude::*, px};
use serde::Deserialize;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::Crumb;
use skwir_core::metadata::{TAG_SEPARATOR, TagRef};
use skwir_core::workspace::PaneId;

use crate::elements::popover::PopoverAnchor;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::{self, Button, Popup};

/// A facet of the search as the plugin describes it.
#[derive(Clone, Debug, Deserialize)]
pub struct Chip {
    pub id: String,
    pub title: String,
    pub value: Option<String>,
    pub icon: String,
    pub choices: Vec<Choice>,
    pub clear: Option<Invocation>,
    /// A tag's: its categories, each going up to it, then itself.
    #[serde(default)]
    pub segments: Vec<Segment>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Segment {
    pub label: String,
    #[serde(default)]
    pub command: Option<Invocation>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Choice {
    pub label: String,
    pub command: Invocation,
    pub active: bool,
}

impl Chip {
    /// What the chip reads: its facet, and what it is set to.
    pub fn label(&self) -> String {
        match (&self.value, self.title.is_empty()) {
            (Some(value), true) => value.clone(),
            (Some(value), false) => format!("{}: {value}", self.title),
            (None, _) => self.title.clone(),
        }
    }

    /// Where the search looks: always drawn, first.
    fn is_scope(&self) -> bool {
        self.id == "where"
    }

    /// Drawn as a pill: where it looks, and every facet that is set.
    fn is_pill(&self) -> bool {
        self.is_scope() || self.value.is_some()
    }
}

/// What the query plugin says of the search a pane shows.
#[derive(Clone, Debug, Default, Deserialize)]
struct Description {
    chips: Vec<Chip>,
    #[serde(default, with = "skwir_core::os_path")]
    root: std::path::PathBuf,
    #[serde(default)]
    text: String,
    #[serde(default)]
    within: Option<skwir_core::Location>,
}

/// The chips of the location a pane shows, read when it changed or the
/// sidebar's names did (where it looks is named as the sidebar names it),
/// and the search cut in two for the field that edits it: where it looks,
/// and the rest.
#[derive(Default)]
pub struct Chips {
    location: String,
    /// The sidebar's names they were read with (`Roots::generation`).
    names: u64,
    pub chips: Vec<Chip>,
    pub root: std::path::PathBuf,
    pub text: String,
    /// The place it looks within instead of `root` (`in:recent:`).
    pub within: Option<skwir_core::Location>,
}

impl Chips {
    /// The tags every match of the search has, and those none has.
    pub fn tags(&self) -> (Vec<String>, Vec<String>) {
        let of = |id: &str| {
            self.chips
                .iter()
                .filter(|c| c.id == id)
                .filter_map(|c| c.value.clone())
                .collect()
        };
        (of("tag"), of("not_tag"))
    }

    /// Whether the facet `id` is set.
    pub fn is_set(&self, id: &str) -> bool {
        self.chips.iter().any(|c| c.id == id && c.value.is_some())
    }
}

/// What narrows the folder a pane shows, as the search's chips, read when
/// it changed: the location's own (a tag's `with`), Narrow by's, then the
/// words typed over it, set and not ("+ Filter" lists those not). None
/// over a search, whose chips are its own.
#[derive(Default)]
pub struct Filters {
    /// The location, words and narrowing they were read for.
    key: String,
    pub chips: Vec<Chip>,
    /// What Narrow by's say, for the words of a folder narrowed to nothing.
    pub narrowing: Vec<String>,
}

/// What the query plugin says of what narrows a folder.
#[derive(Default, Deserialize)]
struct FolderDescription {
    narrowing: Vec<Chip>,
    filter: Vec<Chip>,
}

/// What a pane's search popover shows: one chip's choices, or the facets
/// not set ("+ Filter").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Chip(usize),
    Facets,
}

/// The chip whose choices are open.
pub struct ChipMenu {
    pane: PaneId,
    target: Target,
    popup: Popup,
}

/// What a chip's toggle names its choices (`Shell::toggle_popup`).
fn chip_key(at: (PaneId, Target)) -> String {
    format!("chip {at:?}")
}

impl Shell {
    /// `ui.search_chip { pane, chip }` opens a chip's choices, or closes
    /// them when they are open; `{ pane, facets: true }` lists the facets
    /// not set; `{ close: true }` closes whichever is open.
    pub fn toggle_search_chip(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let close = args.get("close").and_then(serde_json::Value::as_bool) == Some(true);
        let wanted = (!close)
            .then(|| {
                let pane = serde_json::from_value::<PaneId>(args.get("pane")?.clone()).ok()?;
                let target =
                    if args.get("facets").and_then(serde_json::Value::as_bool) == Some(true) {
                        Target::Facets
                    } else {
                        Target::Chip(usize::try_from(args.get("chip")?.as_u64()?).ok()?)
                    };
                Some((pane, target))
            })
            .flatten();
        let key = wanted.map(chip_key).unwrap_or_default();
        if self.toggle_popup(&key, args, window, cx)
            && let Some((pane, target)) = wanted
        {
            self.search_menu = Some(ChipMenu {
                pane,
                target,
                popup: Popup::new("search-chip", window, cx),
            });
        }
        cx.notify();
    }

    /// Close the open chip's choices, giving the focus back; what its
    /// toggle names them.
    pub fn close_search_chip(&mut self, window: &mut Window, cx: &mut gpui::App) -> Option<String> {
        let menu = self.search_menu.take()?;
        menu.popup.restore(window, cx);
        Some(chip_key((menu.pane, menu.target)))
    }
}

/// Read the chips of what the pane being rendered shows: a search's, or
/// what narrows its folder.
pub fn sync(shell: &mut Shell, window: &mut Window, cx: &mut gpui::App) {
    sync_search(shell, window, cx);
    sync_filters(shell, window, cx);
}

/// Read the chips of the search the pane being rendered shows, when it
/// shows another one than they were read for or the sidebar's names
/// changed since; none for a folder.
fn sync_search(shell: &mut Shell, window: &mut Window, cx: &mut gpui::App) {
    let pane = shell.pane;
    let location = shell
        .app
        .pane_location(pane)
        .filter(|_| shell.ui().place.search);
    let shown = location
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default();
    let names = shell.roots.generation;
    if shell.ui().chips.location == shown && shell.ui().chips.names == names {
        return;
    }
    let described = match location {
        Some(_) if shell.app.commands.get("query.describe").is_some() => shell
            .app
            .dispatch(&Invocation::with_args(
                "query.describe",
                &serde_json::json!({ "pane": pane }),
            ))
            .ok()
            .and_then(|outcome| match outcome {
                Outcome::Value(value) => serde_json::from_value::<Description>(value).ok(),
                _ => None,
            })
            .unwrap_or_default(),
        _ => Description::default(),
    };
    shell.ui_mut().chips = Chips {
        location: shown,
        names,
        chips: described.chips,
        root: described.root,
        text: described.text,
        within: described.within,
    };
    // A menu belongs to the search it was opened on.
    if shell.search_menu.as_ref().is_some_and(|m| m.pane == pane) {
        shell.close_search_chip(window, cx);
    }
}

/// Read what narrows the folder the pane being rendered shows, when it,
/// the words typed over it or its narrowing changed; not while the field
/// is typed in, which shows the text.
fn sync_filters(shell: &mut Shell, window: &mut Window, cx: &mut gpui::App) {
    let pane = shell.pane;
    if shell.ui().address.read(cx).is_focused(window) {
        return;
    }
    let (typed, narrowed) = {
        let ws = shell.app.workspace.read();
        let p = ws.pane(pane);
        (
            p.and_then(|p| p.filter().map(str::to_owned)),
            p.and_then(|p| p.narrowed().map(|n| n.expr.clone())),
        )
    };
    let search = !shell.ui().chips.chips.is_empty();
    let own = shell
        .ui()
        .place
        .trail
        .as_ref()
        .map(|t| t.filters.clone())
        .unwrap_or_default();
    let key = if search {
        String::new()
    } else {
        let location = shell
            .app
            .pane_location(pane)
            .map(|l| l.to_string())
            .unwrap_or_default();
        format!("{location}\0{typed:?}\0{narrowed:?}")
    };
    if shell.ui().filters.key == key {
        return;
    }
    let mut chips: Vec<Chip> = if search {
        Vec::new()
    } else {
        own.iter().map(own_chip).collect()
    };
    let mut narrowing = Vec::new();
    if !search && (typed.is_some() || narrowed.is_some()) {
        let described: FolderDescription = shell
            .app
            .commands
            .get("query.describe_filter")
            .and_then(|_| {
                let ask = Invocation::with_args(
                    "query.describe_filter",
                    &serde_json::json!({ "pane": pane }),
                );
                match shell.app.dispatch(&ask) {
                    Ok(Outcome::Value(value)) => serde_json::from_value(value).ok(),
                    _ => None,
                }
            })
            .unwrap_or_default();
        narrowing = described.narrowing.iter().map(Chip::label).collect();
        let read = described.filter.iter().any(Chip::is_pill);
        chips.extend(described.narrowing);
        chips.extend(described.filter);
        if let Some(text) = typed.filter(|_| !read) {
            chips.push(typed_chip(&text));
        }
    }
    shell.ui_mut().filters = Filters {
        key,
        chips,
        narrowing,
    };
    // A menu belongs to what it was opened on.
    if !search && shell.search_menu.as_ref().is_some_and(|m| m.pane == pane) {
        shell.close_search_chip(window, cx);
    }
}

/// A location's own narrowing (a tag's `with`) as a chip: a tag's as that
/// tag, one left out as not tagged, the rest as the location says it;
/// taking it off goes where the location leads without it.
fn own_chip(crumb: &Crumb) -> Chip {
    let (id, title, value, segments) = match &crumb.tag {
        Some(tag) if !tag.lacks => (
            "tag",
            "Tag",
            tag.name.clone(),
            tag.name
                .split(TAG_SEPARATOR)
                .map(|label| Segment {
                    label: label.to_owned(),
                    command: None,
                })
                .collect(),
        ),
        Some(tag) => ("not_tag", "Not tagged", tag.name.clone(), Vec::new()),
        None => ("own", "", crumb.label.clone(), Vec::new()),
    };
    Chip {
        id: id.to_owned(),
        title: title.to_owned(),
        value: Some(value),
        icon: "filter".to_owned(),
        choices: Vec::new(),
        clear: Some(Invocation::navigate(&crumb.location)),
        segments,
    }
}

/// The words typed when nothing read them into facets (the query plugin
/// away, or no start of them reads), as typed: Edit types them again, its
/// × shows everything, as Escape in the pane does.
fn typed_chip(text: &str) -> Chip {
    Chip {
        id: "typed".to_owned(),
        title: String::new(),
        value: Some(format!("“{text}”")),
        icon: "filter".to_owned(),
        choices: vec![Choice {
            label: "Edit…".to_owned(),
            command: Invocation::with_args("ui.search", &serde_json::json!({ "text": text })),
            active: false,
        }],
        clear: Some(Invocation::new("filter")),
        segments: Vec::new(),
    }
}

/// The chips the pane being rendered draws: its search's, or what narrows
/// its folder.
fn shown_chips(shell: &Shell) -> &[Chip] {
    let search = &shell.ui().chips.chips;
    if search.is_empty() {
        &shell.ui().filters.chips
    } else {
        search
    }
}

/// The search the pane being rendered shows, as pills for its address
/// field: where it looks, each facet set, "+ Filter", then room that edits
/// the text when clicked. `None` when there are no chips (a folder, or the
/// query plugin away): the field shows text. `anchor` goes to what the
/// open popover hangs from. No width of its own, like the trail.
pub fn pills(shell: &Shell, anchor: &PopoverAnchor, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if shell.ui().chips.chips.is_empty() {
        return None;
    }
    // The richest that fits (`Fit`): the words searched stay in view
    // however narrow the pane.
    Some(
        div()
            .flex_1()
            .w(px(0.))
            .min_w_0()
            .h(px(24.))
            .child(crate::elements::adaptive::AdaptiveRow {
                variants: [Fit::Full, Fit::Compact, Fit::Tight]
                    .into_iter()
                    .map(|fit| line(shell, anchor, fit, cx))
                    .collect(),
                height: 24.,
                chosen: None,
            })
            .into_any_element(),
    )
}

/// How the search's pills fit their room, richest first.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fit {
    /// Every pill as it reads.
    Full,
    /// Where it looks and "+ Filter" as their icons alone, the facets as
    /// their icon and value (`🔍 rapport`), each said in full in its
    /// tooltip.
    Compact,
    /// The facets alone, each its value without icon (`rapport` for
    /// "Name: rapport", said in full in its tooltip). Where it looks and
    /// the facets to add are the field's, with Ctrl+F or a click beside.
    Tight,
}

/// The search's pills on one line, as `fit` says, then room that edits
/// the text.
fn line(shell: &Shell, anchor: &PopoverAnchor, fit: Fit, cx: &mut Context<Shell>) -> AnyElement {
    let least = if fit == Fit::Tight { 8. } else { 24. };
    div()
        .size_full()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(4.))
        .px(px(3.))
        .children(pill_row(shell, anchor, fit, false, cx))
        .child(rest(shell, false, least, cx))
        .into_any_element()
}

/// What is left after the pills, or after a trail: it edits the address as
/// text, as a click on the trail does.
fn rest(shell: &Shell, folder: bool, least: f32, cx: &mut Context<Shell>) -> AnyElement {
    let pane = shell.pane;
    div()
        .id(("chip-rest", pane.0 as usize))
        .role(gpui::Role::Button)
        .aria_label(if folder {
            "Edit the address as text"
        } else {
            "Edit the search as text"
        })
        .flex_1()
        .min_w(px(least))
        .h_full()
        .cursor_text()
        .on_click(cx.listener(move |this, _, window, cx| {
            this.activate_pane(pane, cx);
            this.run("ui.focus_address", serde_json::json!({}), window, cx);
        }))
        .into_any_element()
}

/// What follows a folder's trail: what narrows it as pills, each with a
/// funnel, then room that edits the address (that room alone when nothing
/// narrows it). The pills know their natural width (`adaptive::Fit`), so
/// they and the trail give way alike, each by its width, and they draw
/// the richest that fits what they got (`Fit`), right after the trail.
pub fn after_trail(
    shell: &Shell,
    anchor: &PopoverAnchor,
    cx: &mut Context<Shell>,
) -> Vec<AnyElement> {
    let mut after = Vec::new();
    if shell.ui().chips.chips.is_empty() && !shell.ui().filters.chips.is_empty() {
        after.push(
            crate::elements::adaptive::Fit {
                variants: [Fit::Full, Fit::Compact, Fit::Tight]
                    .into_iter()
                    .map(|fit| {
                        // As wide as it is given, so the last fit shrinks
                        // its pills there.
                        div()
                            .w_full()
                            .h(px(24.))
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .px(px(3.))
                            .children(pill_row(shell, anchor, fit, true, cx))
                            .into_any_element()
                    })
                    .collect(),
                cuts: true,
                start: true,
            }
            .into_any_element(),
        );
    }
    after.push(rest(shell, true, 24., cx));
    after
}

/// The pills of what the pane shows (`shown_chips`) as `fit` says, then
/// "+ Filter" while a facet is not set. A folder's (`folder`) wear a
/// funnel whatever the fit, a search's their facet's icon.
fn pill_row(
    shell: &Shell,
    anchor: &PopoverAnchor,
    fit: Fit,
    folder: bool,
    cx: &mut Context<Shell>,
) -> Vec<AnyElement> {
    let chips = shown_chips(shell);
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    let open = shell
        .search_menu
        .as_ref()
        .filter(|m| m.pane == pane)
        .map(|m| m.target);
    // A chip without a pill opens from "+ Filter", like the list of them.
    let from_filter = match open {
        Some(Target::Facets) => true,
        Some(Target::Chip(i)) => chips.get(i).is_some_and(|c| !c.is_pill()),
        None => false,
    };
    let order = chips
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_scope() && fit != Fit::Tight)
        .chain(
            chips
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.is_scope() && c.is_pill()),
        );
    let mut line: Vec<AnyElement> = Vec::new();
    let mut after_tag = false;
    for (shown, (i, chip)) in order.enumerate() {
        // Tags every match has, one after the other: "+" between them.
        let is_tag = chip.id == "tag" && !chip.segments.is_empty();
        if is_tag && after_tag {
            line.push(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("+")
                    .into_any_element(),
            );
        }
        after_tag = is_tag;
        // A tag every match has, or none has, wears the tag's colors, as
        // the pill of a folder narrowed by it does: hollow for none.
        let tint = chip
            .value
            .clone()
            .and_then(|tag| match chip.id.as_str() {
                "tag" => Some(TagRef::of(tag)),
                "not_tag" => Some(TagRef::lacking(tag)),
                _ => None,
            })
            .and_then(|tag| toolbar::Tint::of(&tag, &shell.tag_colors));
        let ink = if tint.is_some() {
            theme.text_muted
        } else {
            theme.selection_text
        };
        let tip = if folder {
            format!("{} · this folder only, not a search", chip.label())
        } else if chip.title.is_empty() {
            "Part of the search no chip holds".to_owned()
        } else if fit != Fit::Full {
            // Its icon or its value alone: the tooltip says it all.
            chip.label()
        } else {
            chip.title.clone()
        };
        let icon = match (chip.is_scope(), fit) {
            _ if folder => "filter".to_owned(),
            (true, _) => "folder".to_owned(),
            (false, Fit::Tight) => String::new(),
            (false, _) => chip.icon.clone(),
        };
        // Its ×, but on a folder's at the last fit: the funnel stays, and
        // what is left goes to the value (its menu removes it).
        let crossed = chip.clear.is_some() && !(folder && fit == Fit::Tight);
        // Known by what it holds: one removed, the next that comes under
        // the pointer is not given its tooltip.
        let known = crate::elements::key_of((&chip.id, &chip.value, &chip.title, i));
        let mut button = Button::new(
            SharedString::from(format!("chip-{known:x}")),
            icon,
            tip,
            "ui.search_chip",
        )
        .args(serde_json::json!({ "pane": pane, "chip": i }));
        match fit {
            _ if is_tag => {}
            Fit::Full => button = button.label(chip.label()),
            Fit::Compact if chip.is_scope() => {}
            Fit::Compact | Fit::Tight => {
                button = button.label(chip.value.clone().unwrap_or_else(|| chip.label()));
            }
        }
        let pill = button
            .active(!chip.is_scope())
            .tint(tint)
            .anchor_if(anchor, open == Some(Target::Chip(i)))
            .render(shell, cx)
            .h(px(22.))
            .rounded_full()
            .when(chip.is_scope(), |d| d.border_color(theme.border))
            .when(crossed, |d| d.pr(px(2.)))
            // The last fit: cut short rather than gone, its label keeping
            // both ends; the later a pill, the sooner it gives way, so the
            // first facet (the words typed, usually) stays readable.
            .when(fit == Fit::Tight, |mut d| {
                d.style().flex_shrink = Some(1. + 3. * shown as f32);
                // A funnel besides what is left of the value.
                d.min_w(px(if folder { 48. } else { 32. }))
            });
        // A tag's categories go back up to them; the tag itself opens the
        // pill's choices, as the rest of it does.
        let pill = if is_tag {
            tag_segments(pill, i, &chip.segments, ink.opacity(0.7), pane, cx)
        } else {
            pill
        };
        // A pointer's shortcut, not a focusable button: it goes with what
        // it removes, and a focused element that goes takes the keyboard
        // with it. The chip's own menu ends with Remove for the keyboard.
        let pill = match chip.clear.clone().filter(|_| crossed) {
            Some(clear) => pill.child(crate::elements::pill::clear(
                shell,
                clear,
                format!("Remove {}", chip.label()).into(),
                ink,
                cx,
            )),
            None => pill,
        };
        line.push(pill.into_any_element());
    }
    if fit != Fit::Tight && chips.iter().any(|c| !c.is_pill()) {
        let facets = Button::new(
            "chip-facets",
            "plus",
            if folder {
                "Filter this folder by name, kind, date, size or tag"
            } else {
                "Narrow the search by name, kind, date, size, tag or contents"
            },
            "ui.search_chip",
        )
        .args(serde_json::json!({ "pane": pane, "facets": true }));
        let facets = if fit == Fit::Full {
            facets.label("Filter")
        } else {
            facets
        };
        line.push(
            facets
                .anchor_if(anchor, from_filter)
                .active(open == Some(Target::Facets))
                .render(shell, cx)
                .h(px(22.))
                .rounded_full()
                .border_color(theme.border)
                .into_any_element(),
        );
    }
    line
}

/// A tag's pill's segments: each category a link up to it, `›` between,
/// in `muted`.
fn tag_segments(
    mut pill: gpui::Stateful<gpui::Div>,
    chip: usize,
    segments: &[Segment],
    muted: gpui::Hsla,
    pane: PaneId,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    for (j, segment) in segments.iter().enumerate() {
        if j > 0 {
            pill = pill.child(div().flex_none().text_color(muted).child("›"));
        }
        pill = match segment.command.clone() {
            Some(up) => {
                let tip: SharedString = format!("All of “{}”", segment.label).into();
                pill.child(
                    div()
                        .id(SharedString::from(format!("chip-{chip}-up-{j}")))
                        .role(gpui::Role::Link)
                        .aria_label(tip.clone())
                        .flex_none()
                        .text_color(muted)
                        .cursor_pointer()
                        .hover(gpui::Styled::underline)
                        .tooltip(move |_, cx| toolbar::tooltip(tip.clone(), cx))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.activate_pane(pane, cx);
                            let up = up.clone().with("pane", pane);
                            this.run(&up.name, up.args, window, cx);
                        }))
                        .child(segment.label.clone()),
                )
            }
            None => pill.child(crate::elements::clip::clipped(
                div()
                    .id(SharedString::from(format!("chip-{chip}-tag")))
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis(),
                segment.label.clone(),
            )),
        };
    }
    pill
}

/// The open popover of the pane being rendered's search: a chip's choices,
/// or the facets not set, under what `pills` gave `anchor`.
pub fn popup(shell: &Shell, anchor: &PopoverAnchor, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let pane = shell.pane;
    let menu = shell.search_menu.as_ref().filter(|m| m.pane == pane)?;
    let chips = shown_chips(shell);
    let theme = cx.global::<Theme>().clone();
    let mut panel = toolbar::popup_panel(&menu.popup, "ui.search_chip", &theme, cx);
    match menu.target {
        Target::Facets => {
            for (i, chip) in chips.iter().enumerate().filter(|(_, c)| !c.is_pill()) {
                panel = panel.child(
                    Button::new(
                        SharedString::from(format!("chip-facet-{i}")),
                        chip.icon.clone(),
                        chip.title.clone(),
                        "ui.search_chip",
                    )
                    .args(serde_json::json!({ "pane": pane, "chip": i }))
                    .label(chip.title.clone())
                    .menu()
                    .render(shell, cx),
                );
            }
        }
        Target::Chip(i) => {
            let chip = chips.get(i)?;
            let remove = chip.clear.as_ref().map(|clear| Choice {
                label: "Remove".to_owned(),
                command: clear.clone(),
                active: false,
            });
            // An advanced part's only choice is already its removal.
            let remove = remove.filter(|_| !chip.title.is_empty());
            for (j, choice) in chip.choices.iter().chain(remove.iter()).enumerate() {
                panel = panel.child(
                    Button::new(
                        SharedString::from(format!("chip-choice-{j}")),
                        if choice.active { "check" } else { "" },
                        choice.label.clone(),
                        choice.command.name.to_string(),
                    )
                    .args(choice.command.args.clone())
                    .label(choice.label.clone())
                    .menu()
                    .render(shell, cx),
                );
            }
        }
    }
    Some(anchor.popup(panel))
}
