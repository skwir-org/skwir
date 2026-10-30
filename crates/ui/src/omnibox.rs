//! The address field as the one place to type (ADR-070, ADR-098): a path,
//! a search, a place to go to, a scope to search in, or a command.
//!
//! The field may hold a scope before its text, a token: where words look.
//! Words look as deep as the field says (ADR-098, `Depth`): in the folder
//! shown alone, filtered at each key, or in it and its subfolders, searched
//! a pause after the last key. Ctrl+Shift+F (`ui.filter`) starts with the
//! folder shown as the scope, this folder only; Ctrl+F (`ui.search`) with
//! its subfolders too; while typing, each switches to its own depth and
//! keeps the words, as the switch on the card's first line does. Ctrl+L
//! (`ui.focus_address`) starts with no scope and the path selected, words
//! then filtering the folder shown; Ctrl+J (`ui.jump`) with none and
//! nothing typed, places first. Over a search, Ctrl+F and Ctrl+L edit it,
//! its root as the scope. Backspace at the start of the text takes the
//! scope away (Left there selects it first); a click on the token, Down on
//! it, Alt+Down, or `@` typed first in an empty field list where else to
//! search. Ctrl+F on a highlighted folder or place makes it the scope.
//!
//! What the dropdown offers comes from sources (`skwir_core::suggest`):
//! each asked again at every change of the text, each in a job of its own,
//! its answer kept by source until the next. The query plugin answers what
//! Enter does with words (the search, or why it cannot run), what the term
//! being typed goes on with, the words read as terms, other scopes, and
//! before anything is typed the terms to add and the searches run lately
//! and saved; Jump the places one goes to; the tags plugin the tags; the
//! shell the other tabs, the pane's history, the sidebar's places and the
//! commands. The shell keeps one source of its own here: a path lists the
//! names of the folder typed that go on with it (`complete.rs`, ADR-070),
//! a place's text those its provider completes it with
//! (`LocationProvider::complete`). Rows are put in their sections' order;
//! an answer arriving later never moves the row the arrows chose. A key
//! takes no rows away: each source's stay until its answer to the new
//! text, so the card never closes or shrinks for a frame between keys;
//! one answering the text before draws after it only what it still adds,
//! and Enter and Tab, pressed before every source answered, ask them all
//! first.
//!
//! Words typed over a folder, this folder only, filter it at once (the
//! core's `filter`, which the query plugin reads: words find names, terms
//! the listing can decide hold for files). With its subfolders, the search
//! Enter would run is shown as they are typed (`Suggestion::live`), in
//! place of one step of the pane's history: the field's own, which Escape
//! takes back. A search that reads the files, or looks below a folder on a
//! server, waits for Enter. The text is drawn with the spans the query
//! plugin reads in it (`query.tokens`): keys in the accent, values tinted,
//! what cannot be read underlined.
//!
//! Tab completes (`ui.omnibox_complete`); Right or End at the end takes
//! what is drawn after the text; Enter keeps the words shown and gives the
//! files the keyboard, or runs the highlighted row; Alt+Enter runs the
//! search in the home folder, Shift+Enter opens a file a path names;
//! Shift+Delete forgets a recent search (`ui.omnibox_forget`). Enter and
//! Tab pressed while the names a path or a place goes on with are read
//! wait for them (`Held`), and are done as if pressed once they came. The
//! keyboard never leaves the field: Up and Down move through the dropdown
//! (bound in the address bar's context), a click on a row runs it without
//! taking the focus. The UI knows no query syntax.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{AnyElement, Context, Window, div, prelude::*, px};
use skwir_core::cancel::CancelToken;
use skwir_core::command::Invocation;
use skwir_core::location::{Completions, Location, PATH_SCHEME};
use skwir_core::os_path::PathArg;
use skwir_core::scheduler::Lane;
use skwir_core::suggest::{Answer, Ask, Section, Span, SpanKind, Suggestion, Typed};

use crate::complete::{self, Resolved};
use crate::elements::popover::PopoverAnchor;
use crate::input::{AtStart, Paint};
use crate::shell::Shell;
use crate::theme::Theme;

/// The folders read for completion kept while the field is typed in.
const FOLDERS_KEPT: usize = 16;

/// How long a folder is read before the dropdown says so.
const READING_SAID: Duration = Duration::from_millis(200);

/// The most scopes listed.
const SCOPES: usize = 10;

/// How long after the last key a search below the folder is shown.
const LIVE_PAUSE: Duration = Duration::from_millis(400);

/// How far the field's card reaches beyond the field: before, above and
/// after it.
const CARD_PAD: f32 = 5.;
/// The least width of the field's card, in a narrow pane.
const CARD_LEAST: f32 = 430.;
/// Where the field's card is drawn among deferred draws (the popups'): the
/// field's text draws one above it.
pub(crate) const CARD_PRIORITY: usize = 3;

/// How the field takes the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Begin {
    /// Ctrl+F: words search where the pane is, and below.
    Search,
    /// Ctrl+Shift+F: words filter the folder shown.
    Filter,
    /// Ctrl+L: the path, selected.
    Path,
    /// Ctrl+J: nothing typed, places first.
    Jump,
}

/// How deep words look (ADR-098).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Depth {
    /// The folder shown alone: filtered at each key.
    #[default]
    Here,
    /// It and its subfolders: searched a pause after the last key.
    Below,
}

/// A pane's field while it is being typed in.
#[allow(clippy::struct_excessive_bools)] // Independent states of the field: its token, its list, edited, a row chosen, a step taken.
#[derive(Default)]
pub struct Omni {
    /// How deep words look.
    pub depth: Depth,
    /// What the pane showed, and its filter and narrowing, when the field
    /// took the keyboard: what Escape shows again.
    before: Option<Before>,
    /// The field showed a location of its own (a search as it is typed, a
    /// search's folder to filter): one step of the pane's history, which
    /// it shows in place and Escape takes back.
    stepped: bool,
    /// The words whose search the pane shows, shown as typed, and the
    /// scope it looks in: the same words in another scope are another
    /// search.
    shown: Option<(String, Option<Location>)>,
    /// The search shown once the pause after the last key is over.
    pending: Option<gpui::Task<()>>,
    /// Where words search, drawn as a token before the text; `None`: the
    /// field goes anywhere (words then search the folder shown).
    pub scope: Option<Location>,
    /// The token is selected (Left at the start of the text): Backspace
    /// takes it away, Down lists the other scopes.
    pub token: bool,
    /// The list of scopes is open (the token clicked, Down on it, Alt+Down).
    scopes: bool,
    /// The text changed since the field took the keyboard.
    edited: bool,
    /// The row Enter runs; `None` before anything is typed, until the
    /// arrows reach one.
    highlighted: Option<usize>,
    /// The highlight was moved (Up, Down, Tab): Tab takes that row, and an
    /// answer arriving later keeps it on the same row.
    chosen: bool,
    /// The rows for the text, in the order shown (`refresh_rows`).
    rows: Vec<Row>,
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
    /// What the sources were asked last, and what stops their jobs.
    asked: Option<(Ask, CancelToken)>,
    /// The sources asked last that have not answered yet.
    waiting: Vec<String>,
    /// Each source's last answer, and the text and scope it answered.
    answers: Vec<(String, (String, Option<Location>), Answer)>,
    /// A key pressed while the names the text completes with were read
    /// (`completing`): done once they come. Another text, or the arrows,
    /// forget it.
    held: Option<Held>,
}

/// A key that takes a row, held until the names it chooses among are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Held {
    /// Enter, Alt+Enter (`everywhere`), Shift+Enter (`open`).
    Submit { everywhere: bool, open: bool },
    /// Tab.
    Complete,
}

impl Omni {
    /// Whether every source has answered `text` in the scope as it is: the
    /// rows are the text's own.
    fn answered(&self, text: &str) -> bool {
        self.waiting.is_empty()
            && self
                .answers
                .iter()
                .all(|(_, (asked, scope), _)| asked == text && *scope == self.scope)
    }
}

/// What a pane showed when its field took the keyboard.
struct Before {
    location: Option<Location>,
    /// Where words filter: the location shown, or the folder a search
    /// shown looks in (the place it lists).
    folder: Option<Location>,
    filter: Option<String>,
    /// The words of the pane's lens when the field began without them (on
    /// a path, Ctrl+L, or on nothing, Ctrl+J): words typed add to the lens
    /// (ADR-107), they never take its place.
    kept: Option<String>,
}

impl Drop for Omni {
    fn drop(&mut self) {
        if let Some((.., cancel)) = &self.reading {
            cancel.cancel();
        }
        if let Some((_, cancel)) = &self.offering {
            cancel.cancel();
        }
        if let Some((_, cancel)) = &self.asked {
            cancel.cancel();
        }
    }
}

/// A row of the dropdown.
#[derive(Clone, Debug, PartialEq)]
enum Row {
    /// Go to the location the text names: a folder, a file shown in its
    /// folder.
    Go,
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
    /// What the field takes (`?`): Enter or Tab writes `example`.
    Help {
        label: String,
        detail: String,
        icon: &'static str,
        example: String,
    },
    /// Where words could search: Enter makes it the scope.
    Scope {
        location: Location,
        label: String,
        icon: &'static str,
        detail: Option<String>,
    },
    /// A source's row.
    Source(Box<Suggestion>),
}

impl Row {
    /// What Tab writes, for a completion.
    fn completion(&self) -> Option<&str> {
        match self {
            Row::Name { text, .. } | Row::Offered { text, .. } => Some(text),
            Row::Help { example, .. } => Some(example),
            Row::Source(s) => s.completion.as_deref(),
            _ => None,
        }
    }

    /// What the row adds after the text typed, drawn there.
    fn rest(&self) -> Option<&str> {
        match self {
            Row::Name { rest, .. } | Row::Offered { rest, .. } => rest.as_deref(),
            Row::Source(s) => s.rest.as_deref(),
            _ => None,
        }
    }

    /// Where Ctrl+F on the row searches.
    fn scope(&self, app: &skwir_core::App) -> Option<Location> {
        match self {
            Row::Name {
                path, dir: true, ..
            } => Some(Location::from_path(path)),
            Row::Offered { location, .. } | Row::Scope { location, .. } => {
                crate::suggestions::scope_of(app, location)
            }
            Row::Source(s) => s.scope.clone(),
            _ => None,
        }
    }

    fn section(&self) -> Section {
        match self {
            Row::Source(s) => s.section,
            Row::Scope { .. } => Section::Scopes,
            Row::Help { .. } => Section::Help,
            _ => Section::Run,
        }
    }

    /// What the row is, to find it again among rows made anew.
    fn key(&self) -> (Section, String) {
        let title = match self {
            Row::Go => String::new(),
            Row::Name { label, .. }
            | Row::Offered { label, .. }
            | Row::Scope { label, .. }
            | Row::Help { label, .. } => label.clone(),
            Row::Source(s) => s.title.clone(),
        };
        (self.section(), title)
    }
}

/// What a row answering an earlier text adds after `text`: the rest of its
/// completion where that goes on from `text`, nothing where `text` left it.
fn rest_after(row: &Suggestion, text: &str) -> Option<String> {
    row.rest.as_ref()?;
    row.completion
        .as_deref()?
        .strip_prefix(text)
        .filter(|rest| !rest.is_empty())
        .map(str::to_owned)
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

/// The path a `file://` URI names on this machine, as the field writes it;
/// `None` for any other text, another machine's URI, or a path the field
/// could only write approximately (not UTF-8).
fn uri_path(text: &str) -> Option<String> {
    let path = skwir_platform::uri::path_of(text.trim())?;
    path.to_str().map(str::to_owned)
}

/// Words that may start a scheme's name (`tr` for `trash:`): one lowercase
/// word, two letters at least.
fn scheme_start(text: &str) -> Option<&str> {
    (text.len() >= 2 && text.chars().all(|c| c.is_ascii_lowercase() || c == '-')).then_some(text)
}

fn home() -> Option<PathBuf> {
    skwir_core::os_path::home()
}

/// What `text` is, as the sources are told: nothing typed (or the text as
/// given, not `edited`), `@` and a scope, a path, a place's address (a
/// scheme a provider opens: `query:`, `shelf:`; `kind:` is a word of the
/// query language), else words.
pub fn typed_for(app: &skwir_core::App, text: &str, edited: bool) -> Typed {
    let typed = text.trim_start();
    if !edited || typed.trim().is_empty() {
        Typed::Nothing
    } else if typed.trim() == "?" {
        Typed::Help
    } else if typed.starts_with('@') {
        Typed::Scope
    } else if complete::is_path(typed) {
        Typed::Path
    } else if scheme_of(typed).is_some_and(|s| app.provider(s).is_some()) {
        Typed::Place
    } else {
        Typed::Words
    }
}

/// A path's names as rows, read here and now: what `ui.suggest` answers
/// for a path (the field reads the folder typed in its lane as it is
/// typed). Folders first, the best first, as the field ranks them; Enter
/// goes to a folder, shows a file selected in its folder.
pub fn path_suggestions(
    app: &skwir_core::App,
    text: &str,
    base: Option<&Path>,
    pane: Option<skwir_core::workspace::PaneId>,
    cancel: &CancelToken,
) -> Vec<Suggestion> {
    let (dir, typed) = complete::split(text);
    let home = home();
    let Some(folder) = complete::resolve(dir, base, home.as_deref()) else {
        return Vec::new();
    };
    // An agent's call (`permissions::as_agent`) reads no folder that is
    // private or on a machine not reached yet, as the text resolves, and
    // is told no private name (ADR-044).
    let agent = skwir_core::permissions::agent_call();
    let mut check = agent
        .as_ref()
        .map(skwir_core::permissions::Privacy::checker);
    if check
        .as_mut()
        .is_some_and(|check| check.admits(&folder).is_err())
    {
        return Vec::new();
    }
    let resolved = complete::folders(app, &folder, false, cancel);
    let (found, _) = complete::rank(&resolved.folders, typed, false);
    found
        .into_iter()
        .filter(|m| {
            check.as_mut().is_none_or(|check| {
                let folder = &resolved.folders[m.folder];
                !check.holds(&folder.path.join(&folder.names[m.index].exact))
            })
        })
        .enumerate()
        .map(|(rank, m)| {
            let folder = &resolved.folders[m.folder];
            let name = &folder.names[m.index];
            let path = folder.path.join(&name.exact);
            let run = if name.dir {
                serde_json::json!({ "location": PathArg::from(path.clone()), "pane": pane })
            } else {
                serde_json::json!({
                    "location": PathArg::from(folder.path.clone()),
                    "select": PathArg::from(PathBuf::from(&name.exact)),
                    "pane": pane,
                })
            };
            let mut row = Suggestion::new(Section::Go, name.text.clone())
                .detail(complete::written(&folder.path, text, home.as_deref()))
                .icon(if name.dir { "folder" } else { "open" })
                .rank(i32::try_from(rank).unwrap_or(i32::MAX))
                .runs(Invocation::with_args("navigate", &run));
            let sep = complete::separator_of(text);
            row.completion = Some(format!(
                "{dir}{}{}",
                name.text,
                if name.dir {
                    sep.to_string()
                } else {
                    String::new()
                }
            ));
            if name.dir {
                row.scope = Some(Location::from_path(&path));
            }
            row
        })
        .collect()
}

/// What a scope is called on its token and in the rows: the root of the
/// file system is the whole disk.
pub fn scope_name(app: &skwir_core::App, scope: &Location) -> String {
    match scope.path() {
        Some(path) if path.parent().is_none() => "Whole disk".to_owned(),
        _ => app.location_title(scope),
    }
}

/// The glyph a scope's token and row wear.
fn scope_glyph(app: &skwir_core::App, scope: &Location) -> &'static str {
    match scope.path() {
        Some(path) if path.parent().is_none() => "layers",
        Some(path) if Some(path.clone()) == home() => "home",
        _ => crate::trail::glyph(app, scope),
    }
}

/// Whether a move goes down the list (Down, Page Down, End).
fn forward(mv: crate::modal::ListMove) -> bool {
    use crate::modal::ListMove;
    match mv {
        ListMove::By(delta) | ListMove::Page(delta) => delta > 0,
        ListMove::Last => true,
        ListMove::First => false,
    }
}

/// How the sources' spans are drawn in the field.
fn paint(kind: SpanKind, theme: &Theme) -> Option<Paint> {
    match kind {
        SpanKind::Word => None,
        SpanKind::Key => Some(Paint {
            color: Some(theme.accent_text),
            ..Paint::default()
        }),
        SpanKind::Value => Some(Paint {
            background: Some(theme.term),
            ..Paint::default()
        }),
        SpanKind::Operator => Some(Paint {
            color: Some(theme.text_muted),
            ..Paint::default()
        }),
        SpanKind::Error => Some(Paint {
            wavy: Some(theme.error),
            ..Paint::default()
        }),
    }
}

impl Shell {
    /// Whether `text` names a location rather than words to look for: a
    /// path, or a scheme a provider opens (`query:`, `shelf:`; `kind:` is
    /// a word of the query language).
    fn names_location(&self, text: &str) -> bool {
        complete::is_path(text) || scheme_of(text).is_some_and(|s| self.app.provider(s).is_some())
    }

    /// What the text is, as the sources are told.
    pub(crate) fn typed_of(&self, text: &str, edited: bool) -> Typed {
        typed_for(&self.app, text, edited)
    }

    /// Give the active pane's field the keyboard, as `how` says, with
    /// `text` to start from. A pane showing a search edits that search,
    /// its root as the scope (Ctrl+J starts anew); this folder only
    /// (Ctrl+Shift+F), the folder it looks in shows, filtered by its words.
    /// With the field already typed in, Ctrl+F makes the folder or place
    /// highlighted the scope, else words look below the folder; Ctrl+Shift+F
    /// brings them back to the folder alone.
    pub fn omnibox_begin(
        &mut self,
        how: Begin,
        text: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(how, Begin::Search | Begin::Filter)
            && text.is_none()
            && self.ui().omni.is_some()
            && self.ui().address.read(cx).is_focused(window)
        {
            if how == Begin::Search && self.scope_from_highlight(window, cx) {
                return;
            }
            let depth = if how == Begin::Search {
                Depth::Below
            } else {
                Depth::Here
            };
            return self.set_depth(depth, window, cx);
        }
        let pane = self.pane;
        let chips = &self.ui().chips;
        let shown_dir = self.app.pane_dir(pane).ok();
        let over_search = !chips.chips.is_empty();
        // Where words filter: the folder shown, or the one a search looks
        // in (the place it lists).
        let folder = if over_search {
            Some(
                chips
                    .within
                    .clone()
                    .unwrap_or_else(|| Location::from_path(&chips.root)),
            )
        } else {
            self.app.pane_location(pane)
        };
        let depth = match how {
            Begin::Search => Depth::Below,
            // Over a search, words are its, looking where it looks.
            Begin::Path | Begin::Jump if over_search => Depth::Below,
            Begin::Filter | Begin::Path | Begin::Jump => Depth::Here,
        };
        // A search's words over the folder alone filter it at once.
        let filters_search = how == Begin::Filter && over_search;
        let (scope, initial, select) = if over_search && how != Begin::Jump {
            let given = text.is_some();
            (
                Some(
                    chips
                        .within
                        .clone()
                        .unwrap_or_else(|| Location::from_path(&chips.root)),
                ),
                text.unwrap_or_else(|| chips.text.clone()),
                !given,
            )
        } else {
            match how {
                // A folder filtered: its words, to edit, as deep as asked.
                Begin::Search | Begin::Filter => (
                    shown_dir.as_ref().map(Location::from_path).or_else(|| {
                        self.app
                            .pane_location(pane)
                            .filter(|l| self.app.searchable(l))
                    }),
                    text.or_else(|| {
                        self.app
                            .workspace
                            .read()
                            .pane(pane)
                            .and_then(|p| p.filter().map(str::to_owned))
                    })
                    .unwrap_or_default(),
                    false,
                ),
                Begin::Path => (
                    None,
                    text.unwrap_or_else(|| self.ui().address_shown.clone()),
                    true,
                ),
                Begin::Jump => (None, text.unwrap_or_default(), false),
            }
        };
        let given = !initial.is_empty() && !select;
        // The text holds the lens where it is the filter's or the search's
        // words; begun on a path or on nothing, the lens is kept beside it.
        let holds_lens = match how {
            Begin::Search | Begin::Filter => true,
            Begin::Path => over_search,
            Begin::Jump => false,
        };
        let before = {
            let ws = self.app.workspace.read();
            let p = ws.pane(pane);
            Before {
                location: p.and_then(|p| p.location().cloned()),
                folder,
                filter: p.and_then(|p| p.filter().map(str::to_owned)),
                kept: p
                    .and_then(|p| p.lens())
                    .filter(|_| !holds_lens)
                    .map(|l| l.words.to_string()),
            }
        };
        let mut omni = Omni::default();
        omni.scope = scope;
        omni.edited = given || (filters_search && !initial.is_empty());
        omni.depth = depth;
        omni.before = Some(before);
        self.ui_mut().omni = Some(omni);
        let placeholder = self.placeholder();
        let address = self.ui().address.clone();
        address.update(cx, |input, cx| {
            input.send_start(true);
            input.set_placeholder(placeholder, cx);
            input.set_text(initial, cx);
            input.set_hint(None, cx);
            if select {
                input.select_all_now(cx);
            }
            input.focus(window, cx);
        });
        // Over a search, this folder only: the folder it looks in shows.
        if filters_search {
            self.show_here(window, cx);
        }
        if given || filters_search {
            self.omnibox_changed_keep_edit(window, cx);
        } else {
            self.ask_suggestions(window, cx);
            self.refresh_rows(cx);
        }
        cx.notify();
    }

    /// How deep words look changed, or is asked to (Ctrl+F, Ctrl+Shift+F,
    /// the switch on the card): the scope says where, the pane shows what
    /// the words keep there, the words kept.
    pub(crate) fn set_depth(&mut self, depth: Depth, window: &mut Window, cx: &mut Context<Self>) {
        let shown_dir = self.app.pane_dir(self.pane).ok().map(Location::from_path);
        let typed = !self.omni_text(cx).trim().is_empty();
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return;
        };
        omni.depth = depth;
        // Words given, switched: they are what is looked for.
        omni.edited |= typed;
        omni.token = false;
        omni.scopes = false;
        let folder = omni.before.as_ref().and_then(|b| b.folder.clone());
        match depth {
            // The folder alone: the token names it, as it did.
            Depth::Here if omni.scope.is_some() => omni.scope = folder,
            Depth::Below if omni.scope.is_none() => omni.scope = shown_dir.or(folder),
            Depth::Here | Depth::Below => {}
        }
        if depth == Depth::Here {
            self.show_here(window, cx);
        }
        let placeholder = self.placeholder();
        let address = self.ui().address.clone();
        address.update(cx, |input, cx| input.set_placeholder(placeholder, cx));
        self.omnibox_changed_keep_edit(window, cx);
        // A switch is asked for: what it shows comes now, not after a pause.
        if depth == Depth::Below {
            self.show_live(window, cx);
        }
    }

    /// The words look in the folder alone: it shows, the search shown as
    /// they were typed taken back, or, over a search, the folder it looks
    /// in shown in place of the field's step.
    fn show_here(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.pane;
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return;
        };
        omni.pending = None;
        omni.shown = None;
        let Some(before) = &omni.before else {
            return;
        };
        let Some(folder) = before.folder.clone() else {
            return;
        };
        let back = omni.stepped && before.location.as_ref() == Some(&folder);
        if self.app.pane_location(pane).as_ref() == Some(&folder) {
            return;
        }
        if back {
            self.take_back(cx);
            return;
        }
        self.step(cx);
        if let Err(error) = self.app.navigate_in_place(pane, &folder) {
            tracing::warn!("address: {error:#}");
        }
    }

    /// The field takes a step of the pane's history of its own, once: what
    /// it shows goes in its place, and Escape takes it back.
    fn step(&mut self, _cx: &mut Context<Self>) {
        let pane = self.pane;
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return;
        };
        if omni.stepped {
            return;
        }
        omni.stepped = true;
        if let Err(error) = self.app.step_here(pane) {
            tracing::warn!("address: {error:#}");
        }
    }

    /// The field's step taken back: the pane shows what it showed when the
    /// field took the keyboard; its filter is put back by the caller.
    fn take_back(&mut self, cx: &mut Context<Self>) {
        let pane = self.pane;
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return;
        };
        omni.pending = None;
        omni.shown = None;
        if !std::mem::take(&mut omni.stepped) {
            return;
        }
        if let Err(error) = self.app.take_step_back(pane) {
            tracing::warn!("address: {error:#}");
        }
        cx.notify();
    }

    /// Words looking below the folder: what they find is shown once the
    /// pause after the last key is over, the search waiting before
    /// dropped; none typed, what was shown before.
    fn schedule_live(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pane = self.pane;
        // What the pane shows now: shown another way since (a place
        // clicked, Back), it is not the field's to replace.
        let shown = self.app.pane_location(pane);
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return;
        };
        omni.pending = None;
        if omni.depth != Depth::Below {
            return;
        }
        let task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(LIVE_PAUSE).await;
            this.update_in(cx, |shell, window, cx| {
                // Still typed in, in a pane of the tab shown, showing what
                // it showed.
                if shell
                    .panes
                    .iter()
                    .any(|ui| ui.id == pane && ui.omni.is_some())
                    && shell.app.pane_location(pane) == shown
                {
                    shell.with_pane(pane, |shell| shell.show_live(window, cx));
                }
            })
            .ok();
        });
        if let Some(omni) = self.ui_mut().omni.as_mut() {
            omni.pending = Some(task);
        }
    }

    /// Show the search of the words typed, as typed (`Suggestion::live`):
    /// in place of the field's step. Words whose search waits for Enter
    /// leave the pane as it is; no words, what it showed before.
    fn show_live(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.omni_text(cx);
        let words = raw.trim().to_owned();
        let pane = self.pane;
        let over_search = self
            .app
            .pane_location(pane)
            .is_some_and(|l| self.app.is_search(&l));
        let typed = self.typed_of(&raw, true);
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return;
        };
        omni.pending = None;
        let searched = (words, omni.scope.clone());
        if omni.depth != Depth::Below || omni.shown.as_ref() == Some(&searched) {
            return;
        }
        if searched.0.is_empty() || typed != Typed::Words {
            return self.take_back(cx);
        }
        // A search's words as they were given: it is what shows.
        if !omni.edited && over_search {
            omni.shown = Some(searched);
            return;
        }
        let Some(live) = self.run_row(&raw, cx).and_then(|row| row.live) else {
            return;
        };
        self.step(cx);
        // Dispatched directly, as the filter is: what is typed is not traced.
        match self.app.dispatch(&live) {
            Ok(_) => {
                if let Some(omni) = self.ui_mut().omni.as_mut() {
                    omni.shown = Some(searched);
                }
            }
            Err(error) => tracing::warn!("{}: {error:#}", live.name),
        }
        cx.notify();
    }

    /// The row that says what Enter runs with the words `raw` (the search,
    /// or why it cannot run): from an answer to them, else asked now.
    fn run_row(&mut self, raw: &str, cx: &mut Context<Self>) -> Option<Suggestion> {
        if !self.has_fresh_run(raw) {
            let mut ask = self.ask_now(cx);
            raw.clone_into(&mut ask.text);
            ask.caret = raw.len();
            ask.typed = self.typed_of(raw, true);
            self.answer_here(&ask);
        }
        let omni = self.ui().omni.as_ref()?;
        omni.answers
            .iter()
            .filter(|(_, (asked, scope), _)| asked == raw && *scope == omni.scope)
            .flat_map(|(.., answer)| answer.rows.iter())
            .find(|row| row.section == Section::Run)
            .cloned()
    }

    /// What the empty field says: where words search, or what it takes.
    fn placeholder(&self) -> String {
        let omni = self.ui().omni.as_ref();
        match omni.and_then(|o| o.scope.as_ref().map(|s| (s, o.depth))) {
            Some((scope, Depth::Here)) => {
                format!("Filter {}, or type a path", scope_name(&self.app, scope))
            }
            Some((scope, Depth::Below)) => format!(
                "Search {} and below, or type a path",
                scope_name(&self.app, scope)
            ),
            None if omni.is_some_and(|o| o.depth == Depth::Below) => {
                "A path, a place, or words to search".to_owned()
            }
            None => "A path, a place, or words to filter".to_owned(),
        }
    }

    /// Ctrl+F in the field on a highlighted folder or place: it becomes
    /// the scope, words looking below it, the text cleared for them.
    /// Whether one was highlighted.
    fn scope_from_highlight(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let row = self
            .ui()
            .omni
            .as_ref()
            .and_then(|o| o.highlighted.and_then(|i| o.rows.get(i)))
            .and_then(|row| row.scope(&self.app));
        let Some(scope) = row else {
            return false;
        };
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return false;
        };
        omni.scope = Some(scope);
        omni.token = false;
        omni.scopes = false;
        self.scope_changed(true, window, cx);
        true
    }

    /// The scope changed: the placeholder says it, the text is cleared when
    /// `clear` (a scope chosen for words still to type), and the sources
    /// are asked again.
    fn scope_changed(&mut self, clear: bool, window: &mut Window, cx: &mut Context<Self>) {
        let typed = !clear && !self.omni_text(cx).trim().is_empty();
        // Another place than the one words filter is searched; no scope,
        // words filter the folder shown.
        if let Some(omni) = self.ui_mut().omni.as_mut() {
            // Words given (a search's), looked for elsewhere: they are what
            // is looked for, as a switch makes them.
            omni.edited |= typed;
            let folder = omni.before.as_ref().and_then(|b| b.folder.clone());
            omni.depth = match &omni.scope {
                None => Depth::Here,
                Some(scope) if Some(scope) != folder.as_ref() => Depth::Below,
                Some(_) => omni.depth,
            };
        }
        if self
            .ui()
            .omni
            .as_ref()
            .is_some_and(|o| o.depth == Depth::Here)
        {
            self.show_here(window, cx);
        }
        let placeholder = self.placeholder();
        let address = self.ui().address.clone();
        address.update(cx, |input, cx| {
            input.set_placeholder(placeholder, cx);
            if clear {
                input.set_text("", cx);
            }
        });
        if clear && let Some(omni) = self.ui_mut().omni.as_mut() {
            omni.edited = false;
        }
        self.omnibox_changed_keep_edit(window, cx);
        // The words searched in another scope: what they find there shows
        // now, as a switch's does, never the last scope's.
        if typed
            && self
                .ui()
                .omni
                .as_ref()
                .is_some_and(|o| o.depth == Depth::Below)
        {
            self.show_live(window, cx);
        }
    }

    /// The field's text changed: the dropdown follows, the folder typed is
    /// read, the sources are asked, and `>` goes to the palette.
    pub fn omnibox_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(omni) = self.ui_mut().omni.as_mut() {
            omni.edited = true;
        }
        self.omnibox_changed_keep_edit(window, cx);
    }

    fn omnibox_changed_keep_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.omni_text(cx);
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
        let edited = self.ui().omni.as_ref().is_some_and(|o| o.edited);
        let typed = self.typed_of(&text, edited);
        let words = text.trim().to_owned();
        self.filter_as_typed(if typed == Typed::Words { &words } else { "" });
        let omni = self.ui_mut().omni.get_or_insert_with(Omni::default);
        omni.highlighted = None;
        omni.chosen = false;
        omni.token = false;
        omni.held = None;
        if typed == Typed::Scope {
            omni.scopes = false;
        }
        self.ask_suggestions(window, cx);
        self.ask_completions(text.trim_start(), window, cx);
        self.refresh_rows(cx);
        self.schedule_live(window, cx);
    }

    /// Words typed over a folder (not a search: its text is the query),
    /// looking in it alone, show only what they keep, at once, in memory
    /// (the core's `filter`, read by the query plugin); looking below it,
    /// they filter nothing.
    fn filter_as_typed(&mut self, words: &str) {
        let pane = self.pane;
        let filters = self
            .app
            .pane_location(pane)
            .is_some_and(|l| !self.app.is_search(&l));
        if !filters {
            return;
        }
        let here = self
            .ui()
            .omni
            .as_ref()
            .is_none_or(|o| o.depth == Depth::Here);
        // The lens kept beside the words, they after it (ADR-107).
        let kept = self.omni_kept().unwrap_or_default();
        let text = format!("{kept} {words}");
        let text = text.trim();
        let wanted = (here && !text.is_empty()).then_some(text);
        let current = self
            .app
            .workspace
            .read()
            .pane(pane)
            .and_then(|p| p.filter().map(str::to_owned));
        if current.as_deref() != wanted {
            self.set_filter(wanted);
            self.ask_filter_metadata();
        }
    }

    /// Show only what `text` keeps of the active pane, or all. Dispatched
    /// directly: what is typed is not traced.
    pub(crate) fn set_filter(&mut self, text: Option<&str>) {
        let pane = self.pane;
        if let Err(error) = self.app.dispatch(&Invocation::with_args(
            "filter",
            &serde_json::json!({ "text": text, "pane": pane }),
        )) {
            tracing::warn!("filter: {error:#}");
        }
    }

    /// A filter that asks what the files' metadata says (a kind, a size, a
    /// tag) has every file's asked for, as a narrowing does: until it
    /// comes, a file waits hidden.
    ///
    /// Asked once per listing and names: what the files lack does not
    /// change with the words typed, and every key walked the whole listing.
    fn ask_filter_metadata(&mut self) {
        let (asked, unknown): ((u64, u64), Vec<skwir_core::EntryId>) = {
            let ws = self.app.workspace.read();
            let Some(pane) = ws.pane(self.pane) else {
                return;
            };
            if !pane.read_filter().is_some_and(|r| r.metadata) {
                return;
            }
            let asked = (pane.listing.id(), pane.listing.names_version());
            if self.ui().filter_hydrated == Some(asked) {
                return;
            }
            (
                asked,
                pane.listing
                    .iter()
                    .filter(|e| !e.is_dir() && !e.metadata.is_hydrated())
                    .map(|e| e.id)
                    .collect(),
            )
        };
        self.ui_mut().filter_hydrated = Some(asked);
        if !unknown.is_empty() {
            self.request_hydrate_off_screen(unknown);
        }
    }

    /// What the sources are asked for the field's text as it is.
    /// The words of the pane's lens the field keeps beside its text: begun
    /// without them (Ctrl+L, Ctrl+J), words typed add to the lens.
    fn omni_kept(&self) -> Option<String> {
        self.ui()
            .omni
            .as_ref()
            .and_then(|o| o.before.as_ref())
            .and_then(|b| b.kept.clone())
    }

    fn ask_now(&self, cx: &gpui::App) -> Ask {
        let text = self.omni_text(cx);
        let input = self.ui().address.read(cx);
        let caret = input.caret().min(text.len());
        let omni = self.ui().omni.as_ref();
        let edited = omni.is_some_and(|o| o.edited);
        Ask {
            typed: self.typed_of(&text, edited),
            text,
            caret,
            scope: omni.and_then(|o| o.scope.clone()),
            shown: self.app.pane_location(self.pane),
            dir: self.app.pane_dir(self.pane).ok(),
            pane: Some(self.pane),
            lens: self.omni_kept(),
        }
    }

    /// Ask every source that answers what the text is, each in a job of its
    /// own in its lane; the jobs asked before stop. An answer is kept by
    /// source when it comes, and the rows are made again.
    fn ask_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ask = self.ask_now(cx);
        let pane = self.pane;
        let cancel = self.app.lifetime().child();
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return;
        };
        if let Some((_, before)) = omni.asked.take() {
            before.cancel();
        }
        omni.asked = Some((ask.clone(), cancel.clone()));
        let sources: Vec<_> = self
            .app
            .registry
            .suggesters
            .all()
            .into_iter()
            .filter(|s| s.0.answers(&ask))
            .collect();
        // A source no longer answering what is typed leaves nothing behind;
        // the others' rows stay until their answers come.
        let answering: Vec<String> = sources.iter().map(|s| s.0.id().to_owned()).collect();
        if let Some(omni) = self.ui_mut().omni.as_mut() {
            omni.answers.retain(|(id, ..)| answering.contains(id));
            omni.waiting = answering;
        }
        for source in sources {
            let id = source.0.id().to_owned();
            let lane = source.0.lane(&ask);
            let app = Arc::clone(&self.app);
            let asked = ask.clone();
            let stop = cancel.child();
            let worker_stop = stop.clone();
            let answered = (ask.text.clone(), ask.scope.clone());
            self.off_thread_until(
                lane,
                "address-suggest",
                stop,
                move || source.0.suggest(&app, &asked, &worker_stop),
                move |shell, answer, _, cx| {
                    let Some(omni) = shell.pane_ui_mut(pane).and_then(|ui| ui.omni.as_mut()) else {
                        return;
                    };
                    // Asked for a text since moved on: the next answer is
                    // what counts.
                    if omni.asked.as_ref().is_none_or(|(a, _)| {
                        (a.text.as_str(), &a.scope) != (&*answered.0, &answered.1)
                    }) {
                        return;
                    }
                    omni.waiting.retain(|source| *source != id);
                    omni.answers.retain(|(source, ..)| *source != id);
                    omni.answers.push((id, answered, answer));
                    shell.with_pane(pane, |shell| shell.refresh_rows(cx));
                },
                window,
                cx,
            );
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
                skwir_core::fs::reach::lane(&folder),
                "address-complete",
                cancel,
                move || complete::folders(&app, &asked, show_hidden, &stop),
                move |shell, resolved, window, cx| {
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
                    shell.with_pane(pane, |shell| {
                        shell.refresh_rows(cx);
                        shell.release_held(window, cx);
                    });
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
                Lane::SHARED,
                "address-complete-place",
                cancel,
                move || provider.complete(&rest),
                move |shell, completions, window, cx| {
                    let Some(omni) = shell.pane_ui_mut(pane).and_then(|ui| ui.omni.as_mut()) else {
                        return;
                    };
                    if omni.offering.as_ref().is_none_or(|(t, _)| *t != asked) {
                        return;
                    }
                    omni.offering = None;
                    omni.offered = Some((asked, completions));
                    shell.with_pane(pane, |shell| {
                        shell.refresh_rows(cx);
                        shell.release_held(window, cx);
                    });
                },
                window,
                cx,
            );
        }
    }

    /// Work out the dropdown's rows for the text as it is, what Tab fills
    /// in, what is drawn after the text and how the text is drawn; the
    /// field is asked to draw. A row the arrows chose stays chosen.
    fn refresh_rows(&mut self, cx: &mut Context<Self>) {
        let text = self.omni_text(cx);
        let (rows, shared) = self.omni_rows(&text);
        let one_shown = self.one_shown(text.trim());
        let typed = self.typed_of(&text, self.ui().omni.as_ref().is_some_and(|o| o.edited));
        let spans = self.fresh_spans(&text);
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return;
        };
        let kept = omni
            .highlighted
            .filter(|_| omni.chosen)
            .and_then(|i| omni.rows.get(i))
            .map(Row::key);
        // Words show what they keep as typed: Enter keeps it, unless the
        // first row says what Enter does instead (a search that waits for
        // it, or why it cannot run).
        let first = match typed {
            _ if omni.scopes => (!rows.is_empty()).then_some(0),
            Typed::Nothing => None,
            Typed::Words => rows
                .first()
                .filter(|r| r.section() == Section::Run)
                .map(|_| 0),
            _ => (!rows.is_empty()).then_some(0),
        };
        omni.highlighted = match &kept {
            Some(key) => rows.iter().position(|r| r.key() == *key),
            None => first,
        };
        if kept.is_some() && omni.highlighted.is_none() {
            omni.chosen = false;
            omni.highlighted = first;
        }
        omni.rows = rows;
        omni.shared = shared;
        omni.one_shown = one_shown;
        self.show_rest(cx);
        if let Some(spans) = spans {
            self.draw_spans(&text, spans, cx);
        }
        cx.notify();
    }

    /// The spans a source read in `text`, from an answer to this text; none
    /// once every source answered it without any. `None` while one has
    /// still to answer: the field keeps the spans the edit left standing.
    fn fresh_spans(&self, text: &str) -> Option<Vec<Span>> {
        let omni = self.ui().omni.as_ref()?;
        match omni
            .answers
            .iter()
            .find(|(_, (asked, _), answer)| asked == text && !answer.spans.is_empty())
        {
            Some((.., answer)) => Some(answer.spans.clone()),
            None => omni.answered(text).then(Vec::new),
        }
    }

    /// Draw `text` with `spans`.
    fn draw_spans(&mut self, text: &str, spans: Vec<Span>, cx: &mut Context<Self>) {
        let theme = cx.global::<Theme>().clone();
        let styles = spans
            .into_iter()
            .filter(|s| s.range.end <= text.len())
            .filter_map(|s| paint(s.kind, &theme).map(|p| (s.range, p)))
            .collect();
        let address = self.ui().address.clone();
        address.update(cx, |input, cx| {
            if input.text() == text {
                input.set_styles(styles, cx);
            }
        });
    }

    /// Draw after the text what the highlighted row adds to it.
    fn show_rest(&mut self, cx: &mut Context<Self>) {
        let rest = self
            .ui()
            .omni
            .as_ref()
            .filter(|o| o.edited && !o.scopes)
            .and_then(|o| o.highlighted.and_then(|i| o.rows.get(i)))
            .and_then(Row::rest)
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

    /// Up or Down in the dropdown (`ui.list_move`). On the selected token,
    /// Down lists the scopes.
    pub fn omnibox_move(&mut self, mv: crate::modal::ListMove, cx: &mut Context<Self>) {
        let count = self.suggestions().len();
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return;
        };
        omni.held = None;
        if omni.token && forward(mv) {
            omni.token = false;
            omni.scopes = true;
            omni.highlighted = Some(0);
            omni.chosen = true;
            self.refresh_rows(cx);
            return;
        }
        if count > 0 {
            omni.highlighted = Some(match omni.highlighted {
                Some(at) => mv.from(at, count),
                // Nothing highlighted yet: Down reaches the first, Up the last.
                None if forward(mv) => 0,
                None => count - 1,
            });
            omni.chosen = true;
            self.show_rest(cx);
            cx.notify();
        }
    }

    /// Alt+Down (`ui.omnibox_scopes`), a click on the token: list where
    /// words could search, or close the list.
    pub fn omnibox_scopes(&mut self, cx: &mut Context<Self>) {
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return;
        };
        omni.scopes = !omni.scopes;
        omni.token = false;
        omni.chosen = omni.scopes;
        omni.highlighted = omni.scopes.then_some(0);
        self.refresh_rows(cx);
    }

    /// Backspace or Left with the caret at the start (`AtStart`): Left
    /// selects the token, Backspace takes it away (from the token selected
    /// too).
    pub fn omnibox_at_start(&mut self, at: AtStart, window: &mut Window, cx: &mut Context<Self>) {
        let Some(omni) = self.ui_mut().omni.as_mut().filter(|o| o.scope.is_some()) else {
            return;
        };
        match at {
            AtStart::Left => {
                omni.token = true;
                cx.notify();
            }
            AtStart::Backspace => {
                omni.scope = None;
                omni.token = false;
                omni.scopes = false;
                self.scope_changed(false, window, cx);
            }
        }
    }

    /// Tab (`ui.omnibox_complete`): a row chosen with the arrows is taken,
    /// or what is drawn after the text; else words that leave one name
    /// shown become its path; else the part
    /// the names typed share is filled in, else the highlighted completion
    /// is taken, else the first is highlighted. A completion taken writes
    /// the text it makes, as an edit Ctrl+Z takes back: a folder ends with
    /// its separator, and its names are listed in turn. Pressed before
    /// every source answered the text, it asks them first; while the names
    /// a path or a place goes on with are read, it waits for them.
    pub fn omnibox_complete(&mut self, cx: &mut Context<Self>) {
        if self.hold(Held::Complete, cx) {
            return;
        }
        self.settle_rows(cx);
        let text = self.omni_text(cx);
        let Some(omni) = self.ui().omni.as_ref() else {
            return;
        };
        let row = omni.highlighted.and_then(|i| omni.rows.get(i));
        // A row chosen with the arrows is what is meant, and so is what a
        // source draws after the text (the value of a term being typed); a
        // path's names keep ADR-070's order below.
        if let Some(row) = row.filter(|r| {
            r.completion().is_some()
                && (omni.chosen || (matches!(r, Row::Source(_)) && r.rest().is_some()))
        }) {
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
                        omni.highlighted = Some(first);
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
            .and_then(|o| o.highlighted.and_then(|i| o.rows.get(i)))
            .filter(|r| r.rest().is_some())
            .cloned();
        if let Some(row) = row {
            self.take_row(&row, cx);
        }
    }

    /// Shift+Delete (`ui.omnibox_forget`): the highlighted row is
    /// forgotten where it can be (a recent search), and the sources asked
    /// again.
    pub fn omnibox_forget(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let forget = self
            .ui()
            .omni
            .as_ref()
            .and_then(|o| o.highlighted.and_then(|i| o.rows.get(i)))
            .and_then(|r| match r {
                Row::Source(s) => s.forget.clone(),
                _ => None,
            });
        if let Some(forget) = forget {
            if let Err(error) = self.app.dispatch(&forget) {
                tracing::warn!("{}: {error:#}", forget.name);
            }
            self.ask_suggestions(window, cx);
            self.refresh_rows(cx);
        }
    }

    /// Write the completion `row` makes.
    fn take_row(&mut self, row: &Row, cx: &mut Context<Self>) {
        match row {
            Row::Name { text, path, .. } => {
                self.take_completion(text.clone(), Some(path.clone()), cx);
            }
            Row::Offered { text, .. } => self.take_completion(text.clone(), None, cx),
            Row::Source(s) => {
                if let Some(text) = &s.completion {
                    self.take_completion(text.clone(), None, cx);
                }
            }
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

    /// Enter (the row highlighted), Alt+Enter (`everywhere`: the search in
    /// the home folder) or Shift+Enter (`open`: a file named is opened,
    /// not shown in its folder). Words with no row chosen keep what they
    /// show, and the files take the keyboard (ADR-098): the folder
    /// filtered, or the search below it, run now when it waited for Enter.
    /// Text not edited does what it says: the location shown goes there
    /// again. Pressed before every source answered the text, it asks them
    /// first; while the names a path or a place goes on with are read, it
    /// waits for them: what Enter runs is the text's.
    pub fn omnibox_submit(
        &mut self,
        everywhere: bool,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.hold(Held::Submit { everywhere, open }, cx) {
            return;
        }
        self.settle_rows(cx);
        self.submit_shown(everywhere, open, window, cx);
    }

    /// What Enter does with the rows as they are shown: a row clicked runs
    /// as it reads.
    fn submit_shown(
        &mut self,
        everywhere: bool,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let raw = self.omni_text(cx);
        let text = raw.trim().to_owned();
        let (edited, highlighted, depth, scoped) = self
            .ui()
            .omni
            .as_ref()
            .map_or((false, None, Depth::Here, false), |o| {
                (o.edited, o.highlighted, o.depth, o.scope.is_some())
            });
        let typed = self.typed_of(&raw, edited);
        let on_run = highlighted
            .and_then(|i| self.suggestions().get(i))
            .is_some_and(|r| r.section() == Section::Run);
        // Words, or a search's words as given, with nothing chosen but what
        // Enter does.
        let words =
            typed == Typed::Words || (typed == Typed::Nothing && scoped && !text.is_empty());
        if words && (highlighted.is_none() || on_run) && !everywhere && !open {
            if depth == Depth::Below && !self.keep_search(&raw, window, cx) {
                return;
            }
            self.omnibox_end(cx);
            window.focus(&self.ui().focus, cx);
            cx.notify();
            return;
        }
        let rows = self.suggestions().to_vec();
        let chosen = if everywhere {
            // The search Enter runs, hidden while it shows as typed.
            self.run_row(&raw, cx)
                .filter(|s| s.alt.is_some())
                .map(|s| Row::Source(Box::new(s)))
        } else if edited && highlighted.is_none() && typed != Typed::Nothing {
            rows.first().cloned()
        } else {
            highlighted.and_then(|i| rows.get(i)).cloned()
        };
        // A scope chosen, or a row that only writes text, keeps the field.
        match &chosen {
            Some(Row::Help { example, .. }) => {
                let example = example.clone();
                let address = self.ui().address.clone();
                address.update(cx, |input, cx| input.replace_all(&example, cx));
                return;
            }
            Some(Row::Scope { location, .. }) => {
                let location = location.clone();
                if let Some(omni) = self.ui_mut().omni.as_mut() {
                    omni.scope = Some(location);
                    omni.scopes = false;
                }
                let clear = typed == Typed::Scope;
                return self.scope_changed(clear, window, cx);
            }
            // Over the folder alone, a term completed or the words read as
            // terms filter it: Enter writes them, as Tab does.
            Some(Row::Source(s))
                if s.run.is_none()
                    || (depth == Depth::Here
                        && matches!(s.section, Section::Complete | Section::Reading)
                        && s.completion.is_some()) =>
            {
                if let Some(text) = s.completion.clone() {
                    let address = self.ui().address.clone();
                    address.update(cx, |input, cx| input.replace_all(&text, cx));
                } else {
                    // A search that cannot run says why: its row stays.
                    self.set_status(s.title.clone(), true, cx);
                }
                return;
            }
            _ => {}
        }
        let pane = self.pane;
        let named = complete::is_path(&text)
            .then(|| self.typed_folder(raw.trim_start()))
            .flatten();
        let filter = self
            .ui()
            .omni
            .as_ref()
            .and_then(|o| o.before.as_ref())
            .and_then(|b| b.filter.clone());
        // A row goes from what the pane showed: the search shown as typed
        // was the field's, never a step of its own.
        self.take_back(cx);
        self.omnibox_end(cx);
        window.focus(&self.ui().focus, cx);
        match chosen {
            // Nothing typed, nothing chosen: what the text says, the
            // location shown again. A row offered before anything is typed
            // (a place one goes to, a tab, a recent search) runs below.
            None | Some(Row::Go) if text.is_empty() => {
                self.set_filter(None);
                let shown = self.ui().address_shown.clone();
                let address = self.ui().address.clone();
                address.update(cx, |input, cx| input.set_text(shown, cx));
            }
            None | Some(Row::Go) => match named {
                Some(path) if open => self.open_path(pane, path, window, cx),
                Some(path) => {
                    self.reveal_path(pane, &path, window, cx);
                }
                // A path read from nothing shown (`../` over a search, a
                // tag, the trash) is never the process's working folder's.
                None if complete::is_path(&text) => {
                    let why = if text.starts_with('~') {
                        "The home folder is not known".to_owned()
                    } else {
                        format!("“{text}” starts from the folder shown, and no folder is shown")
                    };
                    self.set_status(why, true, cx);
                }
                None => self.run(
                    "navigate",
                    serde_json::json!({ "location": text, "pane": pane }),
                    window,
                    cx,
                ),
            },
            Some(Row::Name {
                path, dir: true, ..
            }) => {
                self.run(
                    "navigate",
                    serde_json::json!({ "location": PathArg::from(path), "pane": pane }),
                    window,
                    cx,
                );
            }
            Some(Row::Name {
                path, dir: false, ..
            }) if open => self.open_path(pane, path, window, cx),
            Some(Row::Name { path, .. }) => {
                self.reveal_path(pane, &path, window, cx);
            }
            Some(Row::Offered { location, .. }) => {
                self.run(
                    "navigate",
                    serde_json::json!({ "location": location.arg(), "pane": pane }),
                    window,
                    cx,
                );
            }
            Some(Row::Source(s)) => {
                let run = if everywhere {
                    s.alt.clone()
                } else if open {
                    s.open.clone().or(s.run.clone())
                } else {
                    s.run.clone()
                };
                // It runs over the pane as it was, filtered as it was (as
                // Escape puts it back): the words typed were the field's.
                // A row that goes elsewhere leaves the filter behind.
                self.set_filter(filter.as_deref());
                if let Some(run) = run {
                    self.run(&run.name, run.args, window, cx);
                }
            }
            Some(Row::Scope { .. } | Row::Help { .. }) => {}
        }
        cx.notify();
    }

    /// Enter on words looking below the folder: the search shown as they
    /// were typed is kept, else shown now (or run, when it waited for
    /// Enter), remembered as Enter's searches are. False when it cannot
    /// run: its row says why, and the field stays.
    fn keep_search(&mut self, raw: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(row) = self.run_row(raw, cx) else {
            return true;
        };
        let Some(run) = row.run.clone() else {
            self.set_status(row.title.clone(), true, cx);
            return false;
        };
        let words = raw.trim();
        let pane = self.pane;
        let over_search = self
            .app
            .pane_location(pane)
            .is_some_and(|l| self.app.is_search(&l));
        let Some(omni) = self.ui_mut().omni.as_mut() else {
            return true;
        };
        omni.pending = None;
        let shown = omni
            .shown
            .as_ref()
            .is_some_and(|(shown, scope)| shown == words && *scope == omni.scope)
            || (!omni.edited && over_search);
        if !shown {
            match row.live {
                Some(live) => {
                    self.step(cx);
                    if let Err(error) = self.app.dispatch(&live) {
                        tracing::warn!("{}: {error:#}", live.name);
                    }
                }
                // It goes from what the pane showed.
                None => self.take_back(cx),
            }
        }
        // A search the pane shows already stays as it is: it is remembered.
        self.run(&run.name, run.args, window, cx);
        true
    }

    /// Whether an answer to `text`, in the scope as it is, holds what Enter
    /// runs with words.
    fn has_fresh_run(&self, text: &str) -> bool {
        self.ui().omni.as_ref().is_some_and(|o| {
            o.answers.iter().any(|(_, (asked, scope), a)| {
                asked == text
                    && *scope == o.scope
                    && a.rows.iter().any(|r| r.section == Section::Run)
            })
        })
    }

    /// Every source asked `ask` here and now, its answer kept as its job's
    /// would have been: a key pressed before their jobs answered. Sources
    /// answer from memory.
    fn answer_here(&mut self, ask: &Ask) {
        let cancel = CancelToken::new();
        let answers = skwir_core::suggest::each(&self.app, ask, &cancel);
        let asked = (ask.text.clone(), ask.scope.clone());
        if let Some(omni) = self.ui_mut().omni.as_mut() {
            omni.answers = answers
                .into_iter()
                .map(|(id, answer)| (id, asked.clone(), answer))
                .collect();
            omni.waiting.clear();
        }
    }

    /// Make the rows the text's own before a key takes one (Enter, Tab):
    /// while a source has yet to answer the text, they are all asked here
    /// what their jobs were (`answer_here`). The row the arrows chose stays
    /// chosen.
    fn settle_rows(&mut self, cx: &mut Context<Self>) {
        let ask = self.ask_now(cx);
        if self
            .ui()
            .omni
            .as_ref()
            .is_none_or(|omni| omni.answered(&ask.text))
        {
            return;
        }
        self.answer_here(&ask);
        self.refresh_rows(cx);
    }

    /// Whether the names `text` goes on with are still being read, on a
    /// worker: the folder a path's folder part makes, or what a place's
    /// provider completes it with. Enter and Tab choose among them.
    fn completing(&self, text: &str) -> bool {
        let Some(omni) = self.ui().omni.as_ref() else {
            return false;
        };
        let text = text.trim_start();
        match self.typed_of(text, omni.edited) {
            Typed::Path => {
                text != "~"
                    && self
                        .typed_folder(complete::split(text).0)
                        .is_some_and(|folder| matches!(omni.folders.get(&folder), Some(None)))
            }
            Typed::Place => omni
                .offering
                .as_ref()
                .is_some_and(|(asked, _)| asked == text),
            _ => false,
        }
    }

    /// Hold `key` while the names the text goes on with are read: true
    /// when it is held, to be done once they come (`release_held`).
    fn hold(&mut self, key: Held, cx: &mut Context<Self>) -> bool {
        let text = self.omni_text(cx);
        if !self.completing(&text) {
            return false;
        }
        if let Some(omni) = self.ui_mut().omni.as_mut() {
            omni.held = Some(key);
        }
        true
    }

    /// The names the text goes on with came: the key held for them is
    /// done. A Tab that then finds nothing to complete keeps the field.
    fn release_held(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.omni_text(cx);
        if self.completing(&text) {
            return;
        }
        match self
            .ui_mut()
            .omni
            .as_mut()
            .and_then(|omni| omni.held.take())
        {
            Some(Held::Submit { everywhere, open }) => {
                self.omnibox_submit(everywhere, open, window, cx);
            }
            Some(Held::Complete) => self.omnibox_complete(cx),
            None => {}
        }
    }

    /// Shift+Enter on a path: a folder is gone to, a file opened as Enter
    /// opens it in the files (`open`). Whether it is a folder, or there,
    /// is asked on a worker, in its lane, for the pane as it is: shown
    /// elsewhere since, it is left.
    fn open_path(
        &mut self,
        pane: skwir_core::workspace::PaneId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked = path.clone();
        let (cancel, shown) = self.pane_lifetime(pane);
        self.off_thread_until(
            skwir_core::fs::reach::lane(&path),
            "address-open",
            cancel,
            move || std::fs::metadata(&asked).map(|m| m.is_dir()),
            move |shell, is_dir, window, cx| match is_dir {
                _ if shell.app.pane_location(pane) != shown => {}
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

    /// Pane `pane`'s field lost the keyboard while typed in (a click on a
    /// place or the files, Tab, F6): it lets go. What its words show stays,
    /// as Enter keeps it (the folder filtered, the search shown as typed,
    /// its step kept), the search waiting for the pause is dropped, and the
    /// field shows where the pane is again. A state left behind would act
    /// later on whatever the pane shows by then.
    pub(crate) fn omnibox_left(
        &mut self,
        pane: skwir_core::workspace::PaneId,
        cx: &mut Context<Self>,
    ) {
        let Some(ui) = self.pane_ui_mut(pane) else {
            return;
        };
        if ui.omni.take().is_none() {
            return;
        }
        let (address, shown) = (ui.address.clone(), ui.address_shown.clone());
        address.update(cx, |input, cx| {
            input.set_hint(None, cx);
            input.set_styles(Vec::new(), cx);
            input.send_start(false);
            input.set_text(shown, cx);
        });
        cx.notify();
    }

    /// Escape: the list of scopes closes; else the pane shows what it
    /// showed when the field took the keyboard, filtered as it was, and the
    /// field shows where it is again, all of it.
    pub fn omnibox_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(omni) = self.ui_mut().omni.as_mut()
            && (omni.scopes || omni.token)
        {
            omni.scopes = false;
            omni.token = false;
            self.refresh_rows(cx);
            return;
        }
        let filter = self
            .ui()
            .omni
            .as_ref()
            .and_then(|o| o.before.as_ref())
            .and_then(|b| b.filter.clone());
        self.take_back(cx);
        self.omnibox_end(cx);
        self.set_filter(filter.as_deref());
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
        address.update(cx, |input, cx| {
            input.set_hint(None, cx);
            input.set_styles(Vec::new(), cx);
            input.send_start(false);
        });
    }

    /// Whether the field's card is drawn: it is typed in and has rows. The
    /// field's text then draws above the card (`address::pane_row`).
    pub fn omnibox_card(&self) -> bool {
        self.ui().omni.is_some() && !self.suggestions().is_empty()
    }

    /// The dropdown's rows for the active pane's text, first the one Enter
    /// runs.
    fn suggestions(&self) -> &[Row] {
        self.ui().omni.as_ref().map_or(&[], |o| o.rows.as_slice())
    }

    /// The active pane's field's text as it is read: a `file://` URI typed
    /// or pasted (from a browser, a terminal's link) as the path it names
    /// here, everything else as written. The field keeps what was typed.
    fn omni_text(&self, cx: &gpui::App) -> String {
        let text = self.ui().address.read(cx).text();
        uri_path(text).unwrap_or_else(|| text.to_owned())
    }

    /// Whether Tab completes the active pane's text (the `completes` key
    /// context): a completion is offered, or words leave one name shown, or
    /// the names a path or a place goes on with are still read (Tab waits
    /// for them); before anything is typed, the row the arrows reached
    /// writes one (a term to add).
    pub fn omnibox_completes(&self, cx: &gpui::App) -> bool {
        let Some(omni) = self.ui().omni.as_ref() else {
            return false;
        };
        if !omni.edited {
            return omni
                .highlighted
                .filter(|_| omni.chosen)
                .and_then(|i| omni.rows.get(i))
                .is_some_and(|r| r.completion().is_some());
        }
        let text = self.omni_text(cx);
        omni.one_shown.is_some()
            || omni.rows.iter().any(|r| r.completion().is_some())
            || matches!(text.trim_start(), "~" | "." | "..")
            || self.completing(&text)
    }

    /// What `text` offers, first what Enter runs, and what Tab fills in.
    fn omni_rows(&self, text: &str) -> (Vec<Row>, Option<String>) {
        let Some(omni) = self.ui().omni.as_ref() else {
            return (Vec::new(), None);
        };
        let typed = self.typed_of(text, omni.edited);
        let scope = omni.scope.clone();
        // The search Enter runs with words is no row while they show what
        // they keep as typed: over the folder alone, or below it as typed.
        // It is one while it waits for Enter, or cannot run.
        let quiet = |r: &Suggestion| {
            typed == Typed::Words
                && r.section == Section::Run
                && !r.error
                && (omni.depth == Depth::Here || r.live.is_some())
        };
        // Each source's rows, until its answer to this text comes: a key
        // never takes rows away before the next answer, so the card neither
        // closes nor jumps between keys. A row answering an earlier text
        // draws after this one only what it still adds; Enter and Tab ask
        // the sources first (`settle_rows`).
        let sources: Vec<Row> = {
            let fresh = |asked: &(String, Option<Location>)| asked.0 == text && asked.1 == scope;
            let answers = omni.answers.iter().map(|(id, asked, answer)| {
                let keep = fresh(asked);
                let rows = answer
                    .rows
                    .iter()
                    .cloned()
                    .map(|mut row| {
                        if !keep {
                            row.rest = rest_after(&row, text);
                        }
                        row
                    })
                    .collect();
                (
                    id.clone(),
                    Answer {
                        rows,
                        spans: Vec::new(),
                    },
                )
            });
            skwir_core::suggest::merge(answers)
                .rows
                .into_iter()
                .filter(|row| !quiet(row))
                .map(|mut row| {
                    if row.section == Section::Actions && row.keys.is_none() {
                        row.keys = row
                            .run
                            .as_ref()
                            .and_then(|run| self.keys_for(&run.name, &run.args));
                    }
                    Row::Source(Box::new(row))
                })
                .collect()
        };
        if omni.scopes {
            return (self.scope_rows(""), None);
        }
        let typed_text = text.trim_start();
        match typed {
            // Words with no source answering them (the query plugin off,
            // its answer not in yet) offer no "Go to": Enter keeps what
            // they show, as with any words.
            Typed::Nothing | Typed::Words => {
                let mut rows = sources;
                if typed == Typed::Words {
                    rows.extend(self.scheme_rows(text.trim()));
                }
                (rows, None)
            }
            Typed::Scope => {
                let mut rows = self.scope_rows(typed_text.trim_start_matches('@').trim());
                rows.extend(sources);
                (rows, None)
            }
            Typed::Help => {
                let mut rows = self.help_rows();
                rows.extend(sources);
                (rows, None)
            }
            Typed::Path => {
                let (mut rows, shared) = self.path_rows(typed_text);
                rows.extend(sources);
                (rows, shared)
            }
            Typed::Place => {
                let scheme = scheme_of(typed_text).unwrap_or_default();
                let (mut rows, shared) = self.place_rows(typed_text, scheme);
                rows.extend(sources);
                (rows, shared)
            }
        }
    }

    /// What the field takes, each with an example Enter writes: a path, a
    /// place's address (the schemes providers open), a scope, words, a
    /// command. The terms of a search are the query plugin's rows.
    fn help_rows(&self) -> Vec<Row> {
        let home = if cfg!(windows) { "~\\" } else { "~/" };
        // The places the sidebar shows by address first (the trash, Recent,
        // tags), then any other scheme a provider opens.
        let mut schemes: Vec<String> = Vec::new();
        let shown = self
            .app
            .registry
            .places
            .all()
            .into_iter()
            .map(|place| place.location.scheme.to_string());
        let opened = self.app.schemes().into_iter().map(|s| s.to_string());
        for scheme in shown.chain(opened) {
            let address = format!("{scheme}:");
            let searches = self
                .app
                .provider(&scheme)
                .is_none_or(|p| p.searches(&Location::new(&scheme, "")));
            if scheme != PATH_SCHEME && !searches && !schemes.contains(&address) {
                schemes.push(address);
            }
        }
        let mut rows = vec![Row::Help {
            label: format!("{home}   /   ./"),
            detail: "A path; Tab completes its names".into(),
            icon: "folder",
            example: home.into(),
        }];
        if let Some(first) = schemes.first().cloned() {
            rows.push(Row::Help {
                label: schemes
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("   "),
                detail: "A place, by its address".into(),
                icon: "open",
                example: first,
            });
        }
        rows.extend([
            Row::Help {
                label: "@".into(),
                detail: "Where words search: the folder above, home, a tag…".into(),
                icon: "layers",
                example: "@".into(),
            },
            Row::Help {
                label: "words".into(),
                detail: "Search where the scope says; they filter the folder at once".into(),
                icon: "search",
                example: String::new(),
            },
            Row::Help {
                label: ">".into(),
                detail: "A command, by its name, in the palette".into(),
                icon: "command",
                example: ">".into(),
            },
        ]);
        rows
    }

    /// Where words could search, those whose names go with `words`: the
    /// folder shown and the one above, the home folder, the whole disk,
    /// the sidebar's folders, tags and Recent.
    fn scope_rows(&self, words: &str) -> Vec<Row> {
        let mut found: Vec<(i32, Row)> = Vec::new();
        let mut add = |location: Location, detail: Option<String>, rank: i32| {
            let label = scope_name(&self.app, &location);
            let at = if words.is_empty() {
                Some(rank)
            } else {
                crate::suggestions::rank_of(&label, words).map(|r| r * 100 + rank)
            };
            if let Some(at) = at
                && !found
                    .iter()
                    .any(|(_, r)| matches!(r, Row::Scope { location: l, .. } if *l == location))
            {
                found.push((
                    at,
                    Row::Scope {
                        icon: scope_glyph(&self.app, &location),
                        label,
                        detail,
                        location,
                    },
                ));
            }
        };
        let shown = self.app.pane_dir(self.pane).ok();
        if let Some(dir) = &shown {
            add(Location::from_path(dir), Some("The folder shown".into()), 0);
            if let Some(up) = dir.parent() {
                add(Location::from_path(up), Some("The folder above".into()), 1);
            }
        }
        if let Some(home) = home() {
            add(Location::from_path(home), None, 2);
        }
        add(
            Location::from_path(PathBuf::from(std::path::MAIN_SEPARATOR_STR)),
            Some("Every folder, by the system's index where it has one".into()),
            3,
        );
        let mut rank = 10;
        for place in self.app.registry.places.all() {
            // A folder, a tag, Recent: where words can search
            // (`App::searchable`), never every tag, the trash or the shelf.
            if place.hidden || !self.app.searchable(&place.location) {
                continue;
            }
            add(
                place.location.clone(),
                Some(crate::suggestions::said(&place.location)),
                rank,
            );
            rank += 1;
        }
        found.sort_by_key(|(rank, _)| *rank);
        found.into_iter().take(SCOPES).map(|(_, row)| row).collect()
    }

    /// A path's rows: the names of the folder typed that go on with the
    /// name typed (or of the folders its names start), the best first,
    /// after Go when no name is typed or none goes on with it. A folder
    /// part that names none, with no name typed after it, offers the
    /// folders it stands for.
    fn path_rows(&self, text: &str) -> (Vec<Row>, Option<String>) {
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
                    Row::Name {
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
                    rows.push(Row::Name {
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
        // `.` and `..` name a folder themselves. A path that names none
        // here (`../` with no folder shown) goes nowhere.
        let goes = self.typed_folder(text).is_some();
        if goes
            && ((typed.is_empty() && !guessed) || rows.is_empty() || matches!(typed, "." | ".."))
        {
            rows.insert(0, Row::Go);
        }
        (rows, shared)
    }

    /// A place's rows (`scheme:rest`): what its provider completes the
    /// name typed with (`LocationProvider::complete`), as a path's names.
    /// Rows asked for an earlier text stay until the answer comes, drawing
    /// nothing after the text.
    fn place_rows(&self, text: &str, scheme: &str) -> (Vec<Row>, Option<String>) {
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
                rows.push(Row::Offered {
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
            rows.insert(0, Row::Go);
        }
        (rows, shared)
    }

    /// The places of the schemes `words` start (`tr` for the trash), as
    /// providers name them; never a search's, whose text is words.
    fn scheme_rows(&self, words: &str) -> Vec<Row> {
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
                (!provider.searches(&location)).then(|| Row::Offered {
                    label: self.app.location_title(&location),
                    text: format!("{scheme}:"),
                    location,
                    rest: None,
                })
            })
            .collect()
    }
}

/// The scope of the pane being rendered, as a token before the text while
/// its field is typed in: pressed, it lists the other scopes; selected
/// (Left at the start), it is drawn as chosen.
pub fn scope_token(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let omni = shell.ui().omni.as_ref()?;
    let scope = omni.scope.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let name = scope_name(&shell.app, scope);
    let pane = shell.pane;
    // How deep words look: a funnel over the folder alone, a magnifier
    // below a folder; a place searched as it lists wears its own glyph.
    let glyph = match omni.depth {
        Depth::Here => "filter",
        Depth::Below if scope.path().is_some() => "search",
        Depth::Below => scope_glyph(&shell.app, scope),
    };
    let (bg, fg) = if omni.token || omni.scopes {
        (theme.selection_bg, theme.selection_text)
    } else {
        (theme.term, theme.accent_text)
    };
    Some(
        div()
            .id(("scope-token", pane.0 as usize))
            .flex_none()
            .ml(px(3.))
            .h(px(22.))
            .pl(px(7.))
            .pr(px(5.))
            .flex()
            .items_center()
            .gap(px(4.))
            .rounded_full()
            .bg(bg)
            .text_xs()
            .text_color(fg)
            .cursor_pointer()
            .when(shell.a11y, |d| {
                d.role(gpui::Role::Button).aria_label(match omni.depth {
                    Depth::Here => format!("Filtering {name}"),
                    Depth::Below => format!("Searching {name} and below"),
                })
            })
            // The field keeps the keyboard: the token acts on the press.
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    this.activate_pane(pane, cx);
                    this.omnibox_scopes(cx);
                }),
            )
            .child(
                gpui::svg()
                    .path(format!("toolbar/{glyph}.svg"))
                    .size(px(13.))
                    .flex_none()
                    .text_color(fg),
            )
            .child(name)
            .child(
                gpui::svg()
                    .path("toolbar/chevron.svg")
                    .size(px(11.))
                    .flex_none()
                    .text_color(fg),
            )
            .into_any_element(),
    )
}

/// What a row draws: its icon (or the tag it stands for), title, detail,
/// whether its text is typed text, and the keys said beside it always.
struct Drawn {
    icon: String,
    tag: Option<skwir_core::metadata::TagRef>,
    title: String,
    detail: Option<String>,
    typed: bool,
    detail_typed: bool,
    keys: Option<String>,
    error: bool,
}

fn drawn(shell: &Shell, row: &Row, text: &str) -> Drawn {
    let plain = |icon: &str, title: String, detail: Option<String>| Drawn {
        icon: icon.to_owned(),
        tag: None,
        title,
        detail,
        typed: false,
        detail_typed: false,
        keys: None,
        error: false,
    };
    match row {
        Row::Go => plain("open", format!("Go to {text}"), None),
        Row::Name {
            dir, label, folder, ..
        } => plain(
            if *dir { "folder" } else { "open" },
            label.clone(),
            folder.clone(),
        ),
        Row::Offered {
            location, label, ..
        } => plain(
            crate::trail::glyph(&shell.app, location),
            label.clone(),
            Some(location.to_string()),
        ),
        Row::Scope {
            icon,
            label,
            detail,
            ..
        } => plain(icon, label.clone(), detail.clone()),
        Row::Help {
            label,
            detail,
            icon,
            ..
        } => Drawn {
            typed: true,
            ..plain(icon, label.clone(), Some(detail.clone()))
        },
        Row::Source(s) => Drawn {
            icon: s.icon.clone().unwrap_or_else(|| "search".to_owned()),
            tag: s.tag.clone(),
            title: s.title.clone(),
            detail: s.detail.clone(),
            typed: s.typed,
            detail_typed: s.detail_typed,
            keys: s.keys.clone(),
            error: s.error,
        },
    }
}

/// What the keys of the highlighted row do, by the keys bound to them
/// (`keys_for`: a scheme or the user may bind others; one bound to none
/// is not said). Enter and Escape are the field's own.
fn keys_of(shell: &Shell, row: &Row) -> String {
    let key = |command: &str, args: serde_json::Value| shell.keys_for(command, &args);
    let and = |keys: Option<String>, does: &str| match keys {
        Some(keys) if does.is_empty() => format!("Enter · {keys}"),
        Some(keys) => format!("Enter · {keys} {does}"),
        None => "Enter".to_owned(),
    };
    let tab = || key("ui.omnibox_complete", serde_json::json!({}));
    match row {
        Row::Help { .. } => and(tab(), "writes it"),
        Row::Name { dir: false, .. } => and(
            key("ui.omnibox_submit", serde_json::json!({ "open": true })),
            "opens",
        ),
        Row::Source(s) if s.error => "Enter waits".to_owned(),
        Row::Source(s) if s.forget.is_some() => and(
            key("ui.omnibox_forget", serde_json::json!({})),
            "forgets it",
        ),
        Row::Source(s) if s.run.is_none() && s.completion.is_some() => and(tab(), ""),
        _ if row.scope(&shell.app).is_some() && !matches!(row, Row::Scope { .. }) => {
            and(key("ui.search", serde_json::json!({})), "searches in it")
        }
        _ if row.completion().is_some() => and(tab(), ""),
        _ => "Enter".to_owned(),
    }
}

/// What the line under the rows says the keys do, for what is typed: the
/// keys bound (`keys_for`), a phrase whose command has none left out.
fn footer(shell: &Shell, typed: Typed, listing_scopes: bool) -> String {
    let omni = shell.ui().omni.as_ref();
    let scoped = omni.is_some_and(|o| o.scope.is_some());
    let here = omni.is_none_or(|o| o.depth == Depth::Here);
    let key = |command: &str, args: serde_json::Value, does: &str| {
        shell
            .keys_for(command, &args)
            .map(|keys| format!("{keys} {does}"))
    };
    let search = |does: &str| key("ui.search", serde_json::json!({}), does);
    let tab = || key("ui.omnibox_complete", serde_json::json!({}), "completes");
    let said = |phrases: Vec<Option<String>>| {
        let phrases: Vec<String> = phrases.into_iter().flatten().collect();
        format!("{}.", phrases.join("; "))
    };
    let plain = |text: &str| Some(text.to_owned());
    // What the folder alone cannot tell: a search's, below it.
    let waits = shell
        .app
        .workspace
        .read()
        .pane(shell.pane)
        .and_then(|p| p.read_filter().map(|r| r.waits.clone()))
        .filter(|w| !w.is_empty() && typed == Typed::Words && here)
        .map(|w| {
            let what = cap(&w.join(" and "));
            match shell.keys_for("ui.search", &serde_json::json!({})) {
                Some(keys) => format!("{what} need a search: {keys}. "),
                None => format!("{what} need a search. "),
            }
        });
    let keys = match typed {
        _ if listing_scopes => "Enter searches there; Escape keeps the scope.".to_owned(),
        Typed::Nothing if scoped && here => {
            "Words filter this folder; Backspace takes the scope away, @ lists others; > for a command."
                .to_owned()
        }
        Typed::Nothing if scoped => {
            "Words search there and below; Backspace takes the scope away, @ lists others; > for a command."
                .to_owned()
        }
        Typed::Nothing => said(vec![
            plain("A path (/ or ~), a place, or words to filter"),
            search("searches below"),
            plain("> for a command"),
        ]),
        Typed::Path | Typed::Place => said(vec![
            tab(),
            key(
                "ui.omnibox_submit",
                serde_json::json!({ "open": true }),
                "opens a file",
            ),
            search("on a folder searches in it"),
        ]),
        Typed::Scope => "Enter searches there.".to_owned(),
        Typed::Help => said(vec![
            plain("Enter writes how it starts"),
            key("ui.omnibox_complete", serde_json::json!({}), "too"),
            plain("Escape goes back"),
        ]),
        Typed::Words if here => said(vec![
            plain("Enter goes to the files"),
            tab(),
            plain("Escape goes back"),
        ]),
        Typed::Words => said(vec![
            plain("Enter goes to the files"),
            tab(),
            key(
                "ui.omnibox_submit",
                serde_json::json!({ "everywhere": true }),
                "searches the home folder",
            ),
            plain("Escape goes back"),
        ]),
    };
    waits.map_or_else(|| keys.clone(), |w| w + &keys)
}

/// `text` with its first letter capital.
fn cap(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// The card's first line while words are typed, or about to be: how deep
/// they look, a switch with each side's keys, and what they keep there
/// (the card hides the top of the list they change).
fn depth_line(shell: &Shell, typed: Typed, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let omni = shell.ui().omni.as_ref()?;
    let words_mode = match typed {
        Typed::Words => true,
        Typed::Nothing => omni.scope.is_some(),
        _ => false,
    };
    if omni.scopes || !words_mode {
        return None;
    }
    let theme = cx.global::<Theme>().clone();
    let pane = shell.pane;
    let a11y = shell.a11y;
    let raw = shell.omni_text(cx);
    let words = raw.trim();
    let say = match omni.depth {
        Depth::Here => {
            let ws = shell.app.workspace.read();
            ws.pane(pane).filter(|p| p.filter().is_some()).map(|p| {
                format!(
                    "As you type: {} of {}",
                    crate::format::thousands(shell.ui().list.sorted.len()),
                    crate::format::count(p.listing.sorted(p.sort).len(), "item")
                )
            })
        }
        Depth::Below if words.is_empty() => None,
        Depth::Below
            if omni
                .shown
                .as_ref()
                .is_some_and(|(shown, scope)| shown == words && *scope == omni.scope) =>
        {
            Some(format!(
                "{} found",
                crate::format::thousands(shell.ui().list.sorted.len())
            ))
        }
        Depth::Below => omni
            .answers
            .iter()
            .filter(|(_, (asked, _), _)| *asked == raw)
            .flat_map(|(.., answer)| answer.rows.iter())
            .find(|row| row.section == Section::Run)
            .and_then(|row| match () {
                () if row.error => None,
                () if row.live.is_some() => Some("As you type".to_owned()),
                () => Some("Enter searches".to_owned()),
            }),
    };
    let segment = |depth: Depth, icon: &str, label: &'static str, command: &str| {
        let on = omni.depth == depth;
        let keys = shell.keys_for(command, &serde_json::json!({}));
        let hover = theme.hover;
        div()
            .id(("omnibox-depth", depth as usize))
            .h(px(24.))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_md()
            .cursor_pointer()
            .when(on, |d| d.bg(theme.term).text_color(theme.accent_text))
            .when(!on, |d| {
                d.text_color(theme.text_muted).hover(move |s| s.bg(hover))
            })
            .when(a11y, |d| {
                d.role(gpui::Role::Button).aria_label(match (depth, on) {
                    (Depth::Here, true) => "This folder only, chosen",
                    (Depth::Here, false) => "This folder only",
                    (Depth::Below, true) => "This folder and its subfolders, chosen",
                    (Depth::Below, false) => "This folder and its subfolders",
                })
            })
            // The field keeps the keyboard: the switch acts on the press.
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    this.activate_pane(pane, cx);
                    this.set_depth(depth, window, cx);
                }),
            )
            .child(
                gpui::svg()
                    .path(format!("toolbar/{icon}.svg"))
                    .size(px(13.))
                    .flex_none()
                    .text_color(if on {
                        theme.accent_text
                    } else {
                        theme.text_muted
                    }),
            )
            .child(label)
            .children(keys.map(|keys| {
                div()
                    .px(px(4.))
                    .rounded_sm()
                    .border_1()
                    .border_color(theme.border)
                    // On `term`, what reads there.
                    .text_color(if on {
                        theme.accent_text
                    } else {
                        theme.text_muted
                    })
                    .child(keys)
            }))
    };
    Some(
        div()
            .px_1()
            .pt_1()
            .pb_1()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .child(
                div()
                    .flex_none()
                    .flex()
                    .p(px(2.))
                    .gap(px(2.))
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .child(segment(Depth::Here, "filter", "This folder", "ui.filter"))
                    .child(segment(
                        Depth::Below,
                        "search",
                        "And subfolders",
                        "ui.search",
                    )),
            )
            .child(div().flex_1())
            .children(say.map(|say| {
                div()
                    .flex_none()
                    .px_1()
                    .text_color(theme.text_muted)
                    .child(say)
            }))
            .into_any_element(),
    )
}

/// The dropdown under the field of the pane being rendered, while it is
/// typed in and something is offered; `anchor` is the field's. It hangs
/// from the field's right end, so the names the typing filters stay in
/// sight at the left. Its rows are a list box beside the field, which
/// keeps the keyboard: the row the arrows reach is its active descendant.
pub fn dropdown(
    shell: &Shell,
    anchor: &PopoverAnchor,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let rows = shell.suggestions();
    if rows.is_empty() {
        return None;
    }
    let omni = shell.ui().omni.as_ref()?;
    let theme = cx.global::<Theme>().clone();
    let text = shell.omni_text(cx).trim().to_owned();
    let typed = shell.typed_of(&text, omni.edited);
    let pane = shell.pane;
    let a11y = shell.a11y;
    let field = gpui::Focusable::focus_handle(shell.ui().address.read(cx), cx);
    let mut list = crate::modal::options("omnibox-rows", "Suggestions", rows.len(), a11y)
        .flex()
        .flex_col();
    let mut section: Option<Section> = None;
    let mut strip: Option<gpui::Div> = None;
    for (i, row) in rows.iter().enumerate() {
        let sec = row.section();
        let first_row = i == 0 && sec == Section::Run;
        if section != Some(sec) {
            if let Some(done) = strip.take() {
                list = list.child(done);
            }
            if let Some(title) = sec.title().filter(|_| !first_row) {
                list = list.child(
                    div()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(title),
                );
            }
            section = Some(sec);
            if sec.is_strip() {
                strip = Some(div().px_2().pb_1().flex().flex_wrap().gap(px(6.)));
            }
        }
        let d = drawn(shell, row, &text);
        let highlighted = omni.highlighted == Some(i);
        let hover = theme.hover;
        let named = a11y.then(|| match &d.detail {
            Some(detail) => format!("{}, {detail}", d.title).into(),
            None => d.title.clone().into(),
        });
        let muted = if highlighted {
            theme.selection_text
        } else {
            theme.text_muted
        };
        let click = cx.listener(
            move |this: &mut Shell, _: &gpui::MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
                this.activate_pane(pane, cx);
                if let Some(omni) = this.ui_mut().omni.as_mut() {
                    omni.highlighted = Some(i);
                    omni.chosen = true;
                }
                this.submit_shown(false, false, window, cx);
            },
        );
        let icon = |color| match &d.tag {
            Some(tag) => {
                let dot = theme.tags[shell.tag_colors.of(&tag.name) % theme.tags.len()];
                div()
                    .size(px(15.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(div().size(px(8.)).rounded_full().bg(dot))
                    .into_any_element()
            }
            None => gpui::svg()
                .path(format!("toolbar/{}.svg", d.icon))
                .size(px(15.))
                .flex_none()
                .text_color(color)
                .into_any_element(),
        };
        let title_font = if d.typed {
            Some(cx.global::<Theme>().code_font.clone())
        } else {
            None
        };
        if let Some(strip_div) = strip.take() {
            let button = crate::modal::option(
                div().id(crate::elements::known_by("omnibox", (&d.title, i))),
                named,
                i,
                highlighted && omni.chosen,
                &field,
            )
            .h(px(26.))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded_full()
            .border_1()
            .border_color(theme.border)
            .cursor_pointer()
            .text_xs()
            .when(highlighted, |b| {
                b.bg(theme.selection_bg)
                    .text_color(theme.selection_text)
                    .border_color(theme.selection_bg)
            })
            .when(!highlighted, |b| b.hover(move |s| s.bg(hover)))
            .on_mouse_down(gpui::MouseButton::Left, click)
            .child(icon(muted))
            .child(d.title.clone())
            .children(
                d.keys
                    .clone()
                    .or_else(|| {
                        matches!(row, Row::Source(s) if s.alt.is_some())
                            .then(|| {
                                shell.keys_for(
                                    "ui.omnibox_submit",
                                    &serde_json::json!({ "everywhere": true }),
                                )
                            })
                            .flatten()
                    })
                    .map(|k| div().text_color(muted).child(k)),
            );
            strip = Some(strip_div.child(button));
            continue;
        }
        let keys = if highlighted {
            Some(keys_of(shell, row))
        } else {
            d.keys.clone()
        };
        let error = d.error;
        list = list.child(
            // Said as focused once the arrows reach it: while typing, the
            // field reads back its text and caret.
            crate::modal::option(
                div().id(crate::elements::known_by(
                    "omnibox",
                    (&d.title, &d.detail, i),
                )),
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
            .when(highlighted, |r| {
                r.bg(theme.selection_bg).text_color(theme.selection_text)
            })
            .when(!highlighted, |r| r.hover(move |s| s.bg(hover)))
            .when(error && !highlighted, |r| r.text_color(theme.error))
            // The field keeps the keyboard: the row acts on the press,
            // before anything under it could take the focus.
            .on_mouse_down(gpui::MouseButton::Left, click)
            .child(icon(if error && !highlighted {
                theme.error
            } else {
                muted
            }))
            .child(crate::elements::clip::clipped(
                div()
                    .id("omni-title")
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis_middle()
                    .when_some(title_font.clone(), gpui::Styled::font_family),
                d.title.clone(),
            ))
            .children(d.detail.clone().map(|detail| {
                crate::elements::clip::clipped(
                    div()
                        .id("omni-detail")
                        .max_w(px(230.))
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .text_xs()
                        .text_color(muted)
                        .when(d.detail_typed, |t| {
                            t.font_family(cx.global::<Theme>().code_font.clone())
                        }),
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
    if let Some(done) = strip.take() {
        list = list.child(done);
    }
    // A folder that takes a moment to read (a disk asleep) says so.
    let reading = omni
        .reading
        .as_ref()
        .filter(|(_, since, _)| since.elapsed() >= READING_SAID)
        .map(|(folder, ..)| format!("Reading {}…", folder.display()));
    // How deep words look, and what they keep there.
    let depth = depth_line(shell, typed, cx);
    // The field's card: its top is the field, which draws itself above it
    // (`Shell::omnibox_card`), the rows under a line.
    let rows = div()
        .p_1()
        .flex()
        .flex_col()
        .border_t_1()
        .border_color(theme.border)
        .children(depth)
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
                .child(footer(shell, typed, omni.scopes)),
        );
    let panel = div()
        .w_full()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .text_color(theme.text)
        .shadow_md()
        .occlude()
        .child(
            div()
                .flex_none()
                .h(px(CARD_PAD + crate::address::FIELD_HEIGHT - 1.)),
        )
        .child(rows);
    Some(anchor.popup_around(CARD_PRIORITY, px(CARD_PAD), px(CARD_LEAST), panel))
}

#[cfg(test)]
mod tests {
    use super::{Answer, Location, Omni, Section, Suggestion, rest_after, scheme_of, scheme_start};

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

    #[test]
    fn file_uris_are_read_as_the_paths_they_name() {
        assert_eq!(super::uri_path("file:///").as_deref(), Some("/"));
    }

    #[test]
    fn a_row_answering_the_text_before_adds_only_what_still_follows_it() {
        // `dunes kind:im` answered with `image`; then `a`, then `x` typed.
        let mut row = Suggestion::new(Section::Complete, "Image");
        row.completion = Some("dunes kind:image".to_owned());
        row.rest = Some("age".to_owned());
        assert_eq!(rest_after(&row, "dunes kind:ima").as_deref(), Some("ge"));
        assert_eq!(rest_after(&row, "dunes kind:imx"), None);
        assert_eq!(rest_after(&row, "dunes kind:image"), None);
        // Typed otherwise than it completes (`IM`): nothing was drawn.
        row.rest = None;
        assert_eq!(rest_after(&row, "dunes kind:ima"), None);
    }

    #[test]
    fn the_rows_are_the_texts_once_every_source_answered_it_in_its_scope() {
        let mut omni = Omni::default();
        omni.answers = vec![(
            "query".to_owned(),
            ("dunes".to_owned(), None),
            Answer::default(),
        )];
        omni.waiting = vec!["jump".to_owned()];
        assert!(!omni.answered("dunes"), "a source still to answer");
        omni.waiting.clear();
        assert!(omni.answered("dunes"));
        assert!(!omni.answered("dunes k"), "an answer to the text before");
        omni.scope = Some(Location::from_path(std::path::Path::new("/srv")));
        assert!(!omni.answered("dunes"), "an answer in another scope");
    }
}
