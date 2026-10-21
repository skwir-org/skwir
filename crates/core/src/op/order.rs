//! Ordering a batch of renames so every step's target is free when it runs.
//!
//! Bulk rename and edit-as-buffer both turn "these entries get these names"
//! into a plan. A rename whose target is another item's current name must
//! wait for that item to move; a cycle (a→b, b→a) parks one item under a
//! free temporary name. Items are keyed by (directory, name), so a selection
//! spanning folders never confuses two `notes.txt`.
//!
//! Each item wants at most one other item's name and targets are unique, so
//! the dependencies form chains and simple cycles: one pass over each, O(n).

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::{EntryRef, Rename, valid_file_name};

/// One wanted rename: an entry and its new name in the same directory.
#[derive(Clone, Debug)]
pub struct RenameRequest {
    pub entry: EntryRef,
    pub new_name: OsString,
}

/// Why a batch of renames cannot be ordered. `item` indexes the requests.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RenameOrderError {
    #[error("{reason}")]
    InvalidName { item: usize, reason: String },
    #[error("several items would be named {}", target.display())]
    Duplicate { items: Vec<usize>, target: PathBuf },
    #[error("“{}” is already there", target.file_name().unwrap_or(target.as_os_str()).display())]
    Conflict { item: usize, target: PathBuf },
}

type Key<'a> = (&'a Path, &'a OsStr);

fn key(path: &Path) -> Key<'_> {
    (
        path.parent().unwrap_or(Path::new("/")),
        path.file_name().unwrap_or_default(),
    )
}

/// The renames of `requests` in an order that never renames onto an
/// occupied name. A request whose name does not change produces no step,
/// and its name stays taken. `exists` says whether a path not held by any
/// request is taken; it is asked only about targets and temporary names.
///
/// # Errors
/// An invalid name, two requests with the same target, or a target taken by
/// something that is not moving away.
pub fn order_renames(
    requests: &[RenameRequest],
    exists: impl Fn(&Path) -> bool,
) -> Result<Vec<Rename>, RenameOrderError> {
    let targets: Vec<PathBuf> = requests
        .iter()
        .map(|r| {
            r.entry
                .path
                .parent()
                .unwrap_or(Path::new("/"))
                .join(&r.new_name)
        })
        .collect();
    let moving: Vec<bool> = requests
        .iter()
        .map(|r| r.entry.path.file_name() != Some(r.new_name.as_os_str()))
        .collect();
    let owner: HashMap<Key, usize> = requests
        .iter()
        .enumerate()
        .map(|(i, r)| (key(&r.entry.path), i))
        .collect();

    let mut claimed: HashMap<Key, Vec<usize>> = HashMap::new();
    for (i, r) in requests.iter().enumerate() {
        valid_file_name(&r.new_name).map_err(|e| RenameOrderError::InvalidName {
            item: i,
            reason: e.to_string(),
        })?;
        claimed.entry(key(&targets[i])).or_default().push(i);
    }
    if let Some(items) = claimed
        .into_values()
        .filter(|items| items.len() > 1)
        .min_by_key(|items| items[0])
    {
        return Err(RenameOrderError::Duplicate {
            target: targets[items[0]].clone(),
            items,
        });
    }

    // wants[i] = j: i's target is j's current name, so j moves first.
    let mut wants: Vec<Option<usize>> = vec![None; requests.len()];
    let mut wanted_by: Vec<Option<usize>> = vec![None; requests.len()];
    for i in (0..requests.len()).filter(|&i| moving[i]) {
        match owner.get(&key(&targets[i])) {
            // An item keeping its name claims it as its target, so taking
            // it was reported as a duplicate above: `j` is moving.
            Some(&j) => {
                wants[i] = Some(j);
                wanted_by[j] = Some(i);
            }
            None if exists(&targets[i]) => {
                return Err(RenameOrderError::Conflict {
                    item: i,
                    target: targets[i].clone(),
                });
            }
            None => {}
        }
    }

    let mut current: Vec<PathBuf> = requests.iter().map(|r| r.entry.path.clone()).collect();
    let mut steps = Vec::with_capacity(requests.len());
    let mut done = vec![false; requests.len()];
    let mut step = |i: usize, to: &Path, current: &mut Vec<PathBuf>| {
        steps.push(Rename {
            entry: EntryRef::new(requests[i].entry.id, current[i].clone())
                .with_only_name(requests[i].entry.only_name),
            new_name: to.file_name().unwrap_or_default().to_os_string(),
        });
        current[i] = to.to_path_buf();
    };

    // Chains: an item whose target is free goes first, then whoever wanted
    // its old name, and so on.
    for root in (0..requests.len()).filter(|&i| moving[i] && wants[i].is_none()) {
        let mut next = Some(root);
        while let Some(i) = next {
            step(i, &targets[i], &mut current);
            done[i] = true;
            next = wanted_by[i];
        }
    }

    // What is left are cycles. Park one item, which frees its name for the
    // rest of its cycle, then give it its final name last.
    let mut taken: HashSet<PathBuf> = requests
        .iter()
        .map(|r| r.entry.path.clone())
        .chain(targets.iter().cloned())
        .collect();
    let mut serial = 0u32;
    for start in 0..requests.len() {
        if !moving[start] || done[start] {
            continue;
        }
        let dir = current[start]
            .parent()
            .unwrap_or(Path::new("/"))
            .to_path_buf();
        let parked = loop {
            serial += 1;
            let candidate = dir.join(format!(".skwir-rename-{serial}"));
            if !taken.contains(&candidate) && !exists(&candidate) {
                break candidate;
            }
        };
        taken.insert(parked.clone());
        step(start, &parked, &mut current);
        let mut i = wanted_by[start].expect("a cycle");
        while i != start {
            step(i, &targets[i], &mut current);
            done[i] = true;
            i = wanted_by[i].expect("a cycle");
        }
        step(start, &targets[start], &mut current);
        done[start] = true;
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(n: u64, path: &str, to: &str) -> RenameRequest {
        RenameRequest {
            entry: EntryRef::new(crate::FileId::new(1, n), path),
            new_name: to.into(),
        }
    }

    /// Run the steps against a model directory tree: every step must find
    /// its entry at its hint and land on a free name.
    fn simulate(requests: &[RenameRequest], steps: &[Rename]) -> HashMap<PathBuf, u64> {
        let mut tree: HashMap<PathBuf, u64> = requests
            .iter()
            .map(|r| (r.entry.path.clone(), r.entry.id.ino))
            .collect();
        for s in steps {
            let id = tree.remove(&s.entry.path).expect("entry at its hint");
            assert_eq!(id, s.entry.id.ino);
            let to = s.entry.path.parent().unwrap().join(&s.new_name);
            assert!(
                tree.insert(to.clone(), id).is_none(),
                "{} was taken",
                to.display()
            );
        }
        tree
    }

    fn expect_final(requests: &[RenameRequest], tree: &HashMap<PathBuf, u64>) {
        for r in requests {
            let to = r.entry.path.parent().unwrap().join(&r.new_name);
            assert_eq!(tree.get(&to), Some(&r.entry.id.ino), "{}", to.display());
        }
    }

    #[test]
    fn a_shift_orders_in_one_pass() {
        // f1..fN -> f2..fN+1: each waits for the next, so the last goes first.
        let n = 2_000;
        let requests: Vec<_> = (1..=n)
            .map(|i| request(i, &format!("/d/f{i}"), &format!("f{}", i + 1)))
            .collect();
        let steps = order_renames(&requests, |_| false).unwrap();
        assert_eq!(steps.len(), n as usize);
        expect_final(&requests, &simulate(&requests, &steps));
    }

    #[test]
    fn cycles_park_one_item_under_a_free_name() {
        let requests = vec![
            request(1, "/d/a", "b"),
            request(2, "/d/b", "c"),
            request(3, "/d/c", "a"),
            request(4, "/d/x", "y"),
            request(5, "/d/y", "x"),
        ];
        // The first temporary name is taken by an unrelated file.
        let steps = order_renames(&requests, |p| p == Path::new("/d/.skwir-rename-1")).unwrap();
        assert_eq!(steps.len(), 7, "one parking step per cycle");
        assert!(steps.iter().all(|s| s.new_name != ".skwir-rename-1"));
        expect_final(&requests, &simulate(&requests, &steps));
    }

    #[test]
    fn items_in_different_folders_do_not_collide() {
        let requests = vec![
            request(1, "/one/notes", "old"),
            request(2, "/two/old", "notes"),
            request(3, "/two/notes", "older"),
        ];
        let steps = order_renames(&requests, |_| false).unwrap();
        expect_final(&requests, &simulate(&requests, &steps));
    }

    #[test]
    fn duplicates_conflicts_and_bad_names_are_reported() {
        let dup = [request(1, "/d/a", "z"), request(2, "/d/b", "z")];
        assert!(matches!(
            order_renames(&dup, |_| false),
            Err(RenameOrderError::Duplicate { items, .. }) if items == [0, 1]
        ));
        // An unchanged item keeps its name.
        let kept = [request(1, "/d/a", "b"), request(2, "/d/b", "b")];
        assert!(matches!(
            order_renames(&kept, |_| false),
            Err(RenameOrderError::Duplicate { items, .. }) if items == [0, 1]
        ));
        let outside = [request(1, "/d/a", "b")];
        assert!(matches!(
            order_renames(&outside, |p| p == Path::new("/d/b")),
            Err(RenameOrderError::Conflict { item: 0, .. })
        ));
        for bad in ["", ".", "..", "x/y", "nul\0"] {
            assert!(matches!(
                order_renames(&[request(1, "/d/a", bad)], |_| false),
                Err(RenameOrderError::InvalidName { item: 0, .. })
            ));
        }
    }
}
