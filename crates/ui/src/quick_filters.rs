//! Quick filters: a bar of what the files shown can be filtered to, each
//! with how many files it keeps, a choice changing the pane's lens
//! (ADR-107, the query plugin's `query.narrow`), kept as the pane shows
//! other locations until taken off. Shown when asked for
//! (`ui.quick_filters`, the View menu's Quick filters) over any location,
//! and by itself over a search with a tag every match has and over a
//! tag's place: there it walks the tags as folders are walked.
//!
//! It offers the tags within each tag the location has (`Places` →
//! `France`), the other tags its files have, their kinds once the listing
//! is complete, and the years they were modified once every file's
//! metadata is in, what the lens sets already not offered again; Alt+click
//! leaves a tag out. Asked for, it always shows, and says why it offers
//! nothing: the files' details still read, no file there, or every file
//! alike. What it offers needs what each file's metadata says, so every
//! file is asked for, as the timeline does, and a file the lens cannot
//! tell yet waits hidden; the footer says how many are still read.
//!
//! What it offers is counted from the files the location lists, so it
//! holds for every facet: once per change of the listing, never per frame,
//! at most every `EVERY` while it streams, and a count under way answers
//! before another is asked. A count walks every file, so a worker does it
//! (`count`), reading the pane a slice at a time and typing the files
//! between the slices, once per suffix: the window's thread never waits
//! for a walk. The same count is the one asker of the metadata a filter
//! needs of the files no view draws, bar or not: it hands them over once
//! per change of the listing's names, and with the bar hidden it counts
//! nothing else.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use skwir_core::CancelToken;
use skwir_core::app::App;
use skwir_core::entry::EntryId;
use skwir_core::metadata::{same_tag, tag_leaf, tag_under, tag_within};
use skwir_core::workspace::PaneId;
use skwir_platform::mime::Kind;

use crate::shell::Shell;
use crate::theme::Theme;

/// How often what it offers is counted again while a search streams.
const EVERY: Duration = Duration::from_millis(500);
/// The entries a count reads under one hold of the workspace's lock.
const SLICE: usize = 1024;
/// The most of the other tags it offers.
const RELATED: usize = 12;

/// What a listing was counted for: its id and version, the pane's filter,
/// whether it was complete, and whether the bar showed (what it offers is
/// counted only then).
type Key = (u64, u64, u64, bool, bool);

/// What the files without metadata were last handed over for: the
/// listing's id and its names' version. Which filter needs them does not
/// change which they are.
type Handed = (u64, u64);

/// A choice and how many of the files shown it keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Count<T> {
    pub value: T,
    pub files: usize,
}

/// What a search can be narrowed to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Offer {
    /// Within each tag the search has, the tags one level in.
    pub within: Vec<(String, Vec<Count<String>>)>,
    /// The other tags the files have, those on every one left out.
    pub related: Vec<Count<String>>,
    pub kinds: Vec<Count<Kind>>,
    pub years: Vec<Count<i64>>,
    /// Nothing offered, every file known: what they all are.
    pub alike: Option<String>,
}

impl Offer {
    /// Whether it offers no choice.
    pub fn is_empty(&self) -> bool {
        self.within.is_empty()
            && self.related.is_empty()
            && self.kinds.is_empty()
            && self.years.is_empty()
    }
}

/// One file as the bar counts it.
pub struct File {
    pub tags: Vec<String>,
    pub kinds: Vec<Kind>,
    pub year: Option<i64>,
}

/// A pane's bar: what it offers, what that was counted for, and whether
/// it shows.
#[derive(Default)]
pub struct QuickFilters {
    /// What was last asked to be counted, and when.
    key: Option<Key>,
    counted: Option<Instant>,
    pub offer: Offer,
    /// What the last count found of the files, for what the bar says when
    /// it offers nothing and the footer's "Reading…".
    pub files: Files,
    /// The bar shows (asked for, or by itself), else the count only reads
    /// what a lens needs.
    shown: bool,
    /// A count of this listing came back: what `files` says holds.
    answered: bool,
    /// A count put off (`EVERY`), done by a wake of its own.
    recount: Option<gpui::Task<()>>,
    /// The count a worker is doing.
    asked: crate::latest::Latest<Counted>,
    /// What the files without metadata were last asked for: they are
    /// asked again only for new names, or once nothing needed them.
    handed: Option<Handed>,
}

/// What a filter that reads the files' metadata (a size, a tag) still
/// waits for (ADR-107), for the footer and the empty state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// Not counted yet.
    Counting,
    /// This many files.
    Files(usize),
}

/// How many files and folders the pane shows, how many files the location
/// lists in all (`listed`, those its filter hides too), and of those how
/// many have no metadata yet (`unknown`): read, they may be shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Files {
    pub files: usize,
    pub folders: usize,
    pub listed: usize,
    pub unknown: usize,
}

/// What a count is asked for, read on the window's thread.
struct Ask {
    pane: PaneId,
    tags: Vec<String>,
    /// The listing is complete.
    complete: bool,
    /// The pane filters what it lists (a folder, the trash, a tag's
    /// place, not a search): its files without metadata are counted,
    /// those it hides too.
    filtered: bool,
    /// The bar shows: what it offers is counted, else the files without
    /// metadata alone.
    shown: bool,
    /// The files without metadata are handed over, for this.
    hand: Option<Handed>,
    chosen: Chosen,
}

/// What the search or the location chooses already, which the bar does
/// not offer again.
#[derive(Clone, Copy)]
struct Chosen {
    kinds: bool,
    years: bool,
    /// A tag location lists the tags within it as places.
    within: bool,
}

/// What a count found: what the bar offers, the files and folders, and,
/// when handed over (`Ask::hand`), the files it cannot tell yet, which no
/// view draws, so the bar asks for them (`hydrate`).
struct Counted {
    offer: Offer,
    files: Files,
    unknown: Vec<EntryId>,
    handed: Option<Handed>,
}

/// Each value's count, most first, then in the order met.
fn ranked<T: Clone + Eq + std::hash::Hash>(values: impl Iterator<Item = T>) -> Vec<Count<T>> {
    let mut order: Vec<T> = Vec::new();
    let mut counts: HashMap<T, usize> = HashMap::new();
    for v in values {
        let n = counts.entry(v.clone()).or_default();
        if *n == 0 {
            order.push(v);
        }
        *n += 1;
    }
    let mut out: Vec<Count<T>> = order
        .into_iter()
        .map(|value| {
            let files = counts[&value];
            Count { value, files }
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.files));
    out
}

/// `counts` without what is on all `all` files: it narrows nothing.
fn narrowing<T>(counts: Vec<Count<T>>, all: usize) -> Vec<Count<T>> {
    counts.into_iter().filter(|c| c.files < all).collect()
}

/// What `files` (a search's results, every one with each of `tags`) can
/// be filtered to: their kinds once `listed` (every name in), their years
/// once `known` (every file's metadata in). A choice on every file
/// filters nothing and is left out.
pub fn offer(files: &[File], tags: &[String], listed: bool, known: bool) -> Offer {
    let all = files.len();
    let within = tags
        .iter()
        .map(|tag| {
            // The tags one level in, each file once.
            let inside = files.iter().flat_map(|f| {
                let mut seen: Vec<String> = Vec::new();
                for t in &f.tags {
                    let Some(rest) = tag_under(t, tag).and_then(|r| r.strip_prefix('/')) else {
                        continue;
                    };
                    let next = rest.split('/').next().unwrap_or(rest);
                    let child = format!("{tag}/{next}");
                    if !seen.iter().any(|s| same_tag(s, &child)) {
                        seen.push(child);
                    }
                }
                seen
            });
            (tag.clone(), narrowing(ranked(inside), all))
        })
        .filter(|(_, inside): &(String, Vec<Count<String>>)| !inside.is_empty())
        .collect();
    // The other tags: neither within a tag of the search nor holding one.
    let mut related = narrowing(
        ranked(files.iter().flat_map(|f| {
            let mut seen: Vec<String> = Vec::new();
            for t in &f.tags {
                let other = !tags.iter().any(|s| tag_within(t, s) || tag_within(s, t));
                if other && !seen.iter().any(|x| same_tag(x, t)) {
                    seen.push(t.clone());
                }
            }
            seen
        })),
        all,
    );
    related.truncate(RELATED);
    // Kinds once every name is in, years once every file's metadata: a
    // part of them says little.
    let kinds = if listed {
        narrowing(ranked(files.iter().flat_map(|f| f.kinds.clone())), all)
    } else {
        Vec::new()
    };
    let years = if known {
        let mut years = narrowing(ranked(files.iter().filter_map(|f| f.year)), all);
        years.sort_by_key(|c| std::cmp::Reverse(c.value));
        years
    } else {
        Vec::new()
    };
    Offer {
        within,
        related,
        kinds,
        years,
        alike: None,
    }
}

impl Shell {
    /// Count again what Quick filters offer over the pane being rendered,
    /// and what of its files its lens needs read, when its listing changed.
    pub(crate) fn sync_quick_filters(&mut self, cx: &mut Context<Self>) {
        self.take_quick_count();
        let pane = self.pane;
        let search = self.ui().place.search;
        // A search's tags, or those a place that offers by itself (a
        // tag's) says its files all have.
        let own = self.ui().place.verbs.quick_filters.clone();
        let tags = match &own {
            Some(own) => own.tags.clone(),
            None => self.ui().chips.tags().0,
        };
        let (lens_set, reads) = {
            let ws = self.app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            (
                p.lens().map(|l| l.set.clone()).unwrap_or_default(),
                // A lens that reads metadata, over what the pane filters:
                // the files no view draws are asked for, bar or not.
                !search && p.read_filter().is_some_and(|r| r.metadata),
            )
        };
        let shown = self.app.commands.get("query.narrow").is_some()
            && (self.prefs.quick_filters || !tags.is_empty() || own.is_some());
        if !shown && !reads {
            if self.ui().quick_filters.key.is_some() {
                self.ui_mut().quick_filters = QuickFilters::default();
            }
            return;
        }
        let (key, names) = {
            let ws = self.app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            (
                (
                    p.listing.id(),
                    p.listing.version(),
                    p.filter_version(),
                    p.listing.is_complete(),
                    shown,
                ),
                p.listing.names_version(),
            )
        };
        self.ui_mut().quick_filters.shown = shown;
        let before = self.ui().quick_filters.key;
        if before == Some(key) {
            return;
        }
        // A count under way for this listing, filter and bar answers
        // first: asked again, it would be cancelled, and one longer than
        // `EVERY` would never answer while the listing moves. Its answer
        // wakes the window, which counts again.
        let same =
            before.is_some_and(|before| (before.0, before.2, before.4) == (key.0, key.2, key.4));
        if same && self.ui().quick_filters.asked.is_pending() {
            return;
        }
        // Counted at most every `EVERY` while what it counts changes (a
        // search streaming, a folder's files hydrating, each batch moving
        // the listing): a count walks every file. The listing completing,
        // another filter or the bar shown counts at once; any other change
        // put off is counted by a wake of its own.
        let ended = key.3 && before.is_some_and(|before| !before.3);
        if same
            && !ended
            && let Some(at) = self.ui().quick_filters.counted
            && at.elapsed() < EVERY
        {
            if self.ui().quick_filters.recount.is_none() {
                let wait = EVERY.saturating_sub(at.elapsed());
                self.ui_mut().quick_filters.recount = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    let _ = this.update(cx, |shell, cx| {
                        shell.ui_mut().quick_filters.recount = None;
                        cx.notify();
                    });
                }));
            }
            return;
        }
        let lifetime = {
            let ws = self.app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            p.location_lifetime()
        };
        let chips = &self.ui().chips;
        let set = |facet: &str| chips.is_set(facet) || lens_set.iter().any(|s| s == facet);
        // The files without metadata, asked for once per change of the
        // names: the asker of what a filter needs, and of what the bar
        // offers of their years.
        let handed = (key.0, names);
        let ask = Ask {
            pane,
            tags,
            complete: key.3,
            filtered: !search,
            shown,
            hand: (self.ui().quick_filters.handed != Some(handed)).then_some(handed),
            chosen: Chosen {
                kinds: set("kind"),
                years: set("modified"),
                within: own.is_some(),
            },
        };
        let app = Arc::clone(&self.app);
        let worker = Arc::clone(&self.app);
        let lane = self.pane_lane(pane);
        let quick = &mut self.ui_mut().quick_filters;
        // Another listing: what was offered for the one before is not its
        // own, even while its count runs.
        if before.is_none_or(|before| before.0 != key.0) {
            quick.offer = Offer::default();
            quick.files = Files::default();
            quick.answered = false;
        }
        quick.key = Some(key);
        quick.counted = Some(Instant::now());
        quick.recount = None;
        quick.asked.ask(
            &app,
            lane,
            &lifetime,
            skwir_core::scheduler::Priority::Visible,
            "quick-filters-count",
            move |cancel| count(&worker, &ask, cancel),
        );
    }

    /// Take what a worker counted for the pane being rendered, and ask for
    /// the metadata of the files the lens cannot tell yet.
    fn take_quick_count(&mut self) {
        let Some(counted) = self.ui_mut().quick_filters.asked.take() else {
            return;
        };
        let quick = &mut self.ui_mut().quick_filters;
        quick.offer = counted.offer;
        quick.files = counted.files;
        quick.answered = true;
        if let Some(handed) = counted.handed {
            quick.handed = Some(handed);
            self.request_hydrate_off_screen(counted.unknown);
        }
    }
}

impl QuickFilters {
    /// What the pane's filter still waits for of its files' metadata, for
    /// the footer and the empty state: `None` when it reads none (`reads`
    /// false: a name, a kind; or the pane is a search) or every file's is
    /// in.
    pub fn reading(&self, reads: bool) -> Option<Reading> {
        if !reads {
            return None;
        }
        if !self.answered {
            return Some(Reading::Counting);
        }
        (self.files.unknown > 0).then_some(Reading::Files(self.files.unknown))
    }
}

/// What the files of `ask.pane` can be filtered to, on a worker. The
/// pane is read `SLICE` entries at a time, each slice under a hold of the
/// lock of its own, and what a file is (its type, its year) is worked out
/// after the slice lets the lock go. A file taken out meanwhile is left
/// out; a count of a listing that changed is asked again.
fn count(app: &App, ask: &Ask, cancel: &CancelToken) -> Counted {
    let mut counted = Counted {
        offer: Offer::default(),
        files: Files::default(),
        unknown: Vec::new(),
        handed: ask.hand,
    };
    let Some((order, held)) = app.workspace.read().pane(ask.pane).map(|p| {
        (
            ask.shown.then(|| p.order()),
            ask.filtered.then(|| p.unfiltered_order()),
        )
    }) else {
        return counted;
    };
    // What the pane filters is known once each of its files has its
    // metadata, those it hides included.
    for slice in held.iter().flat_map(|held| held.chunks(SLICE)) {
        if cancel.is_cancelled() {
            return counted;
        }
        let ws = app.workspace.read();
        let Some(p) = ws.pane(ask.pane) else {
            return counted;
        };
        for e in slice.iter().filter_map(|id| p.entry(*id)) {
            if e.is_dir() || e.is_place() {
                continue;
            }
            counted.files.listed += 1;
            if !e.metadata.is_hydrated() {
                counted.files.unknown += 1;
                if ask.hand.is_some() {
                    counted.unknown.push(e.id);
                }
            }
        }
    }
    // The bar hidden, the files without metadata were all it was for.
    let Some(order) = order else {
        return counted;
    };
    let listed = ask.complete;
    let known = listed && counted.files.unknown == 0;
    let calendar = skwir_platform::calendar::Calendar::system();
    // Typed once per suffix: a hundred thousand names are a few types.
    let types = skwir_platform::mime::BySuffix::default();
    let mut files: Vec<File> = Vec::with_capacity(order.len());
    for slice in order.chunks(SLICE) {
        if cancel.is_cancelled() {
            return counted;
        }
        let read: Vec<Read> = {
            let ws = app.workspace.read();
            let Some(p) = ws.pane(ask.pane) else {
                return counted;
            };
            let entries: Vec<&skwir_core::Entry> =
                slice.iter().filter_map(|id| p.entry(*id)).collect();
            counted.files.folders += entries
                .iter()
                .filter(|e| e.is_dir() && !e.is_place())
                .count();
            entries
                .into_iter()
                // A place (a tag's) is no file; a lens shows the folders
                // whatever it keeps.
                .filter(|e| !(e.is_place() || ask.filtered && e.is_dir()))
                .map(|e| Read {
                    tags: crate::tags::tags_of(e),
                    typed: listed.then(|| (e.inode_type(), e.type_name().to_owned())),
                    modified: e
                        .metadata
                        .value(skwir_core::entry::keys::MODIFIED)
                        .and_then(skwir_core::Value::as_time),
                })
                .collect()
        };
        files.extend(read.into_iter().map(|r| {
            File {
                tags: r.tags,
                kinds: r
                    .typed
                    .and_then(|(inode, name)| match inode {
                        Some(inode) => Some(skwir_platform::mime::Mime(inode.to_owned())),
                        None => types.of(&name),
                    })
                    .map(|m| skwir_platform::mime::kinds(&m))
                    .unwrap_or_default(),
                year: r.modified.map(|t| calendar.year_of(t)),
            }
        }));
    }
    counted.files.files = files.len();
    let mut offer = offer(&files, &ask.tags, listed, known);
    // What the search or the lens chooses already is not offered again.
    if ask.chosen.kinds {
        offer.kinds.clear();
    }
    if ask.chosen.years {
        offer.years.clear();
    }
    if ask.chosen.within {
        offer.within.clear();
    }
    if offer.is_empty() && known {
        offer.alike = alike(&files);
    }
    counted.offer = offer;
    counted
}

/// What `files` all are, when nothing tells them apart: `all 12 files are
/// images from 2024`, `one file (image, 2024)`.
fn alike(files: &[File]) -> Option<String> {
    let first = files.first()?;
    let kind = first
        .kinds
        .first()
        .copied()
        .filter(|k| files.iter().all(|f| f.kinds.contains(k)));
    let year = first
        .year
        .filter(|y| files.iter().all(|f| f.year == Some(*y)));
    if files.len() == 1 {
        let said: Vec<String> = kind
            .map(|k| k.name().to_owned())
            .into_iter()
            .chain(year.map(|y| y.to_string()))
            .collect();
        return Some(if said.is_empty() {
            "one file".to_owned()
        } else {
            format!("one file ({})", said.join(", "))
        });
    }
    let all = format!("all {} files are", crate::format::thousands(files.len()));
    let kind = kind.map(|k| k.title().to_lowercase());
    Some(match (kind, year) {
        (Some(kind), Some(year)) => format!("{all} {kind} from {year}"),
        (Some(kind), None) => format!("{all} {kind}"),
        (None, Some(year)) => format!("{all} from {year}"),
        (None, None) => format!("{all} alike"),
    })
}

/// What a count reads of a file under the lock: its type is told after.
struct Read {
    tags: Vec<String>,
    typed: Option<(Option<&'static str>, std::ffi::OsString)>,
    modified: Option<SystemTime>,
}

/// What the bar asked for says when it offers nothing: what it waits for,
/// or why there is nothing to choose.
fn why_empty(quick: &QuickFilters) -> String {
    let files = quick.files;
    if !quick.answered {
        "Counting the files…".to_owned()
    } else if files.unknown > 0 {
        format!(
            "Reading the details of {}…",
            crate::format::count(files.unknown, "file")
        )
    } else if files.files == 0 && files.listed > 0 {
        "Every file here is hidden by the filters".to_owned()
    } else if files.files == 0 && files.folders > 0 {
        "No files here, only folders".to_owned()
    } else if files.files == 0 {
        "No files here".to_owned()
    } else {
        match &quick.offer.alike {
            Some(alike) => format!("Nothing to filter: {alike}"),
            None => "Nothing to filter".to_owned(),
        }
    }
}

/// The bar of the pane being rendered: asked for, it always shows, saying
/// why it offers nothing; by itself (a search's tags, a tag's place), only
/// with something to offer.
pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let quick = &shell.ui().quick_filters;
    let offer = &quick.offer;
    if !quick.shown || (offer.is_empty() && !shell.prefs.quick_filters) {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    let command = skwir_core::command::Invocation::new("query.narrow");
    let label = |text: String| {
        div()
            .flex_none()
            .text_xs()
            .text_color(theme.text_muted)
            .child(text)
    };
    let mut row = div()
        .id(("quick-filters", pane.0 as usize))
        .flex_none()
        .h(px(30.))
        .px_2()
        .flex()
        .items_center()
        .gap(px(6.))
        .overflow_x_scroll()
        // A strip of one line, nothing scrolling around it: the wheel goes
        // along it too, not only with Shift.
        .map(|mut d| {
            d.style().restrict_scroll_to_axis = Some(false);
            d
        })
        .border_b_1()
        .border_color(theme.border);
    let mut n = 0usize;
    let mut pill = |text: String,
                    files: usize,
                    tone: Option<usize>,
                    tip: String,
                    facet: &'static str,
                    value: String,
                    leave_out: bool| {
        n += 1;
        let command = command.clone();
        let tip: SharedString = if leave_out {
            format!("{tip} · Alt+click: none tagged so").into()
        } else {
            tip.into()
        };
        div()
            // Known by what it narrows by: one taken away, the next that
            // comes under the pointer is not given its tooltip.
            .id(crate::elements::known_by("quick-filter", (facet, &value)))
            .flex_none()
            .h(px(22.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(11.))
            .text_xs()
            .text_color(theme.text)
            .when_some(tone, |d, tone| d.bg(theme.tag_fill(tone)))
            .when(tone.is_none(), |d| d.border_1().border_color(theme.border))
            .cursor_pointer()
            .hover(|s| s.border_color(theme.accent))
            .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, window, cx| {
                let facet = if leave_out && ev.modifiers().alt {
                    "not_tag"
                } else {
                    facet
                };
                this.activate_pane(pane, cx);
                let command = command
                    .clone()
                    .with("facet", facet)
                    .with("value", &value)
                    .with("pane", pane);
                this.run(&command.name, command.args, window, cx);
            }))
            .child(text)
            .child(
                div()
                    .text_color(theme.text_muted)
                    .child(crate::format::thousands(files)),
            )
    };
    let colors = &shell.tag_colors;
    for (tag, inside) in &offer.within {
        row = row.child(label(format!("In {}", tag_leaf(tag))));
        for c in inside {
            row = row.child(pill(
                tag_leaf(&c.value).to_owned(),
                c.files,
                Some(colors.of(&c.value)),
                c.value.clone(),
                "also_tag",
                c.value.clone(),
                true,
            ));
        }
    }
    if !offer.related.is_empty() {
        row = row.child(label("With".into()));
        for c in &offer.related {
            row = row.child(pill(
                tag_leaf(&c.value).to_owned(),
                c.files,
                Some(colors.of(&c.value)),
                c.value.clone(),
                "also_tag",
                c.value.clone(),
                true,
            ));
        }
    }
    if !offer.kinds.is_empty() {
        row = row.child(label("Kind".into()));
        for c in &offer.kinds {
            row = row.child(pill(
                c.value.title().to_owned(),
                c.files,
                None,
                c.value.title().to_owned(),
                "kind",
                c.value.name().to_owned(),
                false,
            ));
        }
    }
    if !offer.years.is_empty() {
        row = row.child(label("Modified".into()));
        for c in &offer.years {
            row = row.child(pill(
                c.value.to_string(),
                c.files,
                None,
                format!("Modified in {}", c.value),
                "modified",
                c.value.to_string(),
                false,
            ));
        }
    }
    if offer.is_empty() {
        row = row.child(label(why_empty(quick)));
    } else if quick.files.unknown > 0 {
        // What the details still read may offer more.
        row = row.child(label(format!(
            "Reading the details of {}…",
            crate::format::count(quick.files.unknown, "file")
        )));
    }
    Some(row.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only a filter that reads the files' metadata waits for it: a name
    /// or a kind never says "Reading"; before the first count it reads,
    /// how many not said yet, and with every file's in, nothing.
    #[test]
    fn only_a_filter_that_reads_metadata_is_reading() {
        let mut quick = QuickFilters::default();
        assert_eq!(quick.reading(false), None);
        assert_eq!(quick.reading(true), Some(Reading::Counting));
        quick.answered = true;
        quick.files.unknown = 3;
        assert_eq!(quick.reading(false), None, "a name filter waits for none");
        assert_eq!(quick.reading(true), Some(Reading::Files(3)));
        quick.files.unknown = 0;
        assert_eq!(quick.reading(true), None);
    }

    /// A place of files from many folders (the shelf, a search), each
    /// hydrated as such a place hydrates them (`fs::hydrate_paths`).
    #[cfg(unix)]
    struct Files(Vec<std::path::PathBuf>);

    #[cfg(unix)]
    struct OpenFiles {
        location: skwir_core::location::Location,
        rx: crossbeam_channel::Receiver<skwir_core::location::ListingEvent>,
        sink: skwir_core::location::EventSink,
        ctx: skwir_core::location::OpenContext,
        paths: Vec<(EntryId, std::path::PathBuf)>,
    }

    #[cfg(unix)]
    impl skwir_core::location::LocationProvider for Files {
        fn scheme(&self) -> &'static str {
            "files"
        }

        fn open(
            &self,
            location: &skwir_core::location::Location,
            ctx: skwir_core::location::OpenContext,
        ) -> anyhow::Result<Arc<dyn skwir_core::location::OpenLocation>> {
            use skwir_core::location::ListingEvent;
            let (sink, rx) = ctx.channel();
            let paths: Vec<(EntryId, std::path::PathBuf)> = (1..)
                .zip(&self.0)
                .map(|(ino, path)| {
                    let file = skwir_core::FileId::new(0, ino);
                    (EntryId { file, at: 0 }, path.clone())
                })
                .collect();
            sink.send(ListingEvent::Added(
                paths
                    .iter()
                    .map(|(id, path)| {
                        let name = path.file_name().unwrap_or_default();
                        skwir_core::Entry::new(*id, name, skwir_core::EntryKind::File)
                    })
                    .collect(),
            ));
            sink.send(ListingEvent::Complete);
            Ok(Arc::new(OpenFiles {
                location: location.clone(),
                rx,
                sink,
                ctx,
                paths,
            }))
        }
    }

    #[cfg(unix)]
    impl skwir_core::location::OpenLocation for OpenFiles {
        fn location(&self) -> &skwir_core::location::Location {
            &self.location
        }
        fn events(&self) -> &crossbeam_channel::Receiver<skwir_core::location::ListingEvent> {
            &self.rx
        }
        fn hydrate(&self, ids: &[EntryId], _: skwir_core::scheduler::Priority) {
            let items: Vec<(EntryId, std::path::PathBuf)> = self
                .paths
                .iter()
                .filter(|(id, _)| ids.contains(id))
                .cloned()
                .collect();
            skwir_core::fs::hydrate_paths(&items, &self.ctx.metadata, &self.sink, &self.ctx.cancel);
        }
        fn path_of(&self, id: EntryId) -> Option<std::path::PathBuf> {
            self.paths
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, p)| p.clone())
        }
        fn refresh(&self) {}
        fn close(&self) {}
    }

    /// A file in a folder that cannot be opened is answered once asked
    /// for (`ListingEvent::Unreadable`): the count no longer says it is
    /// read, where it waited for it forever.
    #[cfg(unix)]
    #[test]
    fn a_file_that_cannot_be_read_is_not_read_forever() {
        use skwir_core::location::Location;
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::home_dir()
            .expect("home")
            .join(".cache")
            .join("skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("quick-")
            .tempdir_in(base)
            .unwrap();
        let shut = tmp.path().join("it's shut");
        std::fs::create_dir(&shut).unwrap();
        let note = shut.join("a note.txt");
        std::fs::write(&note, b"read me").unwrap();
        let open = tmp.path().join("an open one.txt");
        std::fs::write(&open, b"read me too").unwrap();
        let mode = |mode| {
            std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        // Passed through, never opened.
        mode(0o300);
        if std::fs::read_dir(&shut).is_ok() {
            // Whoever reads every folder (root) cannot be refused one.
            mode(0o700);
            return;
        }
        let app = Arc::new(
            App::new(skwir_core::app::AppConfig::ephemeral(
                tmp.path().join("state"),
            ))
            .unwrap(),
        );
        app.register_provider("test", Arc::new(Files(vec![note, open])));
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::new("files", "")).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
        let ask = Ask {
            pane,
            tags: Vec::new(),
            complete: true,
            filtered: true,
            shown: false,
            hand: Some((0, 0)),
            chosen: Chosen {
                kinds: false,
                years: false,
                within: false,
            },
        };
        let counted = count(&app, &ask, &CancelToken::new());
        assert_eq!(counted.files.unknown, 2, "none asked for yet");
        app.dispatch_args(
            "hydrate",
            skwir_core::app::HydrateArgs {
                ids: counted.unknown,
                pane: Some(pane),
                off_screen: true,
            },
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let answered = loop {
            app.pump();
            let counted = count(&app, &ask, &CancelToken::new());
            if counted.files.unknown == 0 || Instant::now() > deadline {
                break counted;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        mode(0o700);
        assert_eq!(answered.files.unknown, 0, "answered, and read no more");
        assert_eq!(answered.files.listed, 2);
    }

    fn file(tags: &[&str], kinds: &[Kind], year: i64) -> File {
        File {
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            kinds: kinds.to_vec(),
            year: Some(year),
        }
    }

    /// Nothing to offer, the bar says what every file is.
    #[test]
    fn files_alike_are_said_so() {
        let images = [
            file(&[], &[Kind::Image], 2024),
            file(&[], &[Kind::Image], 2024),
        ];
        assert_eq!(
            alike(&images).as_deref(),
            Some("all 2 files are images from 2024")
        );
        assert_eq!(
            alike(&images[..1]).as_deref(),
            Some("one file (image, 2024)")
        );
        let mixed = [
            file(&[], &[Kind::Image], 2023),
            file(&[], &[Kind::Image], 2024),
        ];
        assert_eq!(alike(&mixed).as_deref(), Some("all 2 files are images"));
        assert_eq!(alike(&[]), None);
    }

    #[test]
    fn a_search_of_tags_offers_what_is_within_and_beside_them() {
        let files = [
            file(&["Places/France/Paris", "family"], &[Kind::Image], 2024),
            file(&["Places/France/Lyon", "family"], &[Kind::Image], 2023),
            file(&["Places/Japan", "work"], &[Kind::Video], 2024),
            file(&["Places", "family"], &[Kind::Image], 2024),
        ];
        let o = offer(&files, &["Places".into()], true, true);
        assert_eq!(
            o.within,
            [(
                "Places".to_owned(),
                vec![
                    Count {
                        value: "Places/France".to_owned(),
                        files: 2
                    },
                    Count {
                        value: "Places/Japan".to_owned(),
                        files: 1
                    },
                ]
            )],
            "a file in France twice counts once"
        );
        assert_eq!(
            o.related,
            [
                Count {
                    value: "family".to_owned(),
                    files: 3
                },
                Count {
                    value: "work".to_owned(),
                    files: 1
                },
            ]
        );
        assert_eq!(
            o.kinds,
            [
                Count {
                    value: Kind::Image,
                    files: 3
                },
                Count {
                    value: Kind::Video,
                    files: 1
                },
            ]
        );
        assert_eq!(o.years[0].value, 2024, "the latest first");
        // What every file has narrows nothing; while it streams, no kinds.
        let o = offer(&files[..2], &["Places/France".into()], false, false);
        assert!(o.related.is_empty(), "both have family");
        assert!(o.kinds.is_empty() && o.years.is_empty());
        assert_eq!(o.within[0].1.len(), 2);
    }
}
