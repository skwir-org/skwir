//! The address field as the one place to type: a path, a search, a place
//! to go to, or a command.
//!
//! Ctrl+F (`ui.search`) starts a search: the field shows where it will
//! look as a pill before the text (the folder shown, or the root of the
//! search shown, whose text is then edited). Ctrl+L (`ui.focus_address`)
//! edits the path. Either way, once the text is edited, a dropdown under
//! the field says what Enter will do and offers the rest:
//!
//! - a path lists the names of the folder typed that go on with it
//!   (`complete.rs`, ADR-070): folders, then files, the best first, what
//!   it adds drawn after the text; a place's text (`tags:/Places/Fr`)
//!   those its provider completes it with (`LocationProvider::complete`);
//! - words search where the pill says (Enter) or the whole home folder
//!   (Alt+Enter), look for files containing them, or go to a place Jump
//!   knows (`jump.candidates`, asked on a worker) or a scheme they start;
//! - `>` hands the rest to the command palette.
//!
//! Tab completes (`ui.omnibox_complete`): the row chosen with the arrows,
//! else the part the names typed share, else the row highlighted, a
//! folder then followed by its separator and listed in turn; words that
//! leave one name shown become its path. Right or End at the end takes
//! what is drawn after the text. Enter goes to a folder and shows a file
//! in its folder; Shift+Enter opens the file.
//!
//! The UI knows no query syntax: a search is `query.search` with the text
//! as `expr` and the pill's folder as `root`, and files containing the text
//! are the plugin's own refinement (`query.refine`, facet `content`). The
//! keyboard never leaves the field: Up and Down move through the dropdown
//! (bound in the address bar's context), a click on a row runs it without
//! taking the focus.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::{Completions, Location, PATH_SCHEME};
use skwir_core::os_path::PathArg;

use crate::complete::{self, Resolved};
use crate::elements::popover::PopoverAnchor;
use crate::shell::Shell;
use crate::theme::Theme;

/// The most places Jump adds under the searches.
const PLACES: usize = 5;

/// The folders read for completion kept while the field is typed in.
const FOLDERS_KEPT: usize = 16;

/// How long a folder is read before the dropdown says so.
const READING_SAID: Duration = Duration::from_millis(200);

/// A pane's field while it is being typed in.
#[derive(Default)]
pub struct Omni {
    /// Where a search looks, drawn as a pill before the text; `None` while
    /// a path is edited (the folder shown is then where words search).
    pub scope: Option<PathBuf>,
    /// The text changed since the field took the keyboard: until then no
    /// dropdown hangs under it.
    edited: bool,
    highlighted: usize,
    /// The highlight was moved (Up, Down, Tab): Tab takes that row.
    chosen: bool,
    /// Jump's places for `asked`.
    places: Vec<crate::jump::Candidate>,
    asked: Option<String>,
    /// The rows for the text, first what Enter runs (`refresh_rows`).
    rows: Vec<Suggestion>,
    /// What Tab fills in: the text with the part the names typed share.
    shared: Option<String>,
    /// Words that leave one name shown: its path as text, and the path.
    one_shown: Option<(String, PathBuf)>,
    /// What the folder parts typed stand for, by the folder each makes;
    /// `None` while it is read.
    folders: HashMap<PathBuf, Option<Arc<Resolved>>>,
    /// The folder being read, since when, and what stops it.
    reading: Option<(PathBuf, Instant, CancelToken)>,
    /// A provider's completions and the text they were asked for; the
    /// text being asked, and what stops it.
    offered: Option<(String, Option<Completions>)>,
    offering: Option<(String, CancelToken)>,
    /// A completion taken, as the text it made and the path it stands
    /// for: a name that is not UTF-8 is written approximately and kept
    /// exactly here.
    taken: Option<(String, PathBuf)>,
}

impl Drop for Omni {
    fn drop(&mut self) {
        if let Some((.., cancel)) = &self.reading {
            cancel.cancel();
        }
        if let Some((_, cancel)) = &self.offering {
            cancel.cancel();
        }
    }
}

/// What a row of the dropdown does.
#[derive(Clone, Debug, PartialEq)]
enum Suggestion {
    /// Go to the location the text names: a folder, a file shown in its
    /// folder.
    Go,
    /// Search for the text where the pill says, or everywhere at home.
    Search { everywhere: bool },
    /// Files whose contents hold the text, where the pill says.
    Contains,
    /// A place Jump knows, and whether Jump said it is a folder.
    Place { path: PathBuf, dir: bool },
    /// A name of the folder typed, or of one its names start (`folder`
    /// then says which): Enter goes there or shows the file, Tab writes
    /// `text`, and `rest` is what it adds after the text typed.
    Name {
        path: PathBuf,
        dir: bool,
        label: String,
        folder: Option<String>,
        text: String,
        rest: Option<String>,
    },
    /// A place a provider's text goes on with, or a scheme words start:
    /// Enter goes there, Tab writes `text`.
    Offered {
        location: Location,
        label: String,
        text: String,
        rest: Option<String>,
    },
}

impl Suggestion {
    /// What Tab writes, for a completion.
    fn completion(&self) -> Option<&str> {
        match self {
            Suggestion::Name { text, .. } | Suggestion::Offered { text, .. } => Some(text),
            _ => None,
        }
    }

    /// What the row adds after the text typed, drawn there.
    fn rest(&self) -> Option<&str> {
        match self {
            Suggestion::Name { rest, .. } | Suggestion::Offered { rest, .. } => rest.as_deref(),
            _ => None,
        }
    }
}

/// The scheme `text` starts with (`scheme:rest`), whether or not a
/// provider has it.
fn scheme_of(text: &str) -> Option<&str> {
    let (scheme, _) = text.trim_start().split_once(':')?;
    (!scheme.is_empty()
        && scheme
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
    .then_some(scheme)
}

/// Words that may start a scheme's name (`tr` for `trash:`): one lowercase
/// word, two letters at least.
fn scheme_start(text: &str) -> Option<&str> {
    (text.len() >= 2 && text.chars().all(|c| c.is_ascii_lowercase() || c == '-')).then_some(text)
}

fn home() -> Option<PathBuf> {
    skwir_core::os_path::home()
}

/// A folder as the pill and the rows say it: as everything names it
/// (`App::location_title`), the sidebar's name for one it shows.
fn folder_name(app: &skwir_core::App, path: &Path) -> String {
    app.location_title(&skwir_core::Location::from_path(path))
}

impl Shell {
    /// Whether `text` names a location rather than words to look for: a
    /// path, or a scheme a provider opens (`query:`, `shelf:`; `kind:` is
    /// a word of the query language).
    fn names_location(&self, text: &str) -> bool {
        complete::is_path(text) || scheme_of(text).is_some_and(|s| self.app.provider(s).is_some())
    }

    /// Give the active pane's field the keyboard: `search` (Ctrl+F) with
    /// where it looks as a pill and `text` to start from, else the path
    /// (Ctrl+L). A pane showing a search edits that search either way.
    pub fn omnibox_begin(
        &mut self,
        search: bool,
        text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane;
        let chips = &self.ui().chips;
        let (scope, initial, select) = if !chips.chips.is_empty() {
            let given = text.is_some();
            (
                Some(chips.root.clone()),
                text.unwrap_or_else(|| chips.text.clone()),
                !given,
            )
        } else if search {
            (
                self.app.pane_dir(pane).ok(),
                text.unwrap_or_default(),
                false,
            )
        } else {
            (None, self.ui().address_shown.clone(), true)
        };
        let placeholder = match &scope {
            Some(root) => format!("Search in {}", folder_name(&self.app, root)),
            None => "A path, or words to search".to_owned(),
        };
        let mut omni = Omni::default();
        omni.scope = scope;
        self.ui_mut().omni = Some(omni);
        let address = self.ui().address.clone();
        address.update(cx, |input, cx| {
            input.set_placeholder(placeholder, cx);
            input.set_text(initial, cx);
            input.set_hint(None, cx);
            if select {
                input.select_all_now(cx);
            }
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// The field's text changed: the dropdown follows, the folder typed is
    /// read, Jump is asked for places, and `>` goes to the palette.
    pub fn omnibox_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.ui().address.read(cx).text().to_owned();
        if let Some(rest) = text.strip_prefix('>') {
            let rest = rest.trim_start().to_owned();
            self.omnibox_cancel(window, cx);
            self.open_palette(window, cx);
            if let Some(palette) = &self.palette {
                palette
                    .input
                    .update(cx, |input, cx| input.set_text(rest, cx));
            }
            return;
        }
        let words = text.trim().to_owned();
        let no_places = words.is_empty()
            || self.names_location(&words)
            || self.app.commands.get("jump.candidates").is_none();
        self.filter_as_typed(&words);
        let omni = self.ui_mut().omni.get_or_insert_with(Omni::default);
        omni.edited = true;
        omni.highlighted = 0;
        omni.chosen = false;
        if no_places {
            omni.places.clear();
            omni.asked = None;
        } else if omni.asked.as_deref() != Some(words.as_str()) {
            omni.asked = Some(words.clone());
            let pane = self.pane;
            let app = std::sync::Arc::clone(&self.app);
            let args = serde_json::json!({ "query": words, "limit": PLACES });
            self.off_thread(
                "omnibox-places",
                move || app.dispatch(&Invocation::with_args("jump.candidates", &args)),
                move |shell, result, _, cx| {
                    let Some(omni) = shell
                        .pane_ui_mut(pane)
                        .and_then(|ui| ui.omni.as_mut())
                        .filter(|o| o.asked.as_deref() == Some(words.as_str()))
                    else {
                        return;
                    };
                    omni.places = match result {
                        Ok(Outcome::Value(value)) => {
                            serde_json::from_value(value).unwrap_or_default()
                        }
                        _ => Vec::new(),
                    };
                    omni.places.truncate(PLACES);
                    shell.with_pane(pane, |shell| shell.refresh_rows(cx));
                },
                window,
                cx,
            );
        }
        self.ask_completions(text.trim_start(), window, cx);
        self.refresh_rows(cx);
    }

    /// Words typed over a folder (not a search: its text is the query)
    /// show only the names that hold them, at once, in memory (the core's
    /// `filter`); Enter then searches the folders inside too.
    fn filter_as_typed(&mut self, words: &str) {
        let pane = self.pane;
        let filters = self
            .app
            .pane_location(pane)
            .is_some_and(|l| !self.app.is_search(&l));
        if !filters {
            return;
        }
        let wanted = (!words.is_empty() && !self.names_location(words)).then_some(words);
        let current = self
            .app
            .workspace
            .read()
            .pane(pane)
            .and_then(|p| p.filter().map(str::to_owned));
        if current.as_deref() != wanted {
            self.set_filter(wanted);
        }
    }

    /// Show only the names of the active pane that hold `text`, or all.
    /// Dispatched directly: what is typed is not traced.
    pub(crate) fn set_filter(&mut self, text: Option<&str>) {
        let pane = self.pane;
        if let Err(error) = self.app.dispatch(&Invocation::with_args(
            "filter",
            &serde_json::json!({ "text": text, "pane": pane }),
        )) {
            tracing::warn!("filter: {error:#}");
        }
    }

    /// The folder a path's folder part typed makes: from where a
    /// completion taken said, else as `complete::resolve` reads it.
    fn typed_folder(&self, dir: &str) -> Option<PathBuf> {
        let omni = self.ui().omni.as_ref();
        let home = home();
        if let Some((taken, path)) = omni.and_then(|o| o.taken.as_ref())
            && let Some(rest) = dir.strip_prefix(taken.as_str())
            && (rest.is_empty() || taken.ends_with(complete::is_separator))
        {
            return complete::resolve(rest, Some(path), home.as_deref());
        }
        let base = self.app.pane_dir(self.pane).ok();
        complete::resolve(dir, base.as_deref(), home.as_deref())
    }

    /// Whether the active pane shows hidden files.
    fn shows_hidden(&self) -> bool {
        self.app
            .workspace
            .read()
            .pane(self.pane)
            .is_some_and(|p| p.sort.show_hidden)
    }

    /// Read what completes `text`, unless it is at hand: the folder a
    /// path's folder part makes, on a worker (a disk may be asleep), the
    /// one read before stopped; or a provider's completions of its text.
    fn ask_completions(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.pane;
        if complete::is_path(text) {
            if text == "~" {
                return;
            }
            let (dir, _) = complete::split(text);
            let Some(folder) = self.typed_folder(dir) else {
                return;
            };
            let show_hidden = self.shows_hidden();
            let cancel = self.app.lifetime().child();
            let Some(omni) = self.ui_mut().omni.as_mut() else {
                return;
            };
            if omni.folders.contains_key(&folder) {
                return;
            }
            if let Some((before, _, cancel)) = omni.reading.take() {
                cancel.cancel();
                omni.folders.remove(&before);
            }
            if omni.folders.len() >= FOLDERS_KEPT {
                omni.folders.clear();
            }
            omni.folders.insert(folder.clone(), None);
            omni.reading = Some((folder.clone(), Instant::now(), cancel.clone()));
            let app = Arc::clone(&self.app);
            let asked = folder.clone();
            let stop = cancel.clone();
            self.off_thread_until(
                "address-complete",
                cancel,
                move || complete::folders(&app, &asked, show_hidden, &stop),
                move |shell, resolved, _, cx| {
                    let Some(omni) = shell.pane_ui_mut(pane).and_then(|ui| ui.omni.as_mut()) else {
                        return;
                    };
                    // Forgotten for another folder meanwhile.
                    if !omni.folders.contains_key(&folder) {
                        return;
                    }
                    if omni.reading.as_ref().is_some_and(|(r, ..)| *r == folder) {
                        omni.reading = None;
                    }
                    omni.folders.insert(folder, Some(Arc::new(resolved)));
                    shell.with_pane(pane, |shell| shell.refresh_rows(cx));
                },
                window,
                cx,
            );
            // A folder still being read is said, once it takes a moment.
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(READING_SAID).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
        } else if let Some(scheme) = scheme_of(text)
            && let Some(provider) = self.app.provider(scheme)
        {
            let rest = text.trim_start()[scheme.len() + 1..].to_owned();
            let cancel = self.app.lifetime().child();
            let Some(omni) = self.ui_mut().omni.as_mut() else {
                return;
            };
            if omni.offered.as_ref().is_some_and(|(t, _)| t == text)
                || omni.offering.as_ref().is_some_and(|(t, _)| t == text)
            {
                return;
            }
            if let Some((_, before)) = omni.offering.take() {
                before.cancel();
            }
            omni.offering = Some((text.to_owned(), cancel.clone()));
            let asked = text.to_owned();
            self.off_thread_until(
                "address-complete-place",
                cancel,
                move || provider.complete(&rest),
                move |shell, completions, _, cx| {
                    let Some(omni) = shell.pane_ui_mut(pane).and_then(|ui| ui.omni.as_mut()) else {
                        return;
                    };
                    if omni.offering.as_ref().is_none_or(|(t, _)| *t != asked) {
                        return;
                    }
                    omni.offering = None;
                    omni.offered = Some((asked, completions));
                    shell.with_pane(pane, |shell| shell.refresh_rows(cx));
                },
                window,
                cx,
            );
        }
    }

    /// Work out the dropdown's rows for the text as it is, what Tab fills
    /// in, and what is drawn after the text; the field is asked to draw.
    fn refresh_rows(&mut self, cx: &mut Context<Self>) {
        let text = self.ui().address.read(cx).text().to_owned();
        let (rows, shared) = self.omni_rows(&text);
        let one_shown = self.one_shown(text.trim());
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return;
        };
        omni.rows = rows;
        omni.shared = shared;
        omni.one_shown = one_shown;
        omni.highlighted = omni.highlighted.min(omni.rows.len().saturating_sub(1));
        self.show_rest(cx);
        cx.notify();
    }

    /// Draw after the text what the highlighted row adds to it.
    fn show_rest(&mut self, cx: &mut Context<Self>) {
        let rest = self
            .ui()
            .omni
            .as_ref()
            .filter(|o| o.edited)
            .and_then(|o| o.rows.get(o.highlighted))
            .and_then(Suggestion::rest)
            .map(str::to_owned);
        let address = self.ui().address.clone();
        address.update(cx, |input, cx| input.set_hint(rest, cx));
    }

    /// Words over a folder that leave one name shown: that name's path as
    /// text (from `~` under the home folder), and the path itself.
    fn one_shown(&self, words: &str) -> Option<(String, PathBuf)> {
        if words.is_empty() || self.names_location(words) || !self.ui().chips.chips.is_empty() {
            return None;
        }
        let (name, dir, folder) = {
            let ws = self.app.workspace.read();
            let pane = ws.pane(self.pane)?;
            pane.filter()?;
            let order = pane.order();
            let [id] = order.as_ref() else {
                return None;
            };
            let entry = pane.listing.get(*id)?;
            (entry.name.clone(), entry.is_dir(), pane.location()?.path()?)
        };
        let path = folder.join(name);
        let mut text = complete::written(&path, "~", home().as_deref());
        if dir {
            text.push(std::path::MAIN_SEPARATOR);
        }
        Some((text, path))
    }

    /// Up or Down in the dropdown (`ui.list_move`).
    pub fn omnibox_move(&mut self, mv: crate::modal::ListMove, cx: &mut Context<Self>) {
        let count = self.suggestions().len();
        if let Some(omni) = self.ui_mut().omni.as_mut()
            && count > 0
        {
            omni.highlighted = mv.from(omni.highlighted, count);
            omni.chosen = true;
            self.show_rest(cx);
            cx.notify();
        }
    }

    /// Tab (`ui.omnibox_complete`): a row chosen with the arrows is taken;
    /// else words that leave one name shown become its path; else the part
    /// the names typed share is filled in, else the highlighted completion
    /// is taken, else the first is highlighted. A completion taken writes
    /// the text it makes, as an edit Ctrl+Z takes back: a folder ends with
    /// its separator, and its names are listed in turn.
    pub fn omnibox_complete(&mut self, cx: &mut Context<Self>) {
        let text = self.ui().address.read(cx).text().to_owned();
        let Some(omni) = self.ui().omni.as_ref() else {
            return;
        };
        let row = omni.rows.get(omni.highlighted);
        // A row chosen with the arrows is what is meant.
        if let Some(row) = row.filter(|r| omni.chosen && r.completion().is_some()) {
            let row = row.clone();
            return self.take_row(&row, cx);
        }
        if let Some((written, path)) = omni.one_shown.clone() {
            return self.take_completion(written, Some(path), cx);
        }
        let completions: Vec<usize> = (0..omni.rows.len())
            .filter(|i| omni.rows[*i].completion().is_some())
            .collect();
        let trimmed = text.trim_start();
        if completions.is_empty() && matches!(trimmed, "~" | "." | "..") {
            let written = format!("{trimmed}{}", std::path::MAIN_SEPARATOR);
            return self.take_completion(written, None, cx);
        }
        if let Some(row) = row.filter(|r| r.completion().is_some()) {
            let row = row.clone();
            match omni.shared.clone() {
                Some(shared) => self.take_completion(shared, None, cx),
                None => self.take_row(&row, cx),
            }
            return;
        }
        match completions.as_slice() {
            [] => {}
            [one] => {
                let row = omni.rows[*one].clone();
                self.take_row(&row, cx);
            }
            [first, ..] => {
                if let Some(shared) = omni.shared.clone() {
                    self.take_completion(shared, None, cx);
                } else {
                    let first = *first;
                    if let Some(omni) = self.ui_mut().omni.as_mut() {
                        omni.highlighted = first;
                        omni.chosen = true;
                    }
                    self.show_rest(cx);
                    cx.notify();
                }
            }
        }
    }

    /// Right or End at the end of the text: take what is drawn after it,
    /// the highlighted row's completion.
    pub fn omnibox_take_rest(&mut self, cx: &mut Context<Self>) {
        let row = self
            .ui()
            .omni
            .as_ref()
            .and_then(|o| o.rows.get(o.highlighted))
            .filter(|r| r.rest().is_some())
            .cloned();
        if let Some(row) = row {
            self.take_row(&row, cx);
        }
    }

    /// Write the completion `row` makes.
    fn take_row(&mut self, row: &Suggestion, cx: &mut Context<Self>) {
        match row {
            Suggestion::Name { text, path, .. } => {
                self.take_completion(text.clone(), Some(path.clone()), cx);
            }
            Suggestion::Offered { text, .. } => self.take_completion(text.clone(), None, cx),
            _ => {}
        }
    }

    /// Write `text`, a completion, as an edit; `path`, what it names,
    /// kept exactly.
    fn take_completion(&mut self, text: String, path: Option<PathBuf>, cx: &mut Context<Self>) {
        if let Some(omni) = self.ui_mut().omni.as_mut() {
            omni.taken = path.map(|path| (text.clone(), path));
        }
        let address = self.ui().address.clone();
        address.update(cx, |input, cx| input.replace_all(&text, cx));
    }

    /// Enter (the row highlighted), Alt+Enter (`everywhere`: a search of
    /// the home folder) or Shift+Enter (`open`: a file named is opened, not
    /// shown in its folder). Text not edited does what its first row
    /// would: the location shown goes there again, a search's words search
    /// again, a filter's words search; never words taken for a path.
    pub fn omnibox_submit(
        &mut self,
        everywhere: bool,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw = self.ui().address.read(cx).text().to_owned();
        let text = raw.trim().to_owned();
        let suggestions = if text == self.ui().address_shown.trim() {
            vec![Suggestion::Go]
        } else {
            self.omni_rows(&raw).0
        };
        let chosen = if everywhere {
            suggestions
                .iter()
                .find(|s| matches!(s, Suggestion::Search { .. }))
                .map(|_| Suggestion::Search { everywhere: true })
        } else {
            let at = self.ui().omni.as_ref().map_or(0, |o| o.highlighted);
            suggestions.get(at).cloned()
        };
        let scope = self.omni_root();
        let pane = self.pane;
        let named = complete::is_path(&text)
            .then(|| self.typed_folder(raw.trim_start()))
            .flatten();
        self.omnibox_end(cx);
        window.focus(&self.ui().focus, cx);
        match chosen {
            _ if text.is_empty() => {
                self.set_filter(None);
                let shown = self.ui().address_shown.clone();
                let address = self.ui().address.clone();
                address.update(cx, |input, cx| input.set_text(shown, cx));
            }
            None | Some(Suggestion::Go) => match named {
                Some(path) if open => self.open_path(pane, path, window, cx),
                Some(path) => {
                    self.reveal_path(pane, &path, window, cx);
                }
                None => self.run(
                    "navigate",
                    serde_json::json!({ "location": text, "pane": pane }),
                    window,
                    cx,
                ),
            },
            Some(Suggestion::Name {
                path, dir: true, ..
            }) => {
                self.run(
                    "navigate",
                    serde_json::json!({ "location": PathArg::from(path), "pane": pane }),
                    window,
                    cx,
                );
            }
            Some(
                Suggestion::Name {
                    path, dir: false, ..
                }
                | Suggestion::Place { path, dir: false },
            ) if open => self.open_path(pane, path, window, cx),
            Some(Suggestion::Name { path, .. } | Suggestion::Place { path, .. }) => {
                self.reveal_path(pane, &path, window, cx);
            }
            Some(Suggestion::Offered { location, .. }) => {
                self.run(
                    "navigate",
                    serde_json::json!({ "location": location.arg(), "pane": pane }),
                    window,
                    cx,
                );
            }
            Some(Suggestion::Search { everywhere }) => {
                let root = if everywhere { home() } else { scope };
                self.run(
                    "query.search",
                    serde_json::json!({
                        "expr": text,
                        "root": root.as_deref().map(PathArg::from),
                        "pane": pane,
                    }),
                    window,
                    cx,
                );
            }
            Some(Suggestion::Contains) => {
                self.run(
                    "query.refine",
                    serde_json::json!({ "facet": "content", "value": text, "pane": pane }),
                    window,
                    cx,
                );
            }
        }
        cx.notify();
    }

    /// Shift+Enter on a path: a folder is gone to, a file opened as Enter
    /// opens it in the files (`open`). Whether it is a folder, or there,
    /// is asked on a worker.
    fn open_path(
        &mut self,
        pane: skwir_core::workspace::PaneId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked = path.clone();
        self.off_thread(
            "address-open",
            move || std::fs::metadata(&asked).map(|m| m.is_dir()),
            move |shell, is_dir, window, cx| match is_dir {
                Ok(true) => shell.run(
                    "navigate",
                    serde_json::json!({ "location": PathArg::from(path), "pane": pane }),
                    window,
                    cx,
                ),
                Ok(false) => shell.run(
                    "open",
                    serde_json::json!({ "entries": [PathArg::from(path)], "pane": pane }),
                    window,
                    cx,
                ),
                Err(_) => {
                    let name = path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    shell.set_status(format!("“{name}” is not there"), true, cx);
                }
            },
            window,
            cx,
        );
    }

    /// Escape: the field shows where the pane is again, all of it.
    pub fn omnibox_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.omnibox_end(cx);
        self.set_filter(None);
        let shown = self.ui().address_shown.clone();
        let address = self.ui().address.clone();
        address.update(cx, |input, cx| input.set_text(shown, cx));
        window.focus(&self.ui().focus, cx);
        cx.notify();
    }

    /// The field lets go: what was being read for it stops, nothing more
    /// is drawn after its text.
    fn omnibox_end(&mut self, cx: &mut Context<Self>) {
        self.ui_mut().omni = None;
        let address = self.ui().address.clone();
        address.update(cx, |input, cx| input.set_hint(None, cx));
    }

    /// Where the active pane's words search: the pill's folder, else the
    /// folder shown.
    fn omni_root(&self) -> Option<PathBuf> {
        self.ui()
            .omni
            .as_ref()
            .and_then(|o| o.scope.clone())
            .or_else(|| self.app.pane_dir(self.pane).ok())
    }

    /// The dropdown's rows for the active pane's text, first the one Enter
    /// runs. Nothing shown until the text is edited.
    fn suggestions(&self) -> &[Suggestion] {
        self.ui()
            .omni
            .as_ref()
            .filter(|o| o.edited)
            .map_or(&[], |o| o.rows.as_slice())
    }

    /// Whether Tab completes the active pane's text (the `completes` key
    /// context): a completion is offered, or words leave one name shown.
    pub fn omnibox_completes(&self, cx: &gpui::App) -> bool {
        let Some(omni) = self.ui().omni.as_ref().filter(|o| o.edited) else {
            return false;
        };
        omni.one_shown.is_some()
            || omni.rows.iter().any(|r| r.completion().is_some())
            || matches!(
                self.ui().address.read(cx).text().trim_start(),
                "~" | "." | ".."
            )
    }

    /// What `text` offers, first what Enter runs, and what Tab fills in.
    fn omni_rows(&self, text: &str) -> (Vec<Suggestion>, Option<String>) {
        let words = text.trim();
        if words.is_empty() {
            return (Vec::new(), None);
        }
        let typed = text.trim_start();
        if complete::is_path(typed) {
            return self.path_rows(typed);
        }
        if let Some(scheme) = scheme_of(typed)
            && self.app.provider(scheme).is_some()
        {
            return self.place_rows(typed, scheme);
        }
        let mut rows = Vec::new();
        if self.app.commands.get("query.search").is_some() {
            rows.push(Suggestion::Search { everywhere: false });
            if home().is_some() && home() != self.omni_root() {
                rows.push(Suggestion::Search { everywhere: true });
            }
            // Contents narrow the folder shown; a search shown is edited as
            // a whole instead.
            if self.ui().chips.chips.is_empty() && self.app.pane_dir(self.pane).is_ok() {
                rows.push(Suggestion::Contains);
            }
        }
        let places = self.ui().omni.as_ref().map(|o| o.places.as_slice());
        rows.extend(
            places
                .unwrap_or_default()
                .iter()
                .map(|c| Suggestion::Place {
                    path: c.path.clone(),
                    dir: c.kind == "dir",
                }),
        );
        rows.extend(self.scheme_rows(words));
        if rows.is_empty() {
            rows.push(Suggestion::Go);
        }
        (rows, None)
    }

    /// A path's rows: the names of the folder typed that go on with the
    /// name typed (or of the folders its names start), the best first,
    /// after Go when no name is typed or none goes on with it. A folder
    /// part that names none, with no name typed after it, offers the
    /// folders it stands for.
    fn path_rows(&self, text: &str) -> (Vec<Suggestion>, Option<String>) {
        let mut rows = Vec::new();
        let mut shared = None;
        let (dir, typed) = complete::split(text);
        let resolved = (text != "~")
            .then(|| self.typed_folder(dir))
            .flatten()
            .and_then(|folder| self.ui().omni.as_ref()?.folders.get(&folder)?.clone());
        let mut guessed = false;
        if let Some(resolved) = resolved {
            let sep = complete::separator_of(text);
            let home = home();
            let written = |path: &Path| complete::written(path, text, home.as_deref());
            let after = |dir: bool| if dir { sep.to_string() } else { String::new() };
            if resolved.abbreviated && typed.is_empty() {
                guessed = !resolved.folders.is_empty();
                rows.extend(resolved.folders.iter().map(|folder| {
                    Suggestion::Name {
                        path: folder.path.clone(),
                        dir: true,
                        label: folder
                            .path
                            .file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                        folder: folder.path.parent().map(written),
                        text: written(&folder.path) + &after(true),
                        rest: None,
                    }
                }));
            } else {
                let (found, common) = complete::rank(&resolved.folders, typed, self.shows_hidden());
                for m in found {
                    let folder = &resolved.folders[m.folder];
                    let name = &folder.names[m.index];
                    let path = folder.path.join(&name.exact);
                    let (text, rest, folder) = if resolved.abbreviated {
                        (
                            written(&path) + &after(name.dir),
                            None,
                            Some(written(&folder.path)),
                        )
                    } else {
                        (
                            format!("{dir}{}{}", name.text, after(name.dir)),
                            complete::rest_of(&name.text, typed)
                                .map(|rest| rest + &after(name.dir)),
                            None,
                        )
                    };
                    rows.push(Suggestion::Name {
                        path,
                        dir: name.dir,
                        label: name.text.clone(),
                        folder,
                        text,
                        rest,
                    });
                }
                shared = common.map(|common| format!("{dir}{common}"));
            }
        }
        // `.` and `..` name a folder themselves.
        if (typed.is_empty() && !guessed) || rows.is_empty() || matches!(typed, "." | "..") {
            rows.insert(0, Suggestion::Go);
        }
        (rows, shared)
    }

    /// A place's rows (`scheme:rest`): what its provider completes the
    /// name typed with (`LocationProvider::complete`), as a path's names.
    /// Rows asked for an earlier text stay until the answer comes, drawing
    /// nothing after the text.
    fn place_rows(&self, text: &str, scheme: &str) -> (Vec<Suggestion>, Option<String>) {
        let mut rows = Vec::new();
        let mut shared = None;
        let mut typed_empty = true;
        let offered = self.ui().omni.as_ref().and_then(|o| o.offered.as_ref());
        if let Some((asked, Some(completions))) = offered
            && let Some(asked_rest) = asked.strip_prefix(scheme).and_then(|r| r.strip_prefix(':'))
            && asked_rest.get(completions.range.clone()).is_some()
        {
            let fresh = asked == text;
            typed_empty = completions.typed.is_empty();
            let names = complete::Folder {
                path: PathBuf::new(),
                names: completions
                    .items
                    .iter()
                    .map(|item| {
                        complete::Name::new(item.label.clone().into(), item.inside.is_some(), false)
                    })
                    .collect(),
            };
            let (found, common) = complete::rank(&[Arc::new(names)], &completions.typed, true);
            let (before, after) = (
                &asked_rest[..completions.range.start],
                &asked_rest[completions.range.end..],
            );
            let at_end = after.is_empty();
            for m in found {
                let item = &completions.items[m.index];
                let inside = item.inside.map(String::from).unwrap_or_default();
                rows.push(Suggestion::Offered {
                    location: item.location.clone(),
                    label: item.label.clone(),
                    text: format!("{scheme}:{before}{}{inside}{after}", item.text),
                    rest: (fresh && at_end)
                        .then(|| complete::rest_of(&item.label, &completions.typed))
                        .flatten()
                        .map(|rest| rest + &inside),
                });
            }
            shared = common.filter(|_| fresh).map(|common| {
                let common = skwir_core::location::escape(common.as_bytes());
                format!("{scheme}:{before}{common}{after}")
            });
        }
        if typed_empty || rows.is_empty() {
            rows.insert(0, Suggestion::Go);
        }
        (rows, shared)
    }

    /// The places of the schemes `words` start (`tr` for the trash), as
    /// providers name them; never a search's, whose text is words.
    fn scheme_rows(&self, words: &str) -> Vec<Suggestion> {
        let Some(start) = scheme_start(words) else {
            return Vec::new();
        };
        self.app
            .schemes()
            .into_iter()
            .filter(|scheme| &**scheme != PATH_SCHEME && scheme.starts_with(start))
            .filter_map(|scheme| {
                let location = Location::new(&scheme, "");
                let provider = self.app.provider(&scheme)?;
                (!provider.searches(&location)).then(|| Suggestion::Offered {
                    label: self.app.location_title(&location),
                    text: format!("{scheme}:"),
                    location,
                    rest: None,
                })
            })
            .collect()
    }
}

/// Where the search typed will look, as a pill before the text: only while
/// the active pane's field is being typed in, in search.
pub fn scope_pill(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let scope = shell.ui().omni.as_ref()?.scope.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    Some(
        div()
            .flex_none()
            .ml(px(3.))
            .h(px(22.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_full()
            .border_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.text_muted)
            .child(
                gpui::svg()
                    .path("toolbar/folder.svg")
                    .size(px(14.))
                    .flex_none()
                    .text_color(theme.text_muted),
            )
            .child(format!("In {}", folder_name(&shell.app, scope)))
            .into_any_element(),
    )
}

/// The dropdown under the field of the pane being rendered, while it is
/// typed in and its text says something; `anchor` is the field's. It hangs
/// from the field's right end, so the names the typing filters stay in
/// sight at the left. Its rows are a list box beside the field, which
/// keeps the keyboard: the row the arrows reach is its active descendant.
pub fn dropdown(
    shell: &Shell,
    anchor: &PopoverAnchor,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let suggestions = shell.suggestions();
    if suggestions.is_empty() {
        return None;
    }
    let omni = shell.ui().omni.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let text = shell.ui().address.read(cx).text().trim().to_owned();
    let root = shell.omni_root();
    let pane = shell.pane;
    let a11y = shell.a11y;
    let field = gpui::Focusable::focus_handle(shell.ui().address.read(cx), cx);
    // Words: what is gone to is set apart from the searches.
    let words = suggestions
        .iter()
        .any(|s| matches!(s, Suggestion::Search { .. }));
    let mut list = crate::modal::options("omnibox-rows", "Suggestions", suggestions.len(), a11y)
        .flex()
        .flex_col();
    let mut places_titled = false;
    for (i, suggestion) in suggestions.iter().enumerate() {
        let (icon, title, detail, keys): (String, String, Option<String>, Option<&str>) =
            match suggestion {
                Suggestion::Go => ("open".into(), format!("Go to {text}"), None, None),
                Suggestion::Search { everywhere: false } => (
                    "search".into(),
                    format!(
                        "“{text}” in {} and its folders",
                        root.as_deref().map_or_else(
                            || "this folder".to_owned(),
                            |root| folder_name(&shell.app, root)
                        )
                    ),
                    None,
                    None,
                ),
                Suggestion::Search { everywhere: true } => (
                    "search".into(),
                    format!("“{text}” everywhere in Home"),
                    None,
                    Some("Alt+Enter"),
                ),
                Suggestion::Contains => (
                    "contents".into(),
                    format!("Files containing “{text}”"),
                    None,
                    None,
                ),
                Suggestion::Place { path, dir } => (
                    if *dir { "folder" } else { "open" }.into(),
                    path.file_name().map_or_else(
                        || path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    ),
                    path.parent().map(|p| p.display().to_string()),
                    None,
                ),
                Suggestion::Name {
                    dir, label, folder, ..
                } => (
                    if *dir { "folder" } else { "open" }.into(),
                    label.clone(),
                    folder.clone(),
                    None,
                ),
                Suggestion::Offered {
                    location, label, ..
                } => (
                    crate::trail::glyph(&shell.app, location).into(),
                    label.clone(),
                    Some(location.to_string()),
                    None,
                ),
            };
        let goes = matches!(
            suggestion,
            Suggestion::Place { .. } | Suggestion::Offered { .. }
        );
        if words && goes && !places_titled {
            places_titled = true;
            list = list.child(
                div()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Go to"),
            );
        }
        let highlighted = i == omni.highlighted;
        let keys = if highlighted {
            Some(match suggestion {
                Suggestion::Name { dir: false, .. } => "Enter · Shift+Enter opens",
                _ if suggestion.completion().is_some() => "Enter · Tab",
                _ => "Enter",
            })
        } else {
            keys
        };
        let hover = theme.hover;
        let muted = if highlighted {
            theme.selection_text
        } else {
            theme.text_muted
        };
        let named = a11y.then(|| match &detail {
            Some(detail) => format!("{title}, {detail}").into(),
            None => title.clone().into(),
        });
        list = list.child(
            // Said as focused once the arrows reach it: while typing, the
            // field reads back its text and caret.
            crate::modal::option(
                div().id(("omnibox", i)),
                named,
                i,
                highlighted && omni.chosen,
                &field,
            )
            .h(px(30.))
            .px_2()
            .flex()
            .items_center()
            .gap(px(10.))
            .rounded_md()
            .cursor_pointer()
            .when(highlighted, |d| {
                d.bg(theme.selection_bg).text_color(theme.selection_text)
            })
            .when(!highlighted, |d| d.hover(move |s| s.bg(hover)))
            // The field keeps the keyboard: the row acts on the press,
            // before anything under it could take the focus.
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    this.activate_pane(pane, cx);
                    if let Some(omni) = this.ui_mut().omni.as_mut() {
                        omni.highlighted = i;
                    }
                    this.omnibox_submit(false, false, window, cx);
                }),
            )
            .child(
                gpui::svg()
                    .path(format!("toolbar/{icon}.svg"))
                    .size(px(15.))
                    .flex_none()
                    .text_color(muted),
            )
            .child(crate::elements::clip::clipped(
                div()
                    .id("omni-title")
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle(),
                title,
            ))
            .children(detail.map(|detail| {
                crate::elements::clip::clipped(
                    div()
                        .id("omni-detail")
                        .max_w(px(220.))
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .text_xs()
                        .text_color(muted),
                    detail,
                )
            }))
            .children(keys.map(|keys| {
                div()
                    .flex_none()
                    .pl_4()
                    .text_xs()
                    .text_color(muted)
                    .child(keys)
            })),
        );
    }
    // A folder that takes a moment to read (a disk asleep) says so.
    let reading = omni
        .reading
        .as_ref()
        .filter(|(_, since, _)| since.elapsed() >= READING_SAID)
        .map(|(folder, ..)| format!("Reading {}…", folder.display()));
    let panel = div()
        .min_w(px(360.))
        .max_w(px(560.))
        .p_1()
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .text_color(theme.text)
        .shadow_md()
        .occlude()
        .child(list)
        .children(reading.map(|reading| {
            div()
                .px_2()
                .py_1()
                .text_xs()
                .text_color(theme.text_muted)
                .child(reading)
        }))
        .child(
            div()
                .mt_1()
                .px_2()
                .pt_2()
                .pb_1()
                .border_t_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(theme.text_muted)
                .child(
                    "Start with / or ~ for a path, Tab completes it; > for a command. Escape goes back.",
                ),
        );
    Some(anchor.popup_right(panel))
}

#[cfg(test)]
mod tests {
    use super::{scheme_of, scheme_start};

    #[test]
    fn schemes_are_told_from_words() {
        assert_eq!(scheme_of("query:kind:image"), Some("query"));
        assert_eq!(scheme_of("kind:image"), Some("kind"));
        assert_eq!(scheme_of("it's: a draft"), None);
        assert_eq!(scheme_of("report"), None);
        assert_eq!(scheme_start("tr"), Some("tr"));
        assert_eq!(scheme_start("t"), None);
        assert_eq!(scheme_start("Trash"), None);
        assert_eq!(scheme_start("two words"), None);
    }
}
