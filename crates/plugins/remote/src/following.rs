//! How far a helper has come with each name it was given (ADR-135), where
//! it says nothing of each until it is whole (sftp in batch, gio, KIO):
//! read from the files it has open (`process::open_files`) as it runs.
//! A file it sends is read from its start, one it brings written from its
//! start, so how far into each it is, is how far its copy came; a file it
//! closed is done. Each file is known by the local folder it is in (where
//! the copy reads, where it writes, the folder a file is written in until
//! whole) and the name given it is under there: the first part of its
//! path below that folder, or the longest name given that part begins
//! with (a file written under a name of the tool's own, `a.bin.part`).
//! What was read ahead of what the server took is never said whole: the
//! meter keeps an item short of its end until its copy is told made. What
//! all came is told too, as the bytes copied so far (`Told::copied`),
//! which move the plan's bar and wake the window.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// How often a helper's open files are read.
const EVERY: Duration = Duration::from_millis(250);

/// What follows a copy's helpers (module documentation).
pub(crate) struct Following<'a> {
    /// The local folders the names given are in: what is read, what is
    /// written.
    roots: Vec<PathBuf>,
    names: Vec<OsString>,
    writing: &'a (dyn Fn(&OsStr, u64) + Sync),
    copied: &'a (dyn Fn(u64) + Sync),
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Each process's files open when it was last read, by path: of which
    /// name, and how far into it.
    open: HashMap<u32, HashMap<PathBuf, (usize, u64)>>,
    /// When each process was last read.
    read: HashMap<u32, Instant>,
    /// Of each name, the bytes of its files closed since.
    done: Vec<u64>,
    /// What was said of each name last: never less.
    said: Vec<u64>,
}

impl<'a> Following<'a> {
    /// What follows the copy of `names`, in the local folders `roots`,
    /// telling `told` how far each came (`writing`), and all (`copied`).
    pub(crate) fn new(
        roots: impl IntoIterator<Item = PathBuf>,
        names: &[OsString],
        told: &'a skwir_core::op::transfer::Told<'a>,
    ) -> Self {
        Following {
            roots: roots.into_iter().collect(),
            names: names.to_vec(),
            writing: told.writing,
            copied: told.copied,
            state: Mutex::new(State {
                done: vec![0; names.len()],
                said: vec![0; names.len()],
                ..State::default()
            }),
        }
    }

    /// Which name given `path` is under, if any.
    fn name_of(&self, path: &Path) -> Option<usize> {
        let first = self.roots.iter().find_map(|root| {
            path.strip_prefix(root)
                .ok()?
                .components()
                .next()
                .map(|c| c.as_os_str().to_os_string())
        })?;
        if let Some(at) = self.names.iter().position(|name| *name == first) {
            return Some(at);
        }
        // A file written under a name of the tool's own, its name first.
        let first = first.as_encoded_bytes();
        self.names
            .iter()
            .enumerate()
            .filter(|(_, name)| first.starts_with(name.as_encoded_bytes()))
            .max_by_key(|(_, name)| name.len())
            .map(|(at, _)| at)
    }

    /// Process `pid` of the copy runs: what it has open read, at most every
    /// `EVERY`, how far each name came told.
    pub(crate) fn look(&self, pid: u32) {
        let now = Instant::now();
        {
            let state = self.state.lock();
            if state.read.get(&pid).is_some_and(|at| now - *at < EVERY) {
                return;
            }
        }
        let open: HashMap<PathBuf, (usize, u64)> = skwir_platform::process::open_files(pid)
            .into_iter()
            .filter_map(|file| Some((self.name_of(&file.path)?, file)))
            .map(|(name, file)| (file.path, (name, file.at)))
            .collect();
        self.took(pid, open, now);
    }

    /// What process `pid` has open now (`open`, by name and offset), read
    /// at `now`: what it closed since is done, at its size.
    fn took(&self, pid: u32, open: HashMap<PathBuf, (usize, u64)>, now: Instant) {
        let mut moved: HashSet<usize> = open.values().map(|(name, _)| *name).collect();
        let mut state = self.state.lock();
        state.read.insert(pid, now);
        let was = state.open.insert(pid, open).unwrap_or_default();
        let closed: Vec<(PathBuf, usize, u64)> = was
            .into_iter()
            .filter(|(path, _)| {
                !state
                    .open
                    .get(&pid)
                    .is_some_and(|open| open.contains_key(path))
            })
            .map(|(path, (name, at))| (path, name, at))
            .collect();
        for (path, name, at) in closed {
            let size = std::fs::symlink_metadata(&path).map_or(at, |m| m.len());
            state.done[name] = state.done[name].saturating_add(size.max(at));
            moved.insert(name);
        }
        let mut told = Vec::new();
        for name in moved {
            let reading: u64 = state
                .open
                .values()
                .flat_map(HashMap::values)
                .filter(|(n, _)| *n == name)
                .map(|(_, at)| *at)
                .sum();
            let so_far = state.done[name].saturating_add(reading);
            if so_far > state.said[name] {
                state.said[name] = so_far;
                told.push((name, so_far));
            }
        }
        let all = (!told.is_empty()).then(|| state.said.iter().sum::<u64>());
        drop(state);
        for (name, so_far) in told {
            (self.writing)(&self.names[name], so_far);
        }
        if let Some(all) = all {
            (self.copied)(all);
        }
    }

    /// Process `pid` ended: what it had open is done.
    pub(crate) fn ended(&self, pid: u32) {
        self.took(pid, HashMap::new(), Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a follower tells, read back: each name's bytes, and all.
    fn told<'a>(
        writing: &'a (dyn Fn(&OsStr, u64) + Sync),
        copied: &'a (dyn Fn(u64) + Sync),
    ) -> skwir_core::op::transfer::Told<'a> {
        skwir_core::op::transfer::Told {
            copied,
            placed: &|_| {},
            made: &|_| {},
            writing,
        }
    }

    /// Each name's bytes so far from what the helpers have open: a file
    /// read as far as it is, one closed whole, under the name given or the
    /// longest a tool's own name begins with; what is elsewhere, nothing.
    /// All of them told as the bytes copied so far.
    #[test]
    fn how_far_each_name_came_is_read_from_the_files_open() {
        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("l'été");
        std::fs::create_dir_all(from.join("tree")).unwrap();
        std::fs::write(from.join("tree/x.bin"), vec![0u8; 100]).unwrap();
        std::fs::write(from.join("a.bin"), vec![0u8; 50]).unwrap();
        let said: Mutex<Vec<(String, u64)>> = Mutex::default();
        let writing = |name: &OsStr, bytes: u64| {
            said.lock()
                .push((name.to_string_lossy().into_owned(), bytes));
        };
        let all: Mutex<Vec<u64>> = Mutex::default();
        let copied = |bytes: u64| all.lock().push(bytes);
        let names: Vec<OsString> = ["tree", "a.bin", "a.bin.more"].map(OsString::from).to_vec();
        let told_by = told(&writing, &copied);
        let following = Following::new([from.clone()], &names, &told_by);
        let open = |files: &[(&str, u64)]| -> HashMap<PathBuf, (usize, u64)> {
            files
                .iter()
                .filter_map(|(rel, at)| {
                    let path = from.join(rel);
                    Some((path.clone(), (following.name_of(&path)?, *at)))
                })
                .collect()
        };
        let t = Instant::now();
        let said_now = || {
            let mut told = std::mem::take(&mut *said.lock());
            told.sort();
            told
        };
        following.took(1, open(&[("tree/x.bin", 40), ("a.bin.part", 10)]), t);
        assert_eq!(
            said_now(),
            [("a.bin".to_owned(), 10), ("tree".to_owned(), 40)]
        );
        // The tree's file closed: whole, at its size; the other read on.
        following.took(1, open(&[("a.bin.part", 30)]), t);
        assert_eq!(
            said_now(),
            [("a.bin".to_owned(), 30), ("tree".to_owned(), 100)]
        );
        // Nothing moved: nothing said.
        following.took(1, open(&[("a.bin.part", 30)]), t);
        assert!(said_now().is_empty());
        assert_eq!(*all.lock(), [50, 130], "all of them, as they came");
        assert_eq!(following.name_of(&root.path().join("elsewhere")), None);
        assert_eq!(
            following.name_of(&from.join("a.bin.more")),
            Some(2),
            "the longest"
        );
    }

    /// A helper that reads a file it sends a thousand bytes at a time,
    /// saying nothing: how far it came is read from the file it has open
    /// as it runs, then whole as it ends (a shell standing in for sftp).
    #[cfg(target_os = "linux")]
    #[test]
    fn how_far_a_helper_came_is_read_from_the_file_it_reads() {
        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("from");
        std::fs::create_dir_all(&from).unwrap();
        let file = from.join("it's sent.bin");
        std::fs::write(&file, vec![7u8; 5000]).unwrap();
        let said: Mutex<Vec<u64>> = Mutex::default();
        let writing = |_: &OsStr, bytes: u64| said.lock().push(bytes);
        let names = vec![OsString::from("it's sent.bin")];
        let told_by = told(&writing, &|_| {});
        let following = Following::new([from.clone()], &names, &told_by);
        let mut command = std::process::Command::new("sh");
        command.args([
            "-c",
            "exec 3< \"$1\"; for i in 1 2 3 4 5; do dd bs=1000 count=1 status=none <&3 >/dev/null; sleep 0.3; done",
            "sh",
        ]);
        command.arg(&file);
        let pid = std::sync::atomic::AtomicU32::new(0);
        let finished = skwir_platform::process::run_looking(
            &mut command,
            None,
            &skwir_core::CancelToken::new(),
            &|| false,
            &mut |_: &[u8]| {},
            &|at| {
                pid.store(at, std::sync::atomic::Ordering::Relaxed);
                following.look(at);
            },
        )
        .unwrap();
        assert!(finished.status.success());
        following.ended(pid.load(std::sync::atomic::Ordering::Relaxed));
        let said = said.into_inner();
        assert!(
            said.iter().any(|bytes| *bytes > 0 && *bytes < 5000),
            "followed as it read: {said:?}"
        );
        assert_eq!(said.last(), Some(&5000), "whole as it ended");
        assert!(
            said.windows(2).all(|two| two[0] < two[1]),
            "never back: {said:?}"
        );
    }
}
