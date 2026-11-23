//! Each pane's footer: what it lists and what is selected there. The
//! window's own state (what plays while the preview is closed, the files
//! waiting for a paste, proposals to review, the shelf, what runs, Undo)
//! ends the footer of the last pane, at the window's bottom right
//! whichever way a split runs. The side panels have none: the sidebar's
//! facts are on its rows and the preview's in Properties, so they run to
//! the window's foot. Messages are toasts (`set_status`), never a
//! footer's.

use crate::sizes::TextSizes;
use crate::sizes::sized;
use gpui::{AnyElement, Context, div, prelude::*};
use skwir_core::coverage::Proof;
use skwir_core::location::RestsOn;

use crate::elements::clip::clipped;
use crate::format;
use crate::shell::Shell;
use crate::theme::Chosen;
use crate::theme::Theme;
use crate::toast::{Action, Toast, Tone};
use skwir_core::words;
use skwir_core::words::{Size, Words};

impl Shell {
    /// Tell the user something, said where the toast is drawn; errors stay
    /// longer and are logged, in English.
    pub fn set_status(&mut self, text: impl Into<Words>, error: bool, cx: &mut Context<Self>) {
        let text = text.into();
        if error {
            tracing::warn!(text = %text.english(), "status");
        }
        self.toast(
            Toast::new(
                text,
                if error { Tone::Error } else { Tone::Info },
                Vec::new(),
            ),
            cx,
        );
    }

    /// A command failed. Nothing selected, reached from the palette or a
    /// menu, is a hint rather than a failure. The error handed over is
    /// said link by link in the window's language (`Words::of_error`).
    pub fn report_error(&mut self, error: anyhow::Error, cx: &mut Context<Self>) {
        if skwir_core::command::is_nothing_selected(&error) {
            self.set_status(crate::shell::nothing_selected(), false, cx);
        } else if skwir_core::command::wants_clipboard(&error) {
            self.set_status(
                words!(
                    status_nothing_to_paste = "Nothing to paste",
                    note = "Said when Paste is asked and nothing waits to be pasted."
                ),
                false,
                cx,
            );
        } else {
            self.set_status(Words::of_error(error), true, cx);
        }
    }

    /// An error whose remedy is a setting: the toast links to it.
    pub fn set_status_about(
        &mut self,
        text: impl Into<Words>,
        setting: &str,
        cx: &mut Context<Self>,
    ) {
        let text = text.into();
        tracing::warn!(text = %text.english(), "status");
        self.toast_about(text, Tone::Error, setting, cx);
    }

    /// Something worth knowing, not a failure, that a setting changes:
    /// the toast links to it.
    pub fn set_hint_about(
        &mut self,
        text: impl Into<Words>,
        setting: &str,
        cx: &mut Context<Self>,
    ) {
        self.toast_about(text.into(), Tone::Info, setting, cx);
    }

    fn toast_about(&mut self, text: Words, tone: Tone, setting: &str, cx: &mut Context<Self>) {
        self.toast(
            Toast::new(
                text,
                tone,
                vec![Action {
                    label: words!(
                        status_open_setting = "Open setting",
                        note = "A button on a toast whose remedy is a setting: opens Settings \
                                at it."
                    ),
                    command: "ui.settings".into(),
                    args: serde_json::json!({ "key": setting }),
                    answers: true,
                }],
            ),
            cx,
        );
    }
}

/// A pane footer's height.
pub const HEIGHT: f32 = 26.;

/// What the footer says of an answer an index gave (`Listing::rests_on`,
/// ADR-150), naming the index (amended: the pane's footer says more than
/// this), and its tooltip: `index up to date` where it is live, `index
/// checked Today 14:02`, `index as of Today 03:00`, then what answered and
/// what that leaves out. The moment is said as the list says its dates
/// (`format::friendly_arg`), the exact one as written; what answered as
/// its plugin said it. Nothing where no index answered.
fn rests_on_said(rests_on: &RestsOn, now: &jiff::ZonedDateTime) -> Option<(Words, Words)> {
    let index = || rests_on.by.clone();
    Some(match rests_on.proof {
        Proof::Live { .. } => (
            words!(
                status_live = "index up to date",
                note = "In a pane's footer, after its count, when an index that follows every \
                        change (skwird's) answered a search."
            ),
            words!(
                status_live_tip = "Found in { $index }, which follows every change: this \
                                   answer is the files as they are now.",
                index = index(),
                note = "The tooltip of a footer's \"index up to date\": index: what answered \
                        (skwird's index)."
            ),
        ),
        Proof::Checked { at } => (
            words!(
                status_checked = "index checked { $when }",
                when = format::friendly_arg(at, now),
                note = "In a pane's footer, after its count, when an index answered a search \
                        and had compared what it knew with the folders; when: the moment \
                        (Today 14:02)."
            ),
            words!(
                status_checked_tip = "Found in { $index }, which compared what it knew with \
                                      the folders { $when }.",
                index = index(),
                when = format::exact_time(at),
                note = "The tooltip of a footer's \"index checked\": index: what answered (skwird's \
                        index); when: the moment it compared."
            ),
        ),
        Proof::AsOf { at: Some(updated) } => (
            words!(
                status_as_of = "index as of { $when }",
                when = format::friendly_arg(updated, now),
                note = "In a pane's footer, after its count, when what answered a search (an \
                        index) was last brought up to date; when: the moment (Today 03:00)."
            ),
            words!(
                status_as_of_tip = "Found in { $index }, last brought up to date { $when }: \
                                    what was made, renamed or removed since is not in this \
                                    answer.",
                index = index(),
                when = format::exact_time(updated),
                note = "The tooltip of a footer's \"index as of\": index: what answered \
                        (plocate's database); when: the moment it was last brought up to date."
            ),
        ),
        Proof::AsOf { at: None } => (
            words!(
                status_as_of_index = "as of { $index }",
                index = index(),
                note = "In a pane's footer, after its count: what answered a search, an index \
                        whose last update cannot be told (plocate's database)."
            ),
            words!(
                status_as_of_index_tip = "Found in { $index }, whose last update cannot be \
                                          told: what was made, renamed or removed since is not \
                                          in this answer.",
                index = index(),
                note = "The tooltip of a footer's \"as of\": index: what answered (plocate's \
                        database)."
            ),
        ),
        // A walk answered there.
        Proof::NotCovered(_) => return None,
    })
}

/// The footer of the pane being rendered; `window` adds the window's own
/// state at its end (the last pane's), with what runs as this frame reads
/// it once (`Shell::under_way`).
pub(crate) fn render(
    shell: &mut Shell,
    window: Option<&crate::activity::Under>,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let theme = cx.global::<Theme>().clone();
    let selection = shell.selection_ctx();
    let (
        held,
        count,
        complete,
        error,
        is_query,
        sweep,
        total,
        rests_on,
        reads,
        left_out,
        looking,
        read,
    ) = {
        let ws = shell.app.workspace.read();
        let pane = ws.pane(shell.pane);
        pane.map_or(
            (
                None, 0, true, None, false, None, None, None, false, 0, false, None,
            ),
            |p| {
                (
                    p.listing.filling_count(),
                    // What it lists, a folded group's files counted in as
                    // its heading counts them (ADR-131, 2).
                    p.found_count().unwrap_or(shell.ui().list.listed.len()),
                    p.listing.is_complete(),
                    p.listing.error().cloned(),
                    shell.ui().place.search,
                    p.selection.sweeping(),
                    // Filtered (its lens, words typed): how many there are in all.
                    p.is_filtered()
                        .then(|| p.listing.order_shown(p.sorting()).len()),
                    p.listing.rests_on().cloned(),
                    p.read_filter().is_some_and(|r| r.metadata),
                    p.listing.left_out(),
                    p.looks_below(),
                    p.listing.reading(),
                )
            },
        )
    };
    // "Loading…" follows the same delayed signal as the pane banner.
    let loading = crate::loading::pending(shell, shell.pane);
    let loading_items = |count: usize| {
        words!(
            status_loading_items = "Loading… { $count ->
                [one] { $count } item
               *[other] { $count } items
            }",
            count = count,
            note = "A pane's footer while its folder is read: how many items were read so far."
        )
    };
    let items = match (is_query, complete) {
        // A folder filled elsewhere until it is whole (ADR-120) has shown
        // nothing: how many it has read, once "Loading…" is said, nothing
        // before.
        _ if held.is_some() => match held.filter(|_| loading) {
            None => Words::default(),
            Some(0) => words!(
                status_loading = "Loading…",
                note = "A pane's footer while its folder is read, before anything was."
            ),
            Some(read) => loading_items(read),
        },
        (true, false) => words!(
            status_searching = "Searching… { $count ->
                [one] { $count } result
               *[other] { $count } results
            }",
            count = count,
            note = "A pane's footer while a search runs: how many it found so far."
        ),
        (true, true) => words!(
            status_results = "{ $count ->
                [one] { $count } result
               *[other] { $count } results
            }",
            count = count,
            note = "A pane's footer once a search ended: how many it found."
        ),
        (false, false) if error.is_none() && loading => loading_items(count),
        (false, _) => match total {
            Some(total) => words!(
                status_shown_of = "{ $count ->
                    [one] { $count }
                   *[other] { $count }
                } of { $total ->
                    [one] { $total } item
                   *[other] { $total } items
                }",
                count = count,
                total = total,
                note = "A filtered pane's footer: how many items are shown, of how many there \
                        are."
            ),
            None => words!(
                status_items = "{ $count ->
                    [one] { $count } item
                   *[other] { $count } items
                }",
                count = count,
                note = "A pane's footer: how many items it lists."
            ),
        },
    };
    // Nothing selected: what the files shown weigh, as a selection's size
    // is counted (folders not measured), once a worker has summed them
    // (`weight.rs`); a view that measures folders says what they weigh.
    let weighs = shell.view_weighs();
    let listed = (selection.count == 0)
        .then(|| weighs.map_or_else(|| shell.listed_bytes(), |w| w.shown))
        .flatten();
    let items = match listed {
        Some(bytes) => words!(
            status_items_weigh = "{ $items } · { $size }",
            items = items,
            size = Size(bytes),
            note = "A pane's footer, nothing selected: how many items it lists (12 items), then \
                    what the files shown weigh."
        ),
        None => items,
    };
    // An order that reads what its entries lack is made once it is read
    // (ADR-121, amended): how far the reading went, past 200 ms.
    let items = match read.filter(|_| crate::loading::reading(shell, shell.pane)) {
        Some((read, asked)) => words!(
            status_reading_order = "{ $items } · Reading… { $read } of { $asked }",
            items = items,
            read = read,
            asked = asked,
            note = "A pane's footer while what its order sorts by is read: how many items it \
                    lists (12 items), then how many were read of all."
        ),
        None => items,
    };
    // A filter that reads what files' metadata says shows them as it
    // comes (ADR-107): how many are still read.
    let reading = shell.ui().quick_filters.reading(reads && !is_query);
    let items = match reading.filter(|_| total.is_some()) {
        Some(crate::quick_filters::Reading::Files(n)) => words!(
            status_reading_files = "{ $items } · Reading { $count ->
                [one] { $count } file
               *[other] { $count } files
            }…",
            items = items,
            count = n,
            note = "A pane's footer while a filter reads what files' metadata says: how many \
                    items it shows (3 of 12 items), then how many files are still read."
        ),
        Some(crate::quick_filters::Reading::Counting) => words!(
            status_reading = "{ $items } · Reading…",
            items = items,
            note = "A pane's footer while a filter reads what files' metadata says, before it \
                    knows how many: how many items it shows (3 of 12 items)."
        ),
        // The folders under a lens show as they are found.
        None if looking => words!(
            status_looking = "{ $items } · Looking in folders…",
            items = items,
            note = "A pane's footer while a filter looks in the folders below for what it \
                    matches: how many items it shows so far (3 of 12 items)."
        ),
        None => items,
    };
    // An index's answer says how far it is proven (ADR-050, ADR-150): as
    // of when, what was made, renamed or removed since not in it.
    let as_of = rests_on.and_then(|r| rests_on_said(&r, &skwir_core::locale::now()));
    // What a search walked past (ADR-125): said, never silently.
    let left_out = (left_out > 0).then(|| {
        words!(
            status_excluded = "{ $count ->
                [one] { $count } excluded
               *[other] { $count } excluded
            }",
            count = left_out,
            note = "In a search's footer: how many files and folders the user's exclusions left \
                    out."
        )
    });
    // What folded groups hold, counted in the items: said, as what a
    // search leaves out is.
    let folded = {
        let list = &shell.ui().list;
        list.listed.len().saturating_sub(list.sorted.len())
    };
    let folded = (folded > 0).then(|| {
        words!(
            status_folded = "{ $count ->
                [one] { $count } folded
               *[other] { $count } folded
            }",
            count = folded,
            note = "In a pane's footer: how many files its folded groups hold."
        )
    });
    // What it says is the status line a screen reader reads when asked,
    // never announced: it changes with every key.
    let mut said = shell.a11y.then(|| {
        as_of
            .iter()
            .map(|(text, _)| text)
            .chain(left_out.as_ref())
            .chain(folded.as_ref())
            .fold(items.clone(), |said, text| and_said(said, text.clone()))
    });
    let mut bar = div()
        .id(("pane-footer", shell.pane.0 as usize))
        .role(gpui::Role::Status)
        .flex_none()
        .h(sized(HEIGHT))
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .px_2()
        .text_body()
        // One line, all it holds: a text narrows to its widest word where
        // the row is short and would wrap in a bar one line tall, cut above
        // and below. What may give way is cut at its end, said whole when
        // pointed at.
        .whitespace_nowrap()
        // A card's foot (`Theme::cards`): a band as its row is, no line.
        .when(theme.lines(), |d| d.border_t_1().border_color(theme.border))
        .bg(theme.bar_ground(shell.lowered()))
        .text_color(theme.text_muted)
        .child(clipped(
            div().id("footer-items").min_w_0().truncate(),
            items.say(),
        ));
    if let Some((text, tip)) = as_of {
        let tip: gpui::SharedString = tip.say().into();
        bar = bar.child(
            div()
                .id("listing-as-of")
                .flex_none()
                .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
                .child(text.say()),
        );
    }
    if let Some(text) = left_out {
        bar = bar.child(
            div()
                .id("listing-left-out")
                .flex_none()
                .tooltip(|_, cx| {
                    crate::toolbar::tooltip(
                        words!(
                            status_excluded_tip = "Files and folders your exclusions leave out \
                                                   (Settings › Exclusions) were walked past: \
                                                   excluded:yes searches them too",
                            note = "The tooltip of a search footer's \"excluded\"; excluded:yes \
                                    is a term of the query language, as it is typed."
                        )
                        .say()
                        .into(),
                        cx,
                    )
                })
                .child(text.say()),
        );
    }
    if let Some(text) = folded {
        bar = bar.child(
            div()
                .id("listing-folded")
                .flex_none()
                .tooltip(|_, cx| {
                    crate::toolbar::tooltip(
                        words!(
                            status_folded_tip = "Files in folded groups: a heading's chevron, \
                                                 or Right on it, opens its group again",
                            note = "The tooltip of a footer's \"folded\"; Right is the arrow key."
                        )
                        .say()
                        .into(),
                        cx,
                    )
                })
                .child(text.say()),
        );
    }
    if selection.count > 0 {
        // A part of the size is never said as the whole: until every
        // selected file is stat-ed (`sync_selection_hydration`), none.
        let bytes = weighs.map_or(selection.bytes.unwrap_or(0), |w| w.selected.unwrap_or(0));
        let text = if bytes > 0 {
            words!(
                status_selected_weigh = "{ $count ->
                    [one] { $count } selected
                   *[other] { $count } selected
                } · { $size }",
                count = selection.count,
                size = Size(bytes),
                note = "In a pane's footer: how many items are selected, then what they weigh."
            )
        } else {
            words!(
                status_selected = "{ $count ->
                    [one] { $count } selected
                   *[other] { $count } selected
                }",
                count = selection.count,
                note = "In a pane's footer: how many items are selected."
            )
        };
        if let Some(said) = &mut said {
            *said = and_said(said.clone(), text.clone());
        }
        let text = text.say();
        bar = bar.child(clipped(
            div()
                .id("footer-selected")
                .min_w_0()
                .truncate()
                .text_color(theme.text),
            text,
        ));
    }
    // A sweep is a mode: the keys act differently while it lasts.
    if let Some(unset) = sweep {
        let keys = shell.keys_for("select_sweep_end", &serde_json::json!({}));
        let text = match (unset, keys) {
            (false, Some(keys)) => words!(
                status_sweep_selecting_keys = "Selecting as the cursor moves · { $keys } to stop",
                keys = skwir_core::words::Keys(keys),
                note = "In a pane's footer while a sweep selects what the cursor passes; keys: \
                        the keys that end it (Escape)."
            ),
            (false, None) => words!(
                status_sweep_selecting = "Selecting as the cursor moves",
                note = "In a pane's footer while a sweep selects what the cursor passes."
            ),
            (true, Some(keys)) => words!(
                status_sweep_deselecting_keys = "Deselecting as the cursor moves · { $keys } to \
                                                 stop",
                keys = skwir_core::words::Keys(keys),
                note = "In a pane's footer while a sweep deselects what the cursor passes; keys: \
                        the keys that end it (Escape)."
            ),
            (true, None) => words!(
                status_sweep_deselecting = "Deselecting as the cursor moves",
                note = "In a pane's footer while a sweep deselects what the cursor passes."
            ),
        };
        if let Some(said) = &mut said {
            *said = and_said(said.clone(), text.clone());
        }
        let text = text.say();
        bar = bar.child(
            div()
                .flex_none()
                .px_1()
                .rounded_sm()
                .chosen(&theme)
                .child(text),
        );
    }
    // A listing's error is a message, not state: the banner says it beside
    // the entries (`loading::message`), the pane itself with none.
    bar = bar
        .when_some(said, |d, said| d.aria_label(said.say()))
        .child(div().flex_1());
    let Some(under) = window else {
        return bar.into_any_element();
    };
    bar = bar.children(crate::player::render_status(shell, &theme, cx));
    // The clipboard is invisible once the pane shows another folder: say
    // what is waiting, and let a click forget it.
    let waiting = shell.pending.len();
    if waiting > 0 {
        let (said, icon) = if shell.pending.cut {
            (
                words!(
                    status_cut = "{ $count ->
                        [one] { $count } item cut
                       *[other] { $count } items cut
                    }",
                    count = waiting,
                    note = "In the window's footer: how many items wait to be moved by a paste."
                ),
                "cut",
            )
        } else {
            (
                words!(
                    status_copied = "{ $count ->
                        [one] { $count } item copied
                       *[other] { $count } items copied
                    }",
                    count = waiting,
                    note = "In the window's footer: how many items wait to be copied by a paste."
                ),
                "copy",
            )
        };
        let tip = match shell.keys_for("clipboard_paste", &serde_json::Value::Null) {
            Some(keys) => words!(
                status_pending_keys_tip = "Paste with { $keys }. Click to forget.",
                keys = skwir_core::words::Keys(keys),
                note = "The tooltip of the footer's items waiting for a paste; keys: the keys \
                        that paste (Ctrl+V)."
            ),
            None => words!(
                status_pending_tip = "Click to forget.",
                note = "The tooltip of the footer's items waiting for a paste."
            ),
        };
        let tip: gpui::SharedString = tip.say().into();
        bar = bar.child(
            div()
                .id("status-pending")
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .px_2()
                .rounded_md()
                .cursor_pointer()
                .text_color(theme.text)
                .hover(|s| s.bg(theme.hover))
                .tooltip(move |_, cx| crate::toolbar::tooltip(tip.clone(), cx))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.run("clipboard_clear", serde_json::json!({}), window, cx);
                }))
                .child(crate::controls::glyph(icon, theme.text_muted, 13.))
                .child(said.say()),
        );
    }
    if !shell.reviews.is_empty() {
        let questions = shell.reviews.iter().filter(|r| r.is_question()).count();
        let plans = shell.reviews.len() - questions;
        let words = match (plans, questions) {
            (plans, 0) => words!(
                status_proposals = "{ $plans ->
                    [one] { $plans } proposal to review
                   *[other] { $plans } proposals to review
                }",
                plans = plans,
                note = "In the window's footer: how many plans wait to be reviewed."
            ),
            (0, questions) => words!(
                status_questions = "{ $questions ->
                    [one] { $questions } question
                   *[other] { $questions } questions
                }",
                questions = questions,
                note = "In the window's footer: how many questions wait for an answer."
            ),
            (plans, questions) => words!(
                status_proposals_questions = "{ $plans ->
                    [one] { $plans } proposal
                   *[other] { $plans } proposals
                } to review · { $questions ->
                    [one] { $questions } question
                   *[other] { $questions } questions
                }",
                plans = plans,
                questions = questions,
                note = "In the window's footer: how many plans wait to be reviewed, then how \
                        many questions wait for an answer."
            ),
        };
        bar = bar.child(clipped(
            div()
                .id("footer-reviews")
                .min_w_0()
                .truncate()
                .text_color(theme.accent_text),
            words.say(),
        ));
    }
    // What a plugin counts (the shelf's items) announces itself only
    // when it counts something, in its words; a click opens what it counts.
    for (name, tally) in &shell.tallies {
        let open = tally.open.clone();
        let said = words!(
            status_tally = "{ $what }: { $count }",
            what = tally.label.clone(),
            count = tally.count,
            note = "In the window's footer: what a plugin counts (Shelf), and how many."
        )
        .say();
        let tallied = div()
            // Known by what it counts: one that comes or goes moves the
            // others.
            .id(crate::elements::known_by("tally", name))
            .role(gpui::Role::Button)
            .when(shell.a11y, |d| d.aria_label(said.clone()))
            .px_2()
            .rounded_md()
            .when(open.is_some(), gpui::Styled::cursor_pointer)
            .chosen(&theme)
            .hover(|s| s.bg(theme.hover))
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some(open) = &open {
                    this.run(&open.name, open.args.clone(), window, cx);
                }
            }));
        bar = bar.child(clipped(tallied.min_w_0().truncate(), said));
    }
    // What runs, docked: its strip, and Activity opened above it (ADR-114).
    if let Some(strip) = crate::activity::strip(shell, under, &theme, cx) {
        bar = bar.child(strip);
    }
    // What Undo takes: the last thing asked, once it ends (ADR-079); its
    // chevron, the History (ADR-079, 13).
    if let Some(undo) = crate::history::undo_control(shell, &theme, cx) {
        bar = bar.child(undo);
    }
    bar.into_any_element()
}

/// What a screen reader hears of a footer: what it said so far, then one
/// more of its facts.
fn and_said(said: Words, fact: Words) -> Words {
    words!(
        status_and = "{ $said }, { $fact }",
        said = said,
        fact = fact,
        note = "What a screen reader hears of a pane's footer, fact after fact: what it said \
                so far (12 items), then the next (3 selected)."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_index_s_answer_says_how_far_it_is_proven() {
        // The zone UTC: what the English below writes. The moment is an
        // argument of the words, said in English's own formats (a 12-hour
        // time), whatever the window's locale.
        skwir_core::locale::set_zone(Some(jiff::tz::TimeZone::UTC));
        let now = jiff::Timestamp::from_second(1_800_000_007)
            .unwrap()
            .to_zoned_datetime(jiff::tz::TimeZone::UTC);
        let an_hour_before = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_secs(1_800_000_007 - 3600);
        let said = |by: &'static str, proof| {
            let (text, tip) = rests_on_said(
                &RestsOn {
                    by: Words::as_written(by),
                    proof,
                },
                &now,
            )
            .expect("an index answered");
            (text.english(), tip.english())
        };

        let (text, tip) = said(
            "plocate's database",
            Proof::AsOf {
                at: Some(an_hour_before),
            },
        );
        assert_eq!(text, "index as of Today 7:00\u{202f}AM");
        assert!(tip.starts_with("Found in plocate's database, last brought up to date "));
        assert!(tip.ends_with("is not in this answer."), "{tip}");

        let (text, tip) = said("plocate's database", Proof::AsOf { at: None });
        assert_eq!(text, "as of plocate's database");
        assert!(tip.contains("whose last update cannot be told"), "{tip}");

        let (text, tip) = said("skwird's index", Proof::Checked { at: an_hour_before });
        assert_eq!(text, "index checked Today 7:00\u{202f}AM");
        assert!(
            tip.starts_with("Found in skwird's index, which compared"),
            "{tip}"
        );

        let (text, tip) = said(
            "skwird's index",
            Proof::Live {
                since: an_hour_before,
            },
        );
        assert_eq!(text, "index up to date");
        assert!(tip.ends_with("the files as they are now."), "{tip}");

        assert!(
            rests_on_said(
                &RestsOn {
                    by: Words::as_written("skwird's index"),
                    proof: Proof::NotCovered(skwir_core::coverage::Why::Denied),
                },
                &now,
            )
            .is_none(),
            "a walk answered: nothing said"
        );
    }
}
