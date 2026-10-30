//! What the query plugin offers the address field (ADR-098): the search
//! Enter runs with the words, or why it cannot run yet; what the term being
//! typed goes on with; the words read as terms ("Did you mean"); the same
//! search in other scopes; and before anything is typed, the terms to add,
//! the searches run lately and the saved ones. The text's spans come from
//! here too (`assist::tokens_at`): the field draws them, knowing no syntax.
//!
//! The searches run from the field are remembered (`recent`), unless the
//! user turned that off. What Enter runs is also shown as the words are
//! typed, where that costs little (`live`).

use std::path::{Path, PathBuf};

use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::command::Invocation;
use skwir_core::location::Location;
use skwir_core::os_path::PathArg;
use skwir_core::suggest::{self, Answer, Ask, Section, SuggestSource, Suggestion, Typed};

use crate::assist::{self, SpanKind};
use crate::{ID, lang};

/// Whether the searches run from the address field are remembered.
pub const REMEMBER_KEY: &str = "query.remember_searches";

/// How many searches are remembered.
const RECENT: usize = 20;
/// How many are offered before anything is typed.
const RECENT_SHOWN: usize = 4;

/// The source.
pub struct Field;

impl SuggestSource for Field {
    fn id(&self) -> &'static str {
        ID
    }

    fn answers(&self, ask: &Ask) -> bool {
        match ask.typed {
            Typed::Words | Typed::Scope | Typed::Help => true,
            Typed::Nothing => ask.scope.is_some(),
            Typed::Path | Typed::Place => false,
        }
    }

    /// Words typed in a scope are searched there: what Enter runs.
    fn searches(&self) -> bool {
        true
    }

    fn suggest(&self, app: &App, ask: &Ask, _: &CancelToken) -> Answer {
        match ask.typed {
            Typed::Nothing => before_typing(app, ask),
            Typed::Scope => Answer {
                rows: vec![at_words(app, ask)],
                spans: Vec::new(),
            },
            Typed::Help => terms(),
            _ => typed(app, ask),
        }
    }
}

/// Where words search: the scope, else the folder shown, else home.
fn scope_of(ask: &Ask) -> Option<Location> {
    ask.scope
        .clone()
        .or_else(|| ask.dir.as_ref().map(Location::from_path))
        .or_else(|| skwir_core::os_path::home().map(Location::from_path))
}

/// What a scope is called in a row: a place with depths searched with
/// everything within it says so (`searched_place`).
fn scope_said(app: &App, scope: &Location, ask: &Ask) -> String {
    match scope.path() {
        Some(path) if path.parent().is_none() => "on the whole disk".to_owned(),
        Some(path) if Some(&path) == skwir_core::os_path::home().as_ref() => {
            "everywhere in Home".to_owned()
        }
        Some(_) => format!("in {} and its folders", app.location_title(scope)),
        None if searched_place(app, scope, ask).is_some_and(|(_, within)| within) => {
            format!("among every file within {}", app.location_title(scope))
        }
        None => format!("among the files in {}", app.location_title(scope)),
    }
}

/// The place a place with depths (a tag's, ADR-108) is searched as, and
/// whether that is with everything within it: as the pane shows it, if
/// what it lists there holds files of its own; else (the top of the tags,
/// a tag holding only sub-tags) and from elsewhere, with everything within
/// it. What the pane lists says it, never the place's scheme. `None`: no
/// place with depths.
fn searched_place(app: &App, scope: &Location, ask: &Ask) -> Option<(Location, bool)> {
    let depths = app.depths(scope)?;
    let shown = ask.shown.as_ref() == Some(scope) && lists_files(app, ask.pane);
    let place = if shown {
        scope.clone()
    } else {
        depths.below.clone()
    };
    let within = place == depths.below;
    Some((place, within))
}

/// Whether `pane` lists an entry of its own, not a place within it (a
/// file, a folder tagged): the first one found says it.
fn lists_files(app: &App, pane: Option<skwir_core::workspace::PaneId>) -> bool {
    pane.is_some_and(|pane| {
        app.workspace
            .read()
            .pane(pane)
            .is_some_and(|p| p.listing.iter().any(|e| !e.is_place()))
    })
}

/// The search of `expr` in `scope`, as the field runs it: a folder is its
/// root; a place is searched as what it lists (`in:recent:`), one with
/// depths (a tag's, ADR-108) as the pane shows it or with everything
/// within it (`searched_place`).
///
/// It says its lens whole (`as_is`, ADR-107): the words typed, after the
/// pane's lens kept when the field began without it (`Ask::lens`).
pub fn search(app: &App, expr: &str, scope: &Location, ask: &Ask) -> Invocation {
    let kept;
    let expr = match ask.lens.as_deref().map(str::trim) {
        Some(lens) if !lens.is_empty() => {
            kept = format!("{lens} {expr}");
            kept.trim()
        }
        _ => expr,
    };
    if let Some(path) = scope.path() {
        return Invocation::with_args(
            "query.search",
            &serde_json::json!({
                "expr": expr,
                "root": PathArg::from(path),
                "pane": ask.pane,
                "remember": true,
                "as_is": true,
            }),
        );
    }
    // A place keeps its trail, searched as what it lists.
    let place = searched_place(app, scope, ask).map_or_else(|| scope.clone(), |(place, _)| place);
    let expr = format!("{expr} in:{}", lang::quote(&place.to_string()));
    Invocation::with_args(
        "query.search",
        &serde_json::json!({
            "expr": expr.trim(),
            "pane": ask.pane,
            "remember": true,
            "as_is": true,
        }),
    )
}

/// The search of `expr` in `scope` shown as its words are typed (ADR-098):
/// in place of the step the pane shows, not remembered. None where each
/// search costs: one that reads the files, the whole disk, a folder on a
/// server (each walk round trips); those wait for Enter.
fn live(app: &App, expr: &str, scope: &Location, ask: &Ask) -> Option<Invocation> {
    if lang::parse(expr).ok()?.reads_files() {
        return None;
    }
    if let Some(path) = scope.path()
        && (path.parent().is_none() || skwir_core::fs::reach::of(&path).is_some())
    {
        return None;
    }
    let mut run = search(app, expr, scope, ask);
    run.args["remember"] = false.into();
    run.args["in_place"] = true.into();
    Some(run)
}

/// The rows for words typed.
fn typed(app: &App, ask: &Ask) -> Answer {
    let text = ask.text.as_str();
    let words = ask.words();
    let caret = ask.caret.min(text.len());
    let spans = assist::tokens_at(text, caret)
        .into_iter()
        .map(|s| suggest::Span {
            range: s.range,
            kind: match s.kind {
                SpanKind::Word => suggest::SpanKind::Word,
                SpanKind::Key | SpanKind::Option => suggest::SpanKind::Key,
                SpanKind::Value => suggest::SpanKind::Value,
                SpanKind::Operator => suggest::SpanKind::Operator,
                SpanKind::Error => suggest::SpanKind::Error,
            },
            message: s.message,
        })
        .collect();
    let Some(scope) = scope_of(ask) else {
        return Answer::default();
    };
    let tags = assist::pane_tags(app, ask.pane);
    let completed = if ask.typing_at_end() {
        assist::complete(text, caret, &tags)
    } else {
        assist::Completed::default()
    };
    let with = |item: &assist::Item| {
        format!(
            "{}{}{}",
            &text[..completed.range.start],
            item.text,
            &text[completed.range.end..]
        )
    };
    let rest_of = |item: &assist::Item| {
        let typed = &text[completed.range.clone()];
        item.text
            .strip_prefix(typed)
            .filter(|rest| !rest.is_empty())
            .map(str::to_owned)
    };
    let mut rows = Vec::new();
    // What Enter runs: the search, the value being typed taken; or why the
    // text cannot be searched yet.
    let first = completed.items.first().filter(|_| completed.typing);
    let problem = assist::problem(text, caret);
    // What is searched: the words typed, the value being typed taken; in
    // the scope, and elsewhere below.
    let expr = first.map_or_else(|| words.to_owned(), |item| with(item).trim().to_owned());
    if let Some(problem) = problem.filter(|_| first.is_none()) {
        let mut row = Suggestion::new(Section::Run, problem).icon("warning");
        row.error = true;
        rows.push(row);
    } else if completed.typing && first.is_none() {
        let said = if assist::quote_open(text) {
            "Close the quote to search"
        } else {
            "Type a value to search"
        };
        let mut row = Suggestion::new(Section::Run, said).icon("warning");
        row.error = true;
        rows.push(row);
    } else {
        let mut row = Suggestion::new(Section::Run, format!("Search “{expr}”"))
            .detail(scope_said(app, &scope, ask))
            .icon("search")
            .runs(search(app, &expr, &scope, ask));
        if let Some(home) = skwir_core::os_path::home().map(Location::from_path)
            && home != scope
        {
            row.alt = Some(search(app, &expr, &home, ask));
        }
        if let Some(item) = first {
            row.completion = Some(with(item));
            row.rest = rest_of(item);
        } else {
            row.live = live(app, &expr, &scope, ask);
        }
        rows.push(row);
    }
    // What the term being typed goes on with: Enter searches with it, Tab
    // writes it.
    for (i, item) in completed.items.iter().enumerate() {
        let is_key = item.text.ends_with(':') && !item.text.contains("::");
        let mut row = Suggestion::new(Section::Complete, item.label.clone())
            .detail(item.detail.clone().unwrap_or_else(|| item.text.clone()))
            .icon(&item.icon)
            .rank(i32::try_from(i).unwrap_or(i32::MAX));
        row.detail_typed = true;
        row.completion = Some(with(item));
        row.rest = rest_of(item);
        row.tag = item.tag.as_deref().map(skwir_core::metadata::TagRef::of);
        if !is_key {
            row.run = Some(search(app, with(item).trim(), &scope, ask));
        }
        rows.push(row);
    }
    // The words read as terms, offered below the search of the words; Tab
    // writes them (and Enter, over a folder filtered alone).
    if let Some(reading) = assist::interpret(words, &tags) {
        let mut row = Suggestion::new(Section::Reading, reading.label)
            .detail(reading.expr.clone())
            .icon("sliders")
            .runs(search(app, &reading.expr, &scope, ask));
        row.detail_typed = true;
        row.completion = Some(reading.expr.clone());
        rows.push(row);
    }
    // The same search elsewhere: the folder shown, home, the whole disk,
    // and the files that contain the words. Made of the words typed, never
    // of the scope's search (a place's `in:`, a tag's term).
    if !rows.first().is_some_and(|r| r.error) {
        let shown = ask.dir.as_ref().map(Location::from_path);
        let home = skwir_core::os_path::home().map(Location::from_path);
        let disk = Location::from_path(PathBuf::from(std::path::MAIN_SEPARATOR_STR));
        let mut seen = vec![scope.clone()];
        for (rank, other, icon) in [
            (0, shown, "folder"),
            (1, home, "home"),
            (2, Some(disk), "layers"),
        ] {
            let Some(other) = other.filter(|o| !seen.contains(o)) else {
                continue;
            };
            seen.push(other.clone());
            let title = match other.path() {
                Some(path) if path.parent().is_none() => "Whole disk".to_owned(),
                _ => app.location_title(&other),
            };
            rows.push(
                Suggestion::new(Section::SearchIn, title)
                    .icon(icon)
                    .rank(rank)
                    .runs(search(app, &expr, &other, ask)),
            );
        }
        if lang::parse(words).is_ok_and(|q| !q.reads_files()) && assist::only_words(words) {
            rows.push(
                Suggestion::new(Section::SearchIn, "Contents")
                    .icon("contents")
                    .rank(3)
                    .runs(search(
                        app,
                        &format!("content:{}", lang::quote(words)),
                        &scope,
                        ask,
                    )),
            );
        }
        // A word written `#tag` is a tag; the same words in names, for a
        // name that has one (`#3`, an editor's `#notes#`).
        if let Some(names) = assist::hashes_as_names(words) {
            rows.push(
                Suggestion::new(Section::SearchIn, "Names")
                    .detail(names.clone())
                    .icon("search")
                    .rank(4)
                    .runs(search(app, &names, &scope, ask)),
            );
        }
    }
    Answer { rows, spans }
}

/// How many terms `?` lists.
const TERMS_SHOWN: usize = 10;

/// `?` alone: the terms of a search, each with examples, the most asked
/// first; Enter or Tab writes the key.
fn terms() -> Answer {
    let rows = assist::terms()
        .into_iter()
        .take(TERMS_SHOWN)
        .enumerate()
        .map(|(rank, (written, icon, example))| {
            let mut row = Suggestion::new(Section::Syntax, written.clone())
                .detail(example)
                .icon(icon)
                .rank(i32::try_from(rank).unwrap_or(0));
            row.typed = true;
            row.completion = Some(written);
            row
        })
        .collect();
    Answer {
        rows,
        spans: Vec::new(),
    }
}

/// `@…` typed: below the scopes the shell lists, the words searched as
/// they are, for a name that starts with `@`.
fn at_words(app: &App, ask: &Ask) -> Suggestion {
    let words = ask.words();
    let expr = lang::quote(words);
    let scope = scope_of(ask).unwrap_or_else(|| Location::from_path(PathBuf::from("/")));
    Suggestion::new(Section::Run, format!("Search for “{words}”"))
        .detail(scope_said(app, &scope, ask))
        .icon("search")
        .rank(1)
        .runs(search(app, &expr, &scope, ask))
}

/// Before anything is typed, with a scope: the terms to add, the searches
/// run lately and the saved ones.
fn before_typing(app: &App, ask: &Ask) -> Answer {
    let mut rows = Vec::new();
    let text = ask.text.trim_end();
    let before = if text.is_empty() {
        String::new()
    } else {
        format!("{text} ")
    };
    for (rank, (key, title, icon)) in [
        ("kind:", "Kind", "grid"),
        ("modified:", "Modified", "calendar"),
        ("size:", "Size", "sliders"),
        ("tag:", "Tag", "tag"),
        ("content:", "Contains", "contents"),
        ("taken:", "Taken", "calendar"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut row = Suggestion::new(Section::Terms, title)
            .icon(icon)
            .rank(i32::try_from(rank).unwrap_or(0));
        row.completion = Some(format!("{before}{key}"));
        rows.push(row);
    }
    for (rank, recent) in recent(app).into_iter().take(RECENT_SHOWN).enumerate() {
        // Where it looked, as its own text says it (`in:recent:`, a folder
        // typed) or as it was run; its words without it.
        let said = lang::parse(&recent.expr).ok().and_then(|q| {
            q.within
                .clone()
                .or_else(|| q.root_given.then(|| Location::from_path(&q.root)))
        });
        let scope = said
            .or_else(|| recent.root.as_ref().map(Location::from_path))
            .or_else(|| scope_of(ask));
        let Some(scope) = scope else { continue };
        let words = crate::facets::text_of(&recent.expr)
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| recent.expr.clone());
        let mut row = Suggestion::new(Section::Recent, words.clone())
            .detail(format!("in {}", app.location_title(&scope)))
            .icon("recent")
            .rank(i32::try_from(rank).unwrap_or(0))
            .runs(search(app, &words, &scope, ask));
        row.typed = true;
        row.forget = Some(Invocation::with_args(
            "query.forget_recent",
            &serde_json::json!({
                "expr": recent.expr,
                "root": recent.root.as_deref().map(PathArg::from),
            }),
        ));
        rows.push(row);
    }
    for (rank, saved) in crate::saved(app).into_iter().take(RECENT_SHOWN).enumerate() {
        let location = Location::new(crate::SCHEME, saved.expr.clone());
        let mut row = Suggestion::new(Section::Saved, saved.name)
            .detail(crate::facets::text_of(&saved.expr).unwrap_or(saved.expr))
            .icon("star")
            .rank(i32::try_from(rank).unwrap_or(0))
            .runs(Invocation::with_args(
                "navigate",
                &serde_json::json!({ "location": location.arg(), "pane": ask.pane }),
            ));
        row.detail_typed = true;
        rows.push(row);
    }
    Answer {
        rows,
        spans: Vec::new(),
    }
}

/// A search run from the field: its words and where it looked.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Recent {
    pub expr: String,
    #[serde(default, with = "skwir_core::os_path::opt")]
    pub root: Option<PathBuf>,
}

const RECENT_KEY: &[u8] = b"recent-searches";

/// The searches run from the field, the latest first; none while the user
/// keeps them from being remembered.
pub fn recent(app: &App) -> Vec<Recent> {
    if !remembers(app) {
        return Vec::new();
    }
    app.cache
        .namespace(ID)
        .get(RECENT_KEY)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn remembers(app: &App) -> bool {
    app.settings.get::<bool>(REMEMBER_KEY).unwrap_or(true)
}

fn keep(app: &App, list: &[Recent]) {
    let slot = app.cache.namespace(ID);
    let written = if list.is_empty() {
        slot.remove(RECENT_KEY)
    } else {
        serde_json::to_vec(list)
            .map_err(std::io::Error::other)
            .and_then(|bytes| slot.put(RECENT_KEY, &bytes))
    };
    if let Err(error) = written {
        tracing::warn!("recent searches: {error}");
    }
}

/// Change the searches remembered as `edit` says, one change at a time
/// (each reads and writes the cache's file): on a worker when asked on the
/// window's thread, which a cache on a sleeping disk must not hold; where
/// it is asked anywhere else (a script's call, a test).
fn change(app: &App, edit: impl FnOnce(&mut Vec<Recent>) + Send + 'static) {
    static CHANGING: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    let changed = |app: &App| {
        let _one = CHANGING.lock();
        let mut list = recent(app);
        edit(&mut list);
        keep(app, &list);
    };
    if !skwir_core::scheduler::on_window_thread() {
        return changed(app);
    }
    let weak = app.weak();
    app.scheduler.spawn(
        skwir_core::scheduler::Priority::Background,
        "query.recent",
        app.lifetime().child(),
        move || {
            if let Some(app) = weak.upgrade() {
                changed(&app);
            }
        },
    );
}

/// Remember a search run from the field, first; an older run of it goes.
/// Nothing is said of it in the logs: what is looked for is the user's.
pub fn remember(app: &App, expr: &str, root: Option<&Path>) {
    if !remembers(app) || expr.trim().is_empty() {
        return;
    }
    let run = Recent {
        expr: expr.trim().to_owned(),
        root: root.map(Path::to_path_buf),
    };
    change(app, move |list| {
        list.retain(|r| *r != run);
        list.insert(0, run);
        list.truncate(RECENT);
    });
}

/// Forget a search remembered.
pub fn forget(app: &App, expr: &str, root: Option<&Path>) {
    let (expr, root) = (expr.to_owned(), root.map(Path::to_path_buf));
    change(app, move |list| {
        list.retain(|r| !(r.expr == expr && r.root == root));
    });
}

/// The searches remembered are let go of: the user turned remembering off.
pub fn forget_all(app: &App) {
    change(app, Vec::clear);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(text: &str) -> Ask {
        Ask {
            text: text.to_owned(),
            caret: text.len(),
            typed: Typed::Words,
            scope: Some(Location::from_path("/data/guide")),
            shown: None,
            dir: Some(PathBuf::from("/data/guide")),
            pane: None,
            lens: None,
        }
    }

    fn rows(text: &str) -> Vec<Suggestion> {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        Field.suggest(&app, &ask(text), &CancelToken::new()).rows
    }

    #[test]
    fn enter_runs_the_search_or_says_why_it_cannot() {
        let r = rows("dunes jpg");
        assert_eq!(r[0].section, Section::Run);
        assert_eq!(r[0].title, "Search “dunes jpg”");
        let run = r[0].run.as_ref().expect("runs");
        assert_eq!(run.args["expr"], "dunes jpg");
        assert_eq!(run.args["root"], "/data/guide");
        assert!(r[0].alt.is_some(), "Alt+Enter searches home");
        // Shown as the words are typed, in place and not remembered.
        let live = r[0].live.as_ref().expect("shown as typed");
        assert_eq!(live.args["expr"], "dunes jpg");
        assert_eq!(live.args["in_place"], true);
        assert_eq!(live.args["remember"], false);
        // Its words say the whole lens (ADR-107); begun without the pane's
        // (Ctrl+L, Ctrl+J), they add to it.
        assert_eq!(run.args["as_is"], true);
        assert_eq!(live.args["as_is"], true);
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let mut kept = ask("dunes");
        kept.lens = Some("kind:image".into());
        let run = search(&app, "dunes", &Location::from_path("/data/guide"), &kept);
        assert_eq!(run.args["expr"], "kind:image dunes");
        let r = rows("kind:pix");
        assert!(r[0].error && r[0].run.is_none());
        assert!(r[0].title.contains("pix"), "{}", r[0].title);
    }

    #[test]
    fn what_reads_the_files_or_walks_the_disk_waits_for_enter() {
        let r = rows("content:filter");
        assert!(r[0].run.is_some() && r[0].live.is_none());
        let r = rows("dunes kind:im");
        assert!(r[0].live.is_none(), "a value still typed");
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let mut disk = ask("dunes");
        disk.scope = Some(Location::from_path(std::path::MAIN_SEPARATOR_STR));
        let r = Field.suggest(&app, &disk, &CancelToken::new()).rows;
        assert!(r[0].run.is_some() && r[0].live.is_none());
    }

    #[test]
    fn a_value_being_typed_is_what_enter_takes_and_tab_writes() {
        let r = rows("dunes kind:im");
        assert_eq!(r[0].title, "Search “dunes kind:image”");
        assert_eq!(r[0].completion.as_deref(), Some("dunes kind:image"));
        assert_eq!(r[0].rest.as_deref(), Some("age"));
        assert!(r.iter().any(|s| s.section == Section::Complete));
    }

    /// `#print` searches the tag; the same word in names is a row below,
    /// and no contents search is offered for a term.
    #[test]
    fn a_hash_searches_a_tag_and_names_are_a_row_below() {
        let r = rows("#print dunes");
        assert_eq!(r[0].title, "Search “#print dunes”");
        let names = r.iter().find(|s| s.title == "Names").expect("names");
        assert_eq!(
            names.run.as_ref().unwrap().args["expr"],
            r##""#print" dunes"##
        );
        assert!(r.iter().all(|s| s.title != "Contents"));
        let r = rows("dunes");
        assert!(r.iter().any(|s| s.title == "Contents"));
        assert!(r.iter().all(|s| s.title != "Names"));
    }

    #[test]
    fn words_read_as_terms_are_offered_below_the_words() {
        let r = rows("pdf last week");
        assert_eq!(r[0].title, "Search “pdf last week”");
        let reading = r
            .iter()
            .find(|s| s.section == Section::Reading)
            .expect("a reading");
        assert_eq!(reading.title, "PDFs modified last week");
        assert_eq!(
            reading.run.as_ref().unwrap().args["expr"],
            "ext:pdf modified:last-week"
        );
        assert!(r.iter().any(|s| s.section == Section::SearchIn));
    }

    /// A place is searched as what it lists, whatever tags it says of
    /// everything it lists; a folder is the search's root.
    #[test]
    fn a_place_is_searched_as_what_it_lists_and_a_folder_from_its_root() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let a = ask("dunes");
        let place = search(&app, "dunes", &Location::new("tagged", "/"), &a);
        assert_eq!(place.args["expr"], "dunes in:tagged:/");
        assert!(place.args.get("root").is_none());
        let folder = search(&app, "dunes", &Location::from_path("/data/it's photos"), &a);
        assert_eq!(folder.args["expr"], "dunes");
        assert_eq!(folder.args["root"], "/data/it's photos");
        assert_eq!(folder.args["remember"], true);
    }

    /// The same search elsewhere is the words typed, never the scope's own
    /// search (a place's `in:`).
    #[test]
    fn the_same_search_elsewhere_is_of_the_words_typed() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let mut recent = ask("dunes");
        recent.scope = Some(Location::new("recent", ""));
        let r = Field.suggest(&app, &recent, &CancelToken::new()).rows;
        assert_eq!(r[0].run.as_ref().unwrap().args["expr"], "dunes in:recent:");
        let elsewhere: Vec<&Suggestion> = r
            .iter()
            .filter(|s| s.section == Section::SearchIn)
            .collect();
        let folder = elsewhere
            .iter()
            .find(|s| s.icon.as_deref() == Some("folder"))
            .expect("the folder");
        assert_eq!(folder.run.as_ref().unwrap().args["expr"], "dunes");
        assert_eq!(folder.run.as_ref().unwrap().args["root"], "/data/guide");
    }

    #[test]
    fn a_quote_not_closed_says_so() {
        let r = rows(r#""my file"#);
        assert!(
            r[0].error && r[0].title == "Close the quote to search",
            "{}",
            r[0].title
        );
    }
}
