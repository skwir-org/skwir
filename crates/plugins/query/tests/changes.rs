//! Exercise operation notifications through the public location provider.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::Duration;

use skwir_core::cancel::CancelToken;
use skwir_core::location::{ListingEvent, LocationProvider, OpenContext, OpenLocation};
use skwir_core::op::Change;
use skwir_core::scheduler::Scheduler;
use skwir_plugin_query::provider::{QueryProvider, Searches};

struct Search {
    open: Arc<dyn OpenLocation>,
    changes: Searches,
    scheduler: Scheduler,
}

impl Search {
    fn new(root: &Path, expression: &str) -> Self {
        let changes = Searches::default();
        let provider = QueryProvider::new(CancelToken::new(), changes.clone(), Weak::new());
        let scheduler = Scheduler::new(2);
        let ctx = OpenContext {
            scheduler: scheduler.clone(),
            cancel: CancelToken::new(),
            batch_size: 2,
            waker: None,
            metadata: Arc::default(),
            fields: Arc::default(),
            hints: Arc::default(),
            companions: Arc::default(),
        };
        let location = skwir_plugin_query::location_for(expression, Some(root));
        let open = provider.open(&location, ctx).unwrap();
        let search = Self {
            open,
            changes,
            scheduler,
        };
        assert!(search.scheduler.wait_idle(Duration::from_secs(10)));
        let events: Vec<_> = search.open.events().try_iter().collect();
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ListingEvent::Complete))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ListingEvent::Error(_)))
        );
        search
    }

    /// Incremental updates must not clear the listing or complete it again.
    fn changed(&self, changes: &[Change]) -> Vec<PathBuf> {
        self.changes.changed(changes);
        assert!(self.scheduler.wait_idle(Duration::from_secs(10)));
        let mut paths = Vec::new();
        for event in self.open.events().try_iter() {
            match event {
                ListingEvent::Added(entries) => {
                    assert!(entries.len() <= 2, "new descendants stream in batches");
                    paths.extend(
                        entries
                            .into_iter()
                            .map(|entry| self.open.path_of(entry.id).unwrap()),
                    );
                }
                ListingEvent::Renamed(entries) => {
                    paths.extend(
                        entries
                            .into_iter()
                            .map(|(_, entry)| self.open.path_of(entry.id).unwrap()),
                    );
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }
        paths.sort();
        paths
    }
}

fn tree(root: &Path) {
    std::fs::create_dir_all(root.join("sub/deep")).unwrap();
    for name in [
        "a.txt",
        "sub/b.txt",
        "sub/deep/c.txt",
        ".secret.txt",
        "other.png",
    ] {
        std::fs::write(root.join(name), "fixture").unwrap();
    }
    // Searches deliberately ignore these rules, for initial and later walks.
    std::fs::write(root.join(".ignore"), "a.txt\nsub\n").unwrap();
}

#[test]
fn arriving_directories_discover_descendants_for_copy_move_rename_and_restore() {
    for operation in ["copy", "move", "rename", "restore"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("search");
        std::fs::create_dir(&root).unwrap();
        let source = if operation == "rename" {
            root.join(".held")
        } else {
            tmp.path().join("held")
        };
        tree(&source);
        let search = Search::new(&root, "ext:txt");
        let target = root.join("arrived");
        let change = match operation {
            "copy" => {
                tree(&target);
                Change::Copy {
                    from: source,
                    to: target.clone(),
                }
            }
            "move" => {
                std::fs::rename(&source, &target).unwrap();
                Change::Move {
                    from: source,
                    to: target.clone(),
                }
            }
            "rename" => {
                std::fs::rename(&source, &target).unwrap();
                Change::Rename {
                    from: source,
                    to: target.clone(),
                }
            }
            "restore" => {
                std::fs::rename(source, &target).unwrap();
                Change::Restore {
                    path: target.clone(),
                }
            }
            _ => unreachable!(),
        };
        assert_eq!(
            search.changed(&[change]),
            [
                target.join("a.txt"),
                target.join("sub/b.txt"),
                target.join("sub/deep/c.txt")
            ],
            "{operation}"
        );
    }
}

#[test]
fn arriving_subtrees_keep_search_depth_hidden_rules_and_root_boundary() {
    for (expression, names) in [
        ("ext:txt depth:1", vec![]),
        ("ext:txt depth:2", vec!["a.txt"]),
        ("ext:txt depth:2 hidden:true", vec![".secret.txt", "a.txt"]),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("search");
        std::fs::create_dir(&root).unwrap();
        let search = Search::new(&root, expression);
        let target = root.join("arrived");
        let outside = tmp.path().join("outside");
        tree(&target);
        tree(&outside);
        let changes = [
            Change::Restore {
                path: target.clone(),
            },
            Change::Restore {
                path: target.join("sub"),
            },
            Change::Restore { path: outside },
        ];
        assert_eq!(
            search.changed(&changes),
            names
                .iter()
                .map(|name| target.join(name))
                .collect::<Vec<_>>(),
            "{expression}"
        );
    }
}

#[test]
fn known_descendants_of_a_renamed_folder_are_not_published_twice() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("before");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("a.txt"), "fixture").unwrap();
    let search = Search::new(tmp.path(), "ext:txt");
    let target = tmp.path().join("after");
    std::fs::rename(&source, &target).unwrap();
    assert_eq!(
        search.changed(&[Change::Rename {
            from: source,
            to: target.clone()
        }]),
        [target.join("a.txt")]
    );
}

#[test]
fn closing_a_search_stops_an_arriving_subtree_from_publishing() {
    let tmp = tempfile::tempdir().unwrap();
    let search = Search::new(tmp.path(), "ext:txt");
    let target = tmp.path().join("arrived");
    std::fs::create_dir(&target).unwrap();
    for n in 0..256 {
        std::fs::write(target.join(format!("file-{n}.txt")), "fixture").unwrap();
    }
    search.changes.changed(&[Change::Restore { path: target }]);
    let first = search
        .open
        .events()
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(matches!(first, ListingEvent::Added(entries) if entries.len() == 2));
    search.open.close();
    // Events queued before close remain valid; none may arrive after it.
    search.open.events().try_iter().for_each(drop);
    assert!(search.scheduler.wait_idle(Duration::from_secs(10)));
    assert!(search.open.events().is_empty());
}

#[cfg(unix)]
#[test]
fn arriving_directory_links_are_not_walked_at_the_root_or_below_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("search");
    std::fs::create_dir(&root).unwrap();
    let outside = tmp.path().join("outside");
    tree(&outside);
    let search = Search::new(&root, "ext:txt");
    let link = root.join("linked");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    let target = root.join("arrived");
    std::fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&outside, target.join("nested-link")).unwrap();
    assert!(
        search
            .changed(&[
                Change::Restore { path: link.clone() },
                Change::Restore {
                    path: link.join("sub")
                },
                Change::Restore { path: target }
            ])
            .is_empty()
    );
}
