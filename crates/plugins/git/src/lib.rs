//! A folder's and a file's history of commits, and each commit's patch
//! (ADR-133): a way of looking (`PreviewProvider::way`) the preview offers
//! wherever the entry is in a repository, read by `skwir-git-worker`. And
//! what the files of a folder shown are to their repository (ADR-134,
//! `state`), which the views mark.
use std::path::Path;
use std::sync::{Arc, Mutex};

use skwir_core::{
    App, CancelToken,
    inspection::{
        Block, InspectRequest, Inspection, InspectionKind, REFS_JOIN, RefKind, Row, commit_name,
        commit_of, commit_target,
    },
    plugin::Plugin,
    preview::{Member, PreviewProvider, PreviewRequest, Way},
};
use skwir_platform::git::{self, Repo};

pub mod lanes;
pub mod state;

/// The provider's id: what the preview names when it asks for a history.
pub const HISTORY: &str = "git.history";

#[derive(Default)]
pub struct GitPlugin {
    /// The files' state, while registered: told when a window comes to the
    /// front and when its setting changes.
    states: Mutex<Option<Arc<state::States>>>,
}

impl Plugin for GitPlugin {
    fn id(&self) -> &'static str {
        "git"
    }
    fn title(&self) -> &'static str {
        "Git"
    }
    fn description(&self) -> &'static str {
        "A folder's and a file's history of commits in a Git repository, the graph of its merges and what each commit changed, and the files' state marked in the views"
    }
    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Previews
    }
    fn requirements(&self) -> Vec<skwir_core::plugin::Requirement> {
        vec![git::WORKER.requirement("Histories of commits")]
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Read, Capability::Programs])
    }
    fn register(&self, app: &App) {
        app.settings.declare(
            skwir_core::settings::SettingSpec::toggle(
                state::SHOWN,
                "Show the files' Git state",
                "Mark files changed, staged, new or in conflict, and the folders that hold them, in folders kept in a Git repository. The list's Git column says the same.",
                true,
            )
            .page(skwir_core::settings::page::VIEWS)
            .section("Git"),
        );
        app.previews.register(self.id(), Arc::new(Provider));
        let states = Arc::new(state::States::new(app));
        app.vcs.register(self.id(), Arc::clone(&states) as _);
        if let Ok(mut held) = self.states.lock() {
            *held = Some(states);
        }
    }
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if keys.iter().any(|key| key == state::SHOWN)
            && let Some(states) = self.states()
        {
            states.settings_changed(app);
            app.wake();
        }
    }
    fn on_looked_at(&self, app: &App) {
        if let Some(states) = self.states() {
            states.looked_at();
            app.wake();
        }
    }
    fn on_disable(&self, _app: &App) {
        if let Ok(mut held) = self.states.lock() {
            *held = None;
        }
    }
}

impl GitPlugin {
    fn states(&self) -> Option<Arc<state::States>> {
        self.states.lock().ok()?.clone()
    }
}

struct Provider;

/// Whether the request is of a folder: a folder's has no bytes to type.
fn is_folder(request: &PreviewRequest) -> bool {
    request.mime.as_deref() == Some("inode/directory")
}

/// The repository the entry is in, and the entry as it names it.
fn located(request: &PreviewRequest) -> Option<(Repo, String)> {
    let path = &request.entry.path;
    let repo = Repo::find(path, is_folder(request))?;
    let relative = repo.relative(path)?;
    Some((repo, relative))
}

impl PreviewProvider for Provider {
    fn id(&self) -> &'static str {
        HISTORY
    }
    fn way(&self) -> Option<Way> {
        Some(Way {
            label: "History",
            tip: "Its history of commits, read from Git",
            icon: "history",
            folders: true,
        })
    }
    /// An entry in a repository's checked-out files, or a bare repository:
    /// found by the names a repository leaves, never by what the file holds.
    fn accepts(&self, request: &PreviewRequest) -> bool {
        located(request).is_some()
    }
    fn inspect(
        &self,
        request: &PreviewRequest,
        view: &InspectRequest,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Inspection>> {
        let (repo, path) =
            located(request).ok_or_else(|| anyhow::anyhow!("Not in a Git repository"))?;
        let log = git::log(&repo, &path, view.whole, cancel)?;
        Ok(Some(history(&log, &path, view.whole)))
    }
    /// A commit, as its patch under the entry: written in `into`, shown as
    /// the text it is below the history.
    fn extract(
        &self,
        request: &PreviewRequest,
        view: &InspectRequest,
        target: &str,
        into: &Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<Member>> {
        let (repo, path) =
            located(request).ok_or_else(|| anyhow::anyhow!("Not in a Git repository"))?;
        let (id, name) = asked(target, &path);
        let (written, _) = git::patch(&repo, id, name, into, view.whole, cancel)?;
        let name = written
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Some(Member {
            name,
            path: written,
            own_file: false,
        }))
    }
}

/// The commit a row's target opens, and the name its patch is read under:
/// the file's in that commit where it had another (`commit_target`), else
/// `path`, the entry's own.
fn asked<'a>(target: &'a str, path: &'a str) -> (&'a str, &'a str) {
    (commit_of(target), commit_name(target).unwrap_or(path))
}

/// A history's inspection: a row a commit, the graph of the merges down
/// the whole repository's, a word where no commit is.
fn history(log: &git::Log, path: &str, whole: bool) -> Inspection {
    let zone = jiff::tz::TimeZone::system();
    let graph = path.is_empty();
    let lanes = if graph {
        lanes::lay(
            log.commits
                .iter()
                .map(|commit| (commit.id.as_str(), commit.parents.as_slice())),
        )
    } else {
        Vec::new()
    };
    let mut lanes = lanes.into_iter();
    let rows: Vec<Row> = log
        .commits
        .iter()
        .map(|commit| Row {
            cells: vec![
                commit.summary.clone(),
                if commit.short.is_empty() {
                    commit.id.chars().take(7).collect()
                } else {
                    commit.short.clone()
                },
                commit.author.clone(),
                when(commit.time, &zone),
                names(&commit.refs, log.head.as_deref()),
            ],
            // Opened under the name the file had in it.
            target: Some(commit_target(&commit.id, commit.path.as_deref())),
            graph: lanes.next().map(Box::new),
            ..Row::default()
        })
        .collect();
    let mut notice = String::new();
    if log.stopped {
        notice = format!(
            "The newest {} commits: reading stopped before the oldest",
            rows.len()
        );
    } else if log.more {
        notice = format!("The newest {} commits", rows.len());
    }
    if log.names_cut {
        let more = "Some commits have more branches and tags than are shown";
        notice = if notice.is_empty() {
            more.to_owned()
        } else {
            format!("{notice}. {more}")
        };
    }
    let blocks = if rows.is_empty() {
        vec![Block {
            text: if path.is_empty() {
                "No commit yet".into()
            } else {
                "No commit changed it".into()
            },
            ..Block::default()
        }]
    } else {
        Vec::new()
    };
    let mut details = vec![skwir_core::inspection::Detail {
        label: "Commits".into(),
        value: if log.more || log.stopped {
            format!("{} shown", rows.len())
        } else {
            rows.len().to_string()
        },
        action: None,
    }];
    if let Some(head) = &log.head {
        details.push(skwir_core::inspection::Detail {
            label: "Branch".into(),
            value: head.clone(),
            action: None,
        });
    }
    Inspection {
        kind: InspectionKind::History,
        // What the bar's caption says is shown: where it is read from, and
        // the branch checked out.
        title: log.head.as_ref().map_or_else(
            || "Git history".to_owned(),
            |head| format!("Git history · {head}"),
        ),
        blocks,
        columns: vec![
            "Commit".into(),
            "Hash".into(),
            "Author".into(),
            "Date".into(),
            "Branches and tags".into(),
        ],
        rows,
        notice,
        details,
        whole_available: !whole && (log.more || log.stopped),
        ..Inspection::default()
    }
}

/// The names at a commit, as the row writes them (`RefKind::written`):
/// the helper's local branch (`main`), the one checked out marked so, a
/// remote-tracking one (`remote: origin/main`), a tag (`tag: v1`).
fn names(refs: &[String], head: Option<&str>) -> String {
    refs.iter()
        .map(|name| {
            if let Some(tag) = name.strip_prefix("tag: ") {
                RefKind::Tag.written(tag)
            } else if let Some(remote) = name.strip_prefix("remote: ") {
                RefKind::Remote.written(remote)
            } else if Some(name.as_str()) == head {
                RefKind::Current.written(name)
            } else {
                RefKind::Branch.written(name)
            }
        })
        .collect::<Vec<_>>()
        .join(REFS_JOIN)
}

/// When a commit was made, in the user's time zone, as the files' dates
/// are written (`YYYY-MM-DD HH:MM`).
fn when(seconds: i64, zone: &jiff::tz::TimeZone) -> String {
    jiff::Timestamp::from_second(seconds).map_or_else(
        |_| "—".into(),
        |at| {
            at.to_zoned(zone.clone())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{asked, history, when};
    use skwir_core::inspection::{InspectionKind, REFS_JOIN};
    use skwir_platform::git::{Commit, Log};

    fn commit(id: &str, parents: &[&str]) -> Commit {
        Commit {
            id: id.into(),
            short: id.into(),
            parents: parents.iter().map(|p| (*p).to_owned()).collect(),
            author: "A Person".into(),
            email: "a@example.org".into(),
            time: 1_700_000_000,
            offset: 0,
            summary: format!("Commit {id}"),
            refs: if id == "c" {
                vec![
                    "main".into(),
                    "remote: origin/main".into(),
                    "tag: v1".into(),
                ]
            } else {
                Vec::new()
            },
            path: None,
        }
    }

    /// The whole repository's history draws its graph; a folder's lists
    /// its commits without one; each row opens its commit; more left says
    /// so, and Full preview reads them.
    #[test]
    fn a_history_rows_its_commits_and_draws_the_whole_repository_s_graph() {
        let log = Log {
            head: Some("main".into()),
            commits: vec![commit("c", &["b"]), commit("b", &["a"]), commit("a", &[])],
            more: true,
            stopped: false,
            names_cut: false,
        };
        let whole = history(&log, "", false);
        assert_eq!(whole.kind, InspectionKind::History);
        assert_eq!(
            whole.title, "Git history · main",
            "its source and its branch"
        );
        assert_eq!(whole.rows.len(), 3);
        assert_eq!(whole.rows[0].target.as_deref(), Some("c"));
        assert_eq!(whole.rows[0].cells[0], "Commit c");
        assert_eq!(whole.rows[0].cells[1], "c", "its short id");
        assert_eq!(
            whole.rows[0].cells[4],
            ["head:main", "remote:origin/main", "tag:v1"].join(REFS_JOIN),
            "the branch checked out told apart"
        );
        assert!(whole.rows.iter().all(|row| row.graph.is_some()));
        assert!(whole.whole_available);
        assert_eq!(whole.notice, "The newest 3 commits");
        let folder = history(&log, "docs", true);
        assert!(folder.rows.iter().all(|row| row.graph.is_none()));
        assert!(!folder.whole_available, "asked whole");
        let none = history(
            &Log {
                head: None,
                commits: Vec::new(),
                more: false,
                stopped: false,
                names_cut: false,
            },
            "docs",
            false,
        );
        assert_eq!(none.blocks[0].text, "No commit changed it");
        // A commit with more names than said: the notice says so.
        let named = history(
            &Log {
                names_cut: true,
                ..log
            },
            "",
            false,
        );
        assert_eq!(
            named.notice,
            "The newest 3 commits. Some commits have more branches and tags than are shown"
        );
    }

    /// A commit from before the file's rename opens under the name the
    /// file had in it; the others under its own, the id copied whole.
    #[test]
    fn a_commit_opens_under_the_name_the_file_had_in_it() {
        let mut renamed = commit("b", &["a"]);
        renamed.path = Some("old name.txt".into());
        let log = Log {
            head: Some("main".into()),
            commits: vec![commit("c", &["b"]), renamed],
            more: false,
            stopped: false,
            names_cut: false,
        };
        let shown = history(&log, "docs/new.txt", false);
        let targets: Vec<&str> = shown
            .rows
            .iter()
            .map(|row| row.target.as_deref().unwrap())
            .collect();
        assert_eq!(targets, ["c", "b old name.txt"]);
        assert_eq!(asked(targets[0], "docs/new.txt"), ("c", "docs/new.txt"));
        assert_eq!(asked(targets[1], "docs/new.txt"), ("b", "old name.txt"));
    }

    #[test]
    fn a_commit_s_date_is_written_as_the_files_are() {
        let utc = jiff::tz::TimeZone::UTC;
        assert_eq!(when(1_700_000_000, &utc), "2023-11-14 22:13");
    }
}
