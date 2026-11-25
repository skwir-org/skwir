//! The shell's own sources of what the address field offers (ADR-098):
//! the window's other tabs, where the pane was, the sidebar's places and
//! the commands. Each answers from memory, on a worker like the plugins'
//! sources, under the shell's owner (`ui`).

use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::command::Invocation;
use skwir_core::location::Location;
use skwir_core::registry::Opens;
use skwir_core::suggest::{Answer, Ask, Section, SuggestSource, Suggester, Suggestion, Typed};
use skwir_core::words;
use skwir_core::words::Words;

/// The most rows a source of places or commands gives for words.
const PLACES: usize = 4;
const ACTIONS: usize = 2;
/// The most steps back offered before anything is typed.
const BACK: usize = 3;

/// Add the shell's sources to the registry.
pub fn register(app: &App) {
    let owner = crate::commands::OWNER;
    for source in [
        Suggester::new(Tabs),
        Suggester::new(History),
        Suggester::new(Places),
        Suggester::new(Commands),
    ] {
        app.registry.suggesters.add(owner, source);
    }
}

/// How well `label` goes with `words`: its start, a word's start, inside
/// it; `None` when it does not hold them. Case is ignored.
pub fn rank_of(label: &str, words: &str) -> Option<i32> {
    let (label, words) = (label.to_lowercase(), words.to_lowercase());
    if label.starts_with(&words) {
        Some(0)
    } else if label
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(&words))
    {
        Some(1)
    } else {
        label.contains(&words).then_some(2)
    }
}

/// A location as a row's detail says it: a folder's path as a person
/// reads it (`os_path::from_home`), drawn alone, its controls shown, never
/// obeyed; else its text, as written.
pub(crate) fn said(location: &Location) -> Words {
    match location.path() {
        Some(path) => skwir_core::os_path::from_home(&path).alone(),
        None => Words::as_written(location.to_string()),
    }
}

/// Where Ctrl+F on a row of `location` searches: itself, where its
/// provider says words can search it (`App::searchable`: a folder, a tag,
/// every tag, Recent); `None` for what words cannot search in (the trash).
pub fn scope_of(app: &App, location: &Location) -> Option<Location> {
    app.searchable(location).then(|| location.clone())
}

fn navigate(location: &Location, ask: &Ask) -> Invocation {
    Invocation::with_args(
        "navigate",
        &serde_json::json!({ "location": location.arg(), "pane": ask.pane }),
    )
}

/// The window's other tabs: before anything is typed without a scope, and
/// those whose title goes with the words.
struct Tabs;

impl SuggestSource for Tabs {
    fn id(&self) -> &'static str {
        "tabs"
    }

    fn answers(&self, ask: &Ask) -> bool {
        match ask.typed {
            Typed::Nothing => ask.scope.is_none(),
            Typed::Words => ask.words().len() >= 2,
            _ => false,
        }
    }

    fn suggest(&self, app: &App, ask: &Ask, _: &CancelToken) -> Answer {
        let words = ask.words();
        let rows = app
            .tab_list()
            .into_iter()
            .filter(|tab| !tab.shown)
            .filter_map(|tab| {
                // What it is called, as its tab says it: what is typed
                // finds it by that.
                let title = tab.title.say();
                let rank = if ask.typed == Typed::Words {
                    rank_of(&title, words)?
                } else {
                    0
                };
                let section = if ask.typed == Typed::Words {
                    Section::Go
                } else {
                    Section::Tabs
                };
                let mut row = Suggestion::new(section, tab.title.clone())
                    .detail(words!(
                        suggestions_switch_tab = "Switch to tab",
                        note = "Under a tab's title in the address field's card: Enter shows that tab."
                    ))
                    .icon("tab-new")
                    .rank(rank)
                    .runs(Invocation::with_args(
                        "tab.select",
                        &serde_json::json!({ "tab": tab.id }),
                    ));
                row.rank += 10;
                Some(row)
            })
            .collect();
        Answer {
            rows,
            spans: Vec::new(),
        }
    }
}

/// Where the pane was, the latest first: before anything is typed without
/// a scope. Enter goes back that many steps.
struct History;

impl SuggestSource for History {
    fn id(&self) -> &'static str {
        "history"
    }

    fn answers(&self, ask: &Ask) -> bool {
        ask.typed == Typed::Nothing && ask.scope.is_none() && ask.pane.is_some()
    }

    fn suggest(&self, app: &App, ask: &Ask, _: &CancelToken) -> Answer {
        let back: Vec<Location> = {
            let ws = app.workspace.read();
            let Some(pane) = ask.pane.and_then(|p| ws.pane(p)) else {
                return Answer::default();
            };
            pane.history[..pane.history_pos.min(pane.history.len())]
                .iter()
                .rev()
                .take(BACK)
                .cloned()
                .collect()
        };
        let rows = back
            .into_iter()
            .enumerate()
            .map(|(i, location)| {
                let mut row = Suggestion::new(Section::History, app.location_title(&location))
                    .icon("back")
                    .rank(i32::try_from(i).unwrap_or(i32::MAX))
                    .runs(Invocation::with_args(
                        "back",
                        &serde_json::json!({ "steps": i + 1, "pane": ask.pane }),
                    ));
                // A folder says where it is; a search's title says it all.
                row.detail = location.path().is_some().then(|| said(&location));
                row.scope = scope_of(app, &location);
                row
            })
            .collect();
        Answer {
            rows,
            spans: Vec::new(),
        }
    }
}

/// The sidebar's places whose names go with the words: folders, bookmarks,
/// saved searches, the trash, tags kept there.
struct Places;

impl SuggestSource for Places {
    fn id(&self) -> &'static str {
        "places"
    }

    fn answers(&self, ask: &Ask) -> bool {
        ask.typed == Typed::Words && ask.words().len() >= 2
    }

    fn suggest(&self, app: &App, ask: &Ask, _: &CancelToken) -> Answer {
        let words = ask.words();
        // Each with where it leads, as data: two places leading to one
        // place, filtered alike, are offered once.
        // Those the window asking offers: a dialog's, none whose files are
        // not to give (the Trash, ADR-144, 3).
        let chooses = ask.pane.is_some_and(|pane| app.chooses_in(pane));
        let mut found: Vec<(i32, (Location, Option<String>), Suggestion)> = app
            .places_offered(chooses)
            .into_iter()
            .filter(|place| !place.hidden)
            .filter_map(|place| {
                // Found by its name as the sidebar says it.
                let title = app
                    .registry
                    .user_title(&place)
                    .map_or_else(|| place.title.clone(), Words::as_written);
                let rank = rank_of(&title.say(), words)?;
                // A saved search or place opens as it was kept, a place
                // saying its filter (ADR-109).
                let leads = (place.location.clone(), place.filter().map(str::to_owned));
                let detail = match place.filter() {
                    Some(filter) => words!(
                        suggestions_place_filtered = "{ $location } · { $filter }",
                        location = said(&place.location),
                        filter = filter,
                        note = "Under a kept place's name in the address field's card; location: where it is (a path); filter: what it is filtered by, as typed."
                    ),
                    None => said(&place.location),
                };
                let runs = match place.opens {
                    Opens::Lensed => navigate(&place.location, ask),
                    Opens::AsKept(_) => place.open().with("pane", ask.pane),
                };
                let mut row = Suggestion::new(Section::Go, title)
                    .detail(detail)
                    .icon(crate::trail::glyph(app, &place.location))
                    .rank(rank)
                    .runs(runs);
                row.scope = scope_of(app, &place.location);
                Some((rank, leads, row))
            })
            .collect();
        found.sort_by_key(|(rank, _, _)| *rank);
        found.dedup_by(|a, b| a.1 == b.1);
        Answer {
            rows: found
                .into_iter()
                .take(PLACES)
                .map(|(_, _, row)| row)
                .collect(),
            spans: Vec::new(),
        }
    }
}

/// Commands whose titles hold the words, a few, so the field teaches what
/// can be done and with which keys (the shell adds them).
struct Commands;

impl SuggestSource for Commands {
    fn id(&self) -> &'static str {
        "commands"
    }

    fn answers(&self, ask: &Ask) -> bool {
        ask.typed == Typed::Words && ask.words().chars().count() >= 3
    }

    fn suggest(&self, app: &App, ask: &Ask, _: &CancelToken) -> Answer {
        let words = ask.words();
        let mut found: Vec<(i32, Suggestion)> = crate::palette::offered(app, &[], &[], &[])
            .into_iter()
            .filter(|offer| offer.required.is_empty())
            .filter_map(|offer| {
                let rank = rank_of(&offer.title, words)?;
                Some((
                    rank,
                    Suggestion::new(Section::Actions, offer.words)
                        .icon("command")
                        .rank(rank)
                        .runs(Invocation {
                            name: offer.name.into(),
                            args: offer.args,
                        }),
                ))
            })
            .collect();
        found.sort_by_key(|(rank, _)| *rank);
        Answer {
            rows: found
                .into_iter()
                .take(ACTIONS)
                .map(|(_, row)| row)
                .collect(),
            spans: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{rank_of, said};
    use skwir_core::location::Location;

    #[test]
    fn labels_rank_by_where_the_words_are() {
        assert_eq!(rank_of("Downloads", "dow"), Some(0));
        assert_eq!(rank_of("Show hidden files", "hid"), Some(1));
        assert_eq!(rank_of("Field Guide", "uid"), Some(2));
        assert_eq!(rank_of("Maps", "x"), None);
    }

    /// A folder below home is said from `~` with the system's separator
    /// between all its names (`~\wip\one` on Windows, never `~/wip\one`),
    /// its controls shown; home itself is `~`; what is no folder, as
    /// written.
    #[test]
    fn a_folder_is_said_from_home_as_the_system_writes_it() {
        let Some(home) = skwir_core::os_path::home() else {
            return;
        };
        let sep = std::path::MAIN_SEPARATOR;
        let nested = home.join("wip").join("one");
        assert_eq!(
            said(&Location::from_path(&nested)).say_plain(),
            format!("~{sep}wip{sep}one")
        );
        let spoof = home.join("a\u{202e}gpj.exe");
        assert_eq!(
            said(&Location::from_path(&spoof)).say_plain(),
            format!("~{sep}a⟨RLO⟩gpj.exe")
        );
        assert_eq!(said(&Location::from_path(&home)).say_plain(), "~");
        let search = Location::new("query", "notes in:/a");
        assert_eq!(said(&search).say_plain(), search.to_string());
    }
}
