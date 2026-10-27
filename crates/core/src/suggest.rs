//! What the address field offers while it is typed in (ADR-098): rows from
//! sources, each a plugin's or the shell's, asked again at each change of
//! the text, each in a job of its own on the scheduler.
//!
//! A source answers an [`Ask`] (the text, where it is typed, the scope
//! words search in, what the pane shows) with an [`Answer`]: rows, each in
//! a [`Section`] and carrying the invocations its keys run, and, from the
//! source that reads the text, the spans the field draws it with. Sources
//! never name each other: the query plugin answers the search rows, Jump
//! the places one goes to, the tags plugin the tags, the shell the tabs,
//! the history, the sidebar's places and the commands. The field merges
//! the answers by section; `ui.suggest` gathers them for an agent or a
//! script ([`gather`]).
//!
//! A source answers from what it keeps or reads cheaply; what may wait (a
//! folder on a remote) runs in that remote's lane ([`SuggestSource::lane`]),
//! and every job stops when the text moves on (its [`CancelToken`]).

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::cancel::CancelToken;
use crate::command::Invocation;
use crate::location::Location;
use crate::metadata::TagRef;
use crate::scheduler::Lane;
use crate::workspace::PaneId;

/// What the text typed is, as the field reads it: a source answers the
/// kinds it knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Typed {
    /// Nothing typed yet, or the text as it was given (the field just took
    /// the keyboard): what is offered before anything is typed.
    Nothing,
    /// Words: a search, a name to go to, a command's name.
    Words,
    /// A path (`/`, `~`, `./`).
    Path,
    /// A place's address (`tags:/Places`, `trash:`).
    Place,
    /// `@` and what follows: where to search.
    Scope,
    /// `?` alone: what the field takes.
    Help,
}

/// What a source is asked.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Ask {
    /// The text as typed, whole.
    pub text: String,
    /// Where the caret is in `text` (a byte offset).
    pub caret: usize,
    pub typed: Typed,
    /// Where words search: the scope token's location, `None` without one.
    #[serde(default)]
    pub scope: Option<Location>,
    /// What the pane shows.
    #[serde(default)]
    pub shown: Option<Location>,
    /// The folder the pane shows, when it shows one.
    #[serde(default, with = "crate::os_path::opt")]
    #[schemars(with = "Option<crate::os_path::PathArg>")]
    pub dir: Option<PathBuf>,
    #[serde(default)]
    pub pane: Option<PaneId>,
}

impl Ask {
    /// The text without the spaces around it.
    pub fn words(&self) -> &str {
        self.text.trim()
    }

    /// Whether the caret is at the end of the text, after a character
    /// that is not a space: a word is being typed there.
    pub fn typing_at_end(&self) -> bool {
        self.caret == self.text.len() && !self.text.ends_with(char::is_whitespace)
    }
}

/// Where a row goes, in the order sections are shown.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    /// What Enter does: first, never titled.
    Run,
    /// What the field takes, each with an example (`?`).
    Help,
    /// The terms of a search, each with examples of its values (`?`).
    Syntax,
    /// Scopes to search in (`@`, the token's list).
    Scopes,
    /// What the text being typed goes on with: a term, a value.
    Complete,
    /// The words read as terms ("Did you mean").
    Reading,
    /// Terms to add, a row of buttons (before anything is typed).
    Terms,
    /// Searches run lately.
    Recent,
    /// Searches the user saved.
    Saved,
    /// The window's other tabs.
    Tabs,
    /// Where the pane was (Back).
    History,
    /// Places to go to: folders, tags, sidebar places.
    Go,
    /// Commands whose names hold the words.
    Actions,
    /// Other scopes to run the same search in, a row of buttons.
    SearchIn,
}

impl Section {
    /// Its title over its rows; `None` for the first row.
    pub fn title(self) -> Option<&'static str> {
        Some(match self {
            Section::Run => return None,
            Section::Help => "What the field takes",
            Section::Syntax => "Terms of a search",
            Section::Scopes | Section::SearchIn => "Search in",
            Section::Complete => "Complete",
            Section::Reading => "Did you mean",
            Section::Terms => "Add a term",
            Section::Recent => "Recent searches",
            Section::Saved => "Saved searches",
            Section::Tabs => "Open tabs",
            Section::History => "Back",
            Section::Go => "Go to",
            Section::Actions => "Actions",
        })
    }

    /// Whether its rows are drawn as a row of buttons.
    pub fn is_strip(self) -> bool {
        matches!(self, Section::Terms | Section::SearchIn)
    }
}

/// A row of the field's dropdown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Suggestion {
    pub section: Section,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// A bundled symbolic icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// A tag it stands for: drawn as the tag's dot, in its color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<TagRef>,
    /// Its title is text as it is typed (a query): drawn in the code font.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub typed: bool,
    /// Its detail is text as it is typed (the terms a row runs).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub detail_typed: bool,
    /// What Enter runs; `None`: Enter waits (a search that cannot parse,
    /// a value still to type), and the row says why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<Invocation>,
    /// What Alt+Enter runs instead (the same search in the home folder).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<Invocation>,
    /// What Shift+Enter runs instead (open a file rather than show it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<Invocation>,
    /// The whole text Tab writes in the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<String>,
    /// What `completion` adds after the text typed, drawn muted there while
    /// the row is highlighted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest: Option<String>,
    /// Where Ctrl+F on the row searches: it becomes the scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Location>,
    /// What Shift+Delete runs: the row is forgotten (a recent search).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forget: Option<Invocation>,
    /// The keys that run the same (a command's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<String>,
    /// The row says the text is wrong (`run` is then `None`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub error: bool,
    /// Its place within its section: lower first.
    #[serde(default)]
    pub rank: i32,
    /// Who answered it (the source's id).
    #[serde(default)]
    pub source: String,
}

impl Suggestion {
    pub fn new(section: Section, title: impl Into<String>) -> Self {
        Suggestion {
            section,
            title: title.into(),
            detail: None,
            icon: None,
            tag: None,
            typed: false,
            detail_typed: false,
            run: None,
            alt: None,
            open: None,
            completion: None,
            rest: None,
            scope: None,
            forget: None,
            keys: None,
            error: false,
            rank: 0,
            source: String::new(),
        }
    }

    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    #[must_use]
    pub fn icon(mut self, icon: &str) -> Self {
        self.icon = Some(icon.to_owned());
        self
    }

    #[must_use]
    pub fn runs(mut self, run: Invocation) -> Self {
        self.run = Some(run);
        self
    }

    #[must_use]
    pub fn rank(mut self, rank: i32) -> Self {
        self.rank = rank;
        self
    }
}

/// How a span of the text is drawn while it is typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// A word to find in names: drawn as typed.
    Word,
    /// A term's key and its operator (`kind:`, `size>`).
    Key,
    /// A term's value.
    Value,
    /// `OR`, `NOT`, `-`, parentheses.
    Operator,
    /// What cannot be read: underlined, with why.
    Error,
}

/// A part of the text and how it is drawn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Span {
    /// Byte offsets into the text.
    pub range: Range<usize>,
    pub kind: SpanKind,
    /// Why it cannot be read (an error's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A source's answer.
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Answer {
    pub rows: Vec<Suggestion>,
    /// How the text is drawn, from the source that reads it; empty from
    /// the others.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<Span>,
}

/// Something that offers rows as the address field is typed in.
pub trait SuggestSource: Send + Sync {
    /// Its name: what a row's `source` says, and what the field keeps its
    /// answer under.
    fn id(&self) -> &'static str;

    /// Whether it has anything to say of `ask`, asked on the window thread
    /// before a job is queued for it: from what `ask` says, never by
    /// reading anything.
    fn answers(&self, ask: &Ask) -> bool;

    /// The lane its job runs in: a remote's when it reads a path there.
    fn lane(&self, _ask: &Ask) -> Lane {
        Lane::SHARED
    }

    /// Its rows, on a worker. It stops early when `cancel` is cancelled:
    /// the text moved on and the answer will not be read.
    fn suggest(&self, app: &App, ask: &Ask, cancel: &CancelToken) -> Answer;
}

/// A source as the registry keeps it.
#[derive(Clone)]
pub struct Suggester(pub Arc<dyn SuggestSource>);

impl Suggester {
    pub fn new(source: impl SuggestSource + 'static) -> Self {
        Suggester(Arc::new(source))
    }
}

impl std::fmt::Debug for Suggester {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Suggester").field(&self.0.id()).finish()
    }
}

/// Answers put in the order the field shows them: by section, then rank,
/// then as they came; each row says its source. The spans are the first
/// source's that gave any.
pub fn merge(answers: impl IntoIterator<Item = (String, Answer)>) -> Answer {
    let mut rows = Vec::new();
    let mut spans = Vec::new();
    for (source, answer) in answers {
        if spans.is_empty() {
            spans = answer.spans;
        }
        rows.extend(answer.rows.into_iter().map(|mut row| {
            if row.source.is_empty() {
                row.source.clone_from(&source);
            }
            row
        }));
    }
    rows.sort_by_key(|row| (row.section, row.rank));
    Answer { rows, spans }
}

/// Every source's answer to `ask`, asked one after another on the caller's
/// thread and merged: what `ui.suggest` answers an agent or a script. A
/// source that panics is left out.
pub fn gather(app: &App, ask: &Ask, cancel: &CancelToken) -> Answer {
    let sources = app.registry.suggesters.all();
    let answers = sources
        .into_iter()
        .filter(|s| s.0.answers(ask))
        .filter_map(|s| {
            let id = s.0.id().to_owned();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                s.0.suggest(app, ask, cancel)
            }))
            .ok()
            .map(|answer| (id, answer))
        });
    merge(answers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(section: Section, title: &str, rank: i32) -> Suggestion {
        Suggestion::new(section, title).rank(rank)
    }

    #[test]
    fn answers_are_merged_by_section_then_rank_and_say_their_source() {
        let a = Answer {
            rows: vec![row(Section::Go, "Maps", 1), row(Section::Run, "Search", 0)],
            spans: vec![Span {
                range: 0..4,
                kind: SpanKind::Word,
                message: None,
            }],
        };
        let b = Answer {
            rows: vec![
                row(Section::Go, "Music", 0),
                row(Section::Complete, "kind:", 0),
            ],
            spans: vec![],
        };
        let merged = merge([("query".to_owned(), a), ("jump".to_owned(), b)]);
        let titles: Vec<&str> = merged.rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Search", "kind:", "Music", "Maps"]);
        assert_eq!(merged.rows[0].source, "query");
        assert_eq!(merged.rows[2].source, "jump");
        assert_eq!(merged.spans.len(), 1);
    }

    #[test]
    fn a_row_reads_back_as_written() {
        let mut r = row(Section::Recent, "kind:image", 2)
            .icon("clock")
            .runs(Invocation::new("query.search"));
        r.typed = true;
        r.forget = Some(Invocation::new("query.forget_recent"));
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["section"], "recent");
        let back: Suggestion = serde_json::from_value(json).unwrap();
        assert_eq!(back, r);
    }
}
