//! "Narrow by": a search of tags walked like folders. Over a pane showing
//! a search with a tag every match has, a bar offers where it can go:
//! the tags within each of its tags (`Places` → `France`), the other tags
//! its files have, with how many files have each, and, once the search has
//! ended, their kinds and years when the search does not choose them yet.
//! A click narrows the search to it (`query.refine`: a tag within one
//! takes its place, another is one more every match has), Alt+click leaves
//! a tag out; Back undoes either, and the tag's pill in the address goes
//! back up.
//!
//! Over a tag location (ADR-032) it offers the same, but the tags within
//! its tag, which it lists as places: a choice narrows it as it says
//! (`Verbs::narrow`, the tags plugin's `tags.narrow`), kept while its
//! trail is walked.
//!
//! Over a folder, when asked for (`ui.narrow_folders`, the View menu's
//! Narrow by) or while it is narrowed, it offers its files' kinds, years
//! and tags: a choice narrows the folder (`query.narrow`, a
//! `workspace::Narrowed`), kept while the pane shows it or a folder
//! under it. Its folders always show. What it offers needs what each
//! file's metadata says, so every file is asked for, as the timeline
//! does, and a file the narrowing cannot tell yet waits hidden.
//!
//! What it offers is counted from the search's own results, so it holds
//! for every facet the search has: once per change of the listing, never
//! per frame, at most every `EVERY` while the search streams. A count
//! walks every file, so a worker does it (`count`), reading the pane a
//! slice at a time and typing the files between the slices: the window's
//! thread never waits for a walk.

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
/// and whether it was complete.
type Key = (u64, u64, u64, bool);

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
}

/// One file as the bar counts it.
pub struct File {
    pub tags: Vec<String>,
    pub kinds: Vec<Kind>,
    pub year: Option<i64>,
}

/// A pane's bar: what it offers, what that was counted for, and what a
/// choice runs: the location's own narrowing (a tag location's,
/// `Verbs::narrow`), else the query plugin's.
#[derive(Default)]
pub struct Narrow {
    /// What was last asked to be counted, and when.
    key: Option<Key>,
    counted: Option<Instant>,
    pub offer: Offer,
    command: Option<skwir_core::command::Invocation>,
    /// A count put off (`EVERY`), done by a wake of its own.
    recount: Option<gpui::Task<()>>,
    /// The count a worker is doing.
    asked: crate::latest::Latest<Counted>,
}

/// What a count is asked for, read on the window's thread.
struct Ask {
    pane: PaneId,
    tags: Vec<String>,
    /// The listing is complete.
    complete: bool,
    /// A folder narrowed: its folders are not counted, and its files
    /// without metadata are asked for.
    folder: bool,
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

/// What a count found: what the bar offers, and in a folder narrowed the
/// files it cannot tell yet, which no view draws, so the bar asks for them
/// (`hydrate`).
struct Counted {
    offer: Offer,
    unknown: Vec<EntryId>,
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
/// be narrowed to. A choice on every file narrows nothing and is left out.
pub fn offer(files: &[File], tags: &[String], complete: bool) -> Offer {
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
    // Kinds and years once every file is in: a part of them says little.
    let (kinds, years) = if complete {
        let kinds = narrowing(ranked(files.iter().flat_map(|f| f.kinds.clone())), all);
        let mut years = narrowing(ranked(files.iter().filter_map(|f| f.year)), all);
        years.sort_by_key(|c| std::cmp::Reverse(c.value));
        (kinds, years)
    } else {
        (Vec::new(), Vec::new())
    };
    Offer {
        within,
        related,
        kinds,
        years,
    }
}

impl Shell {
    /// Count again what the pane being rendered's search can be narrowed
    /// to, when its listing changed.
    pub(crate) fn sync_narrow(&mut self, cx: &mut Context<Self>) {
        self.take_narrow_count();
        // A search's tags, or what a location that narrows itself (a tag
        // location) says its files all have.
        let (mut tags, _) = self.ui().chips.tags();
        let mut own = self.ui().place.verbs.narrow.clone();
        // A folder narrows through the query plugin, when asked for or
        // while it is narrowed.
        let folder = self.folder_narrowing();
        if let Some(narrowing) = &folder {
            own = Some(narrowing.clone());
        }
        if let Some(own) = &own {
            tags.clone_from(&own.tags);
        }
        if tags.is_empty() && own.is_none() {
            if self.ui().narrow.key.is_some() {
                self.ui_mut().narrow = Narrow::default();
            }
            return;
        }
        let pane = self.pane;
        let key = {
            let ws = self.app.workspace.read();
            let Some(p) = ws.pane(pane) else { return };
            (
                p.listing.id(),
                p.listing.version(),
                p.filter_version(),
                p.listing.is_complete(),
            )
        };
        if self.ui().narrow.key == Some(key) {
            return;
        }
        // Counted at most every `EVERY` while what it counts changes (a
        // search streaming, a folder's files hydrating, each batch moving
        // the listing): a count walks every file. The listing completing
        // counts at once; any other change put off is counted by a wake
        // of its own.
        let ended = key.3 && self.ui().narrow.key.is_some_and(|before| !before.3);
        if !ended
            && let Some(at) = self.ui().narrow.counted
            && at.elapsed() < EVERY
        {
            if self.ui().narrow.recount.is_none() {
                let wait = EVERY.saturating_sub(at.elapsed());
                self.ui_mut().narrow.recount = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    let _ = this.update(cx, |shell, cx| {
                        shell.ui_mut().narrow.recount = None;
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
        let set = |facet: &str| {
            chips.is_set(facet)
                || own
                    .as_ref()
                    .is_some_and(|own| own.set.iter().any(|s| s == facet))
        };
        let ask = Ask {
            pane,
            tags,
            complete: key.3,
            folder: folder.is_some(),
            chosen: Chosen {
                kinds: set("kind"),
                years: set("modified"),
                within: own.is_some(),
            },
        };
        let app = Arc::clone(&self.app);
        let worker = Arc::clone(&self.app);
        let lane = self.pane_lane(pane);
        let narrow = &mut self.ui_mut().narrow;
        // Another listing: what was offered for the one before is not its
        // own, even while its count runs.
        if narrow.key.is_none_or(|before| before.0 != key.0) {
            narrow.offer = Offer::default();
        }
        narrow.key = Some(key);
        narrow.counted = Some(Instant::now());
        narrow.command = own.map(|own| own.command);
        narrow.recount = None;
        narrow.asked.ask(
            &app,
            lane,
            &lifetime,
            skwir_core::scheduler::Priority::Visible,
            "narrow-count",
            move |cancel| count(&worker, &ask, cancel),
        );
    }

    /// Take what a worker counted for the pane being rendered, and ask for
    /// the metadata of the files a folder's narrowing cannot tell yet.
    fn take_narrow_count(&mut self) {
        let Some(counted) = self.ui_mut().narrow.asked.take() else {
            return;
        };
        self.ui_mut().narrow.offer = counted.offer;
        self.request_hydrate_off_screen(counted.unknown);
    }
}

/// What the files of `ask.pane` can be narrowed to, on a worker. The pane
/// is read `SLICE` entries at a time, each slice under a hold of the lock
/// of its own, and what a file is (its type, its year) is worked out
/// after the slice lets the lock go. A file taken out meanwhile is left
/// out; a count of a listing that changed is asked again.
fn count(app: &App, ask: &Ask, cancel: &CancelToken) -> Counted {
    let mut counted = Counted {
        offer: Offer::default(),
        unknown: Vec::new(),
    };
    let Some((order, held)) = app
        .workspace
        .read()
        .pane(ask.pane)
        .map(|p| (p.order(), ask.folder.then(|| p.unfiltered_order())))
    else {
        return counted;
    };
    // A folder's files are complete once each has its metadata.
    for slice in held.iter().flat_map(|held| held.chunks(SLICE)) {
        if cancel.is_cancelled() {
            return counted;
        }
        let ws = app.workspace.read();
        let Some(p) = ws.pane(ask.pane) else {
            return counted;
        };
        counted.unknown.extend(slice.iter().copied().filter(|id| {
            p.entry(*id)
                .is_some_and(|e| !e.is_dir() && !e.is_place() && !e.metadata.is_hydrated())
        }));
    }
    let complete = ask.complete && counted.unknown.is_empty();
    let calendar = skwir_platform::calendar::Calendar::system();
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
            slice
                .iter()
                .filter_map(|id| p.entry(*id))
                // A place (a tag's) is no file; a folder narrowed shows
                // its folders whatever it offers.
                .filter(|e| !(e.is_place() || ask.folder && e.is_dir()))
                .map(|e| Read {
                    tags: crate::tags::tags_of(e),
                    typed: complete.then(|| (e.inode_type(), e.type_name().to_owned())),
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
                    .and_then(|(inode, name)| skwir_platform::mime::of_parts(inode, &name))
                    .map(|m| skwir_platform::mime::kinds(&m))
                    .unwrap_or_default(),
                year: r.modified.map(|t| calendar.year_of(t)),
            }
        }));
    }
    let mut offer = offer(&files, &ask.tags, complete);
    // What the search chooses already is not offered again.
    if ask.chosen.kinds {
        offer.kinds.clear();
    }
    if ask.chosen.years {
        offer.years.clear();
    }
    if ask.chosen.within {
        offer.within.clear();
    }
    counted.offer = offer;
    counted
}

/// What a count reads of a file under the lock: its type is told after.
struct Read {
    tags: Vec<String>,
    typed: Option<(Option<&'static str>, std::ffi::OsString)>,
    modified: Option<SystemTime>,
}

impl Shell {
    /// How the folder the pane being rendered shows is narrowed, when it
    /// is or the bar is asked for over folders, and the query plugin is
    /// there to do it.
    fn folder_narrowing(&self) -> Option<skwir_core::Narrowing> {
        if !self.ui().place.folder || self.app.commands.get("query.narrow").is_none() {
            return None;
        }
        let set = {
            let ws = self.app.workspace.read();
            let p = ws.pane(self.pane)?;
            p.location()?.path()?;
            p.narrowed().map(|n| n.set.clone())
        };
        (self.prefs.narrow_folders || set.is_some()).then(|| skwir_core::Narrowing {
            command: skwir_core::command::Invocation::new("query.narrow"),
            tags: Vec::new(),
            set: set.unwrap_or_default(),
        })
    }
}

/// The bar of the pane being rendered, when its search can be narrowed.
pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let offer = &shell.ui().narrow.offer;
    if offer.within.is_empty()
        && offer.related.is_empty()
        && offer.kinds.is_empty()
        && offer.years.is_empty()
    {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    let command = shell
        .ui()
        .narrow
        .command
        .clone()
        .unwrap_or_else(|| skwir_core::command::Invocation::new("query.refine"));
    let label = |text: String| {
        div()
            .flex_none()
            .text_xs()
            .text_color(theme.text_muted)
            .child(text)
    };
    let mut row = div()
        .id(("narrow", pane.0 as usize))
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
            .id(crate::elements::known_by("narrow-pill", (facet, &value)))
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
    Some(row.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(tags: &[&str], kinds: &[Kind], year: i64) -> File {
        File {
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            kinds: kinds.to_vec(),
            year: Some(year),
        }
    }

    #[test]
    fn a_search_of_tags_offers_what_is_within_and_beside_them() {
        let files = [
            file(&["Places/France/Paris", "family"], &[Kind::Image], 2024),
            file(&["Places/France/Lyon", "family"], &[Kind::Image], 2023),
            file(&["Places/Japan", "work"], &[Kind::Video], 2024),
            file(&["Places", "family"], &[Kind::Image], 2024),
        ];
        let o = offer(&files, &["Places".into()], true);
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
        let o = offer(&files[..2], &["Places/France".into()], false);
        assert!(o.related.is_empty(), "both have family");
        assert!(o.kinds.is_empty() && o.years.is_empty());
        assert_eq!(o.within[0].1.len(), 2);
    }
}
