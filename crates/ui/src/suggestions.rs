//! The shell's own sources of what the address field offers (ADR-098):
//! the window's other tabs, where the pane was, the sidebar's places and
//! the commands. Each answers from memory, on a worker like the plugins'
//! sources, under the shell's owner (`ui`).

use skwir_core::app::App;
use skwir_core::cancel::CancelToken;
use skwir_core::command::Invocation;
use skwir_core::location::{Location, PATH_SCHEME};
use skwir_core::suggest::{Answer, Ask, Section, SuggestSource, Suggester, Suggestion, Typed};

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

/// A location as a row's detail says it: a path from `~`, else its text.
pub fn said(location: &Location) -> String {
    match location.path() {
        Some(path) => match skwir_core::os_path::home().and_then(|home| {
            path.strip_prefix(&home)
                .ok()
                .map(std::path::Path::to_path_buf)
        }) {
            Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Some(rest) => format!("~/{}", rest.display()),
            None => path.display().to_string(),
        },
        None => location.to_string(),
    }
}

/// Where Ctrl+F on a row of `location` searches: a folder, a tag, Recent;
/// `None` for what words cannot search in.
pub fn scope_of(location: &Location) -> Option<Location> {
    (&*location.scheme == PATH_SCHEME || matches!(&*location.scheme, "tags" | "recent"))
        .then(|| location.clone())
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
                let rank = if ask.typed == Typed::Words {
                    rank_of(&tab.title, words)?
                } else {
                    0
                };
                let section = if ask.typed == Typed::Words {
                    Section::Go
                } else {
                    Section::Tabs
                };
                let mut row = Suggestion::new(section, tab.title.clone())
                    .detail("Switch to tab")
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
                row.scope = scope_of(&location);
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
        let mut found: Vec<(i32, Suggestion)> = app
            .registry
            .places
            .all()
            .into_iter()
            .filter(|place| !place.hidden)
            .filter_map(|place| {
                let title = app
                    .registry
                    .user_title(&place)
                    .unwrap_or(place.title.clone());
                let rank = rank_of(&title, words)?;
                let mut row = Suggestion::new(Section::Go, title)
                    .detail(said(&place.location))
                    .icon(crate::trail::glyph(app, &place.location))
                    .rank(rank)
                    .runs(navigate(&place.location, ask));
                row.scope = scope_of(&place.location);
                Some((rank, row))
            })
            .collect();
        found.sort_by_key(|(rank, _)| *rank);
        found.dedup_by(|a, b| a.1.detail == b.1.detail);
        Answer {
            rows: found.into_iter().take(PLACES).map(|(_, row)| row).collect(),
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
        let mut found: Vec<(i32, Suggestion)> = crate::palette::offered(app, &[], &[])
            .into_iter()
            .filter(|offer| offer.required.is_empty())
            .filter_map(|offer| {
                let rank = rank_of(&offer.title, words)?;
                Some((
                    rank,
                    Suggestion::new(Section::Actions, offer.title)
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
    use super::rank_of;

    #[test]
    fn labels_rank_by_where_the_words_are() {
        assert_eq!(rank_of("Downloads", "dow"), Some(0));
        assert_eq!(rank_of("Show hidden files", "hid"), Some(1));
        assert_eq!(rank_of("Field Guide", "uid"), Some(2));
        assert_eq!(rank_of("Maps", "x"), None);
    }
}
