//! Where tagged files are, for a `tag:` search to consider first
//! (ADR-026): Baloo's index when Baloo runs, and an index of our own in
//! the cache, learned from what the provider reads. Both only hint: the
//! search checks every file named and walks as it would have.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use bibli_core::app::App;
use bibli_core::cache::CacheSlot;
use bibli_core::cancel::CancelToken;
use bibli_core::entry::keys;
use bibli_core::hints::HintProvider;
use bibli_core::scheduler::Priority;
use parking_lot::{Mutex, MutexGuard};

/// Tagged files remembered at most. Past it, new ones are not learned (a
/// search still finds them by walking); those known are kept up to date.
const AT_MOST: usize = 50_000;
const KEY: &[u8] = b"index";

#[derive(Default)]
struct State {
    /// Each tagged file's tags, in lowercase: tags ignore case.
    tags: HashMap<PathBuf, Vec<String>>,
    loaded: bool,
}

/// The tagged files bibli has seen: every file the provider reads tells
/// it whether the file has tags (a folder shown, a search's walk, a file
/// changed and read again), so an edit, here or elsewhere, is learned the
/// next time the file is read. Deleting it (it lives in the deletable
/// cache) only makes the first answers of the next search slower.
pub struct TagIndex {
    state: Mutex<State>,
    slot: Option<CacheSlot>,
    app: Weak<App>,
    lifetime: CancelToken,
    writing: AtomicBool,
}

impl TagIndex {
    pub fn new(slot: Option<CacheSlot>, app: &App, lifetime: CancelToken) -> Arc<Self> {
        Arc::new(TagIndex {
            state: Mutex::default(),
            slot,
            app: app.weak(),
            lifetime,
            writing: AtomicBool::new(false),
        })
    }

    /// The index, read from the cache the first time it is needed: on the
    /// worker that needs it, never at startup.
    fn state(&self) -> MutexGuard<'_, State> {
        let mut state = self.state.lock();
        if !state.loaded {
            state.loaded = true;
            let stored: Vec<(String, Vec<String>)> = self
                .slot
                .as_ref()
                .and_then(|slot| slot.get(KEY))
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_default();
            state.tags = stored
                .into_iter()
                .map(|(path, tags)| (PathBuf::from(path), tags))
                .collect();
        }
        state
    }

    /// What the provider read of files: each path with its tags, none for
    /// a file without. Writes the index soon when that changed it.
    pub fn learn(self: &Arc<Self>, read: Vec<(PathBuf, Vec<String>)>) {
        let changed = {
            let mut state = self.state();
            let mut changed = false;
            for (path, tags) in read {
                if tags.is_empty() {
                    changed |= state.tags.remove(&path).is_some();
                    continue;
                }
                let tags: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
                let full = state.tags.len() >= AT_MOST;
                match state.tags.get_mut(&path) {
                    Some(known) if *known == tags => {}
                    Some(known) => {
                        *known = tags;
                        changed = true;
                    }
                    None if !full => {
                        state.tags.insert(path, tags);
                        changed = true;
                    }
                    None => {}
                }
            }
            changed
        };
        if changed {
            self.write_soon();
        }
    }

    /// The files under `root` known to have `tag`. Those gone since are
    /// forgotten on the way.
    pub fn files_with(self: &Arc<Self>, tag: &str, root: &Path) -> Vec<PathBuf> {
        let tag = tag.to_lowercase();
        let found: Vec<PathBuf> = self
            .state()
            .tags
            .iter()
            .filter(|(path, tags)| path.starts_with(root) && tags.contains(&tag))
            .map(|(path, _)| path.clone())
            .collect();
        let (here, gone): (Vec<PathBuf>, Vec<PathBuf>) = found.into_iter().partition(|path| {
            !matches!(std::fs::symlink_metadata(path),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        });
        if !gone.is_empty() {
            let mut state = self.state();
            for path in &gone {
                state.tags.remove(path);
            }
            drop(state);
            self.write_soon();
        }
        here
    }

    fn write_soon(self: &Arc<Self>) {
        let Some(app) = self.app.upgrade().filter(|_| self.slot.is_some()) else {
            return;
        };
        if self.writing.swap(true, Ordering::AcqRel) {
            return;
        }
        let this = Arc::clone(self);
        app.scheduler.spawn(
            Priority::Background,
            "tags.index",
            self.lifetime.child(),
            move || {
                this.writing.store(false, Ordering::Release);
                this.write();
            },
        );
    }

    fn write(&self) {
        let Some(slot) = &self.slot else { return };
        let bytes = {
            let state = self.state();
            // A path that is not text stays out: a walk finds it.
            let stored: Vec<(&str, &Vec<String>)> = state
                .tags
                .iter()
                .filter_map(|(path, tags)| Some((path.to_str()?, tags)))
                .collect();
            serde_json::to_vec(&stored)
        };
        if let Ok(bytes) = bytes
            && let Err(error) = slot.put(KEY, &bytes)
        {
            tracing::debug!(%error, "tags: the index was not written");
        }
    }
}

/// The index as a hint provider.
pub struct Learned(pub Arc<TagIndex>);

impl HintProvider for Learned {
    fn id(&self) -> &'static str {
        "tags.index"
    }
    fn candidates(
        &self,
        key: &str,
        value: &str,
        root: &Path,
        _: &CancelToken,
    ) -> Option<Vec<PathBuf>> {
        (key == keys::TAGS).then(|| self.0.files_with(value, root))
    }
}

/// Baloo, KDE's file index, which indexes the desktop's tags (ours), when
/// the user runs it: asked with `baloosearch`, never started by us.
#[derive(Default)]
pub struct Baloo {
    /// Where its settings and its index are; the user's when `None`.
    pub private: Option<PrivateBaloo>,
}

/// A Baloo of its own (tests): its settings file and its data directory.
pub struct PrivateBaloo {
    pub config: PathBuf,
    pub data_home: PathBuf,
}

/// How long Baloo may take: it answers in milliseconds from its index.
const BALOO_TIMEOUT: Duration = Duration::from_secs(2);

impl Baloo {
    /// `baloosearch`, when Baloo is on: its settings say indexing is
    /// enabled (an index left by a Baloo turned off is out of date).
    fn search_program(&self) -> Option<PathBuf> {
        let config = match &self.private {
            Some(private) => private.config.clone(),
            None => bibli_platform::xdg::config_home()?.join("baloofilerc"),
        };
        let text = std::fs::read_to_string(config).ok()?;
        if !indexing_enabled(&text) {
            return None;
        }
        ["baloosearch6", "baloosearch"]
            .into_iter()
            .find_map(bibli_core::os_path::find_program)
    }
}

/// Whether a `baloofilerc` says indexing is on (the default when unsaid).
fn indexing_enabled(config: &str) -> bool {
    let mut section = "";
    for line in config.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name;
        } else if section == "Basic Settings"
            && let Some(value) = line.strip_prefix("Indexing-Enabled=")
        {
            return !value.trim().eq_ignore_ascii_case("false");
        }
    }
    true
}

/// Baloo's query for files tagged `tag`; `None` for a tag its query
/// language cannot quote.
fn baloo_query(tag: &str) -> Option<String> {
    (!tag.contains('"') && !tag.contains('\\')).then(|| format!("tag:\"{tag}\""))
}

/// The paths in `baloosearch`'s output: one per line, then a line of
/// timing in the user's language, which is not a path.
fn baloo_paths(stdout: &[u8]) -> Vec<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    stdout
        .split(|b| *b == b'\n')
        .filter(|line| line.starts_with(b"/"))
        .map(|line| PathBuf::from(std::ffi::OsStr::from_bytes(line)))
        .collect()
}

impl HintProvider for Baloo {
    fn id(&self) -> &'static str {
        "tags.baloo"
    }
    fn candidates(
        &self,
        key: &str,
        value: &str,
        root: &Path,
        cancel: &CancelToken,
    ) -> Option<Vec<PathBuf>> {
        if key != keys::TAGS {
            return None;
        }
        let program = self.search_program()?;
        let query = baloo_query(value)?;
        let mut command = std::process::Command::new(program);
        command.arg("-d").arg(root).arg(query);
        if let Some(private) = &self.private {
            command.env("XDG_DATA_HOME", &private.data_home);
        }
        match bibli_platform::process::run(&mut command, cancel, BALOO_TIMEOUT) {
            Ok(stdout) => Some(baloo_paths(&stdout)),
            Err(error) => {
                tracing::debug!(%error, "tags: Baloo did not answer");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Arc<TagIndex> {
        Arc::new(TagIndex {
            state: Mutex::new(State {
                tags: HashMap::new(),
                loaded: true,
            }),
            slot: None,
            app: Weak::new(),
            lifetime: CancelToken::new(),
            writing: AtomicBool::new(false),
        })
    }

    #[test]
    fn the_index_learns_tags_and_their_removal_and_forgets_the_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("it's a"), tmp.path().join("b"));
        std::fs::write(&a, b"x").unwrap();
        std::fs::write(&b, b"x").unwrap();
        let index = index();
        index.learn(vec![
            (a.clone(), vec!["Work".into(), "home".into()]),
            (b.clone(), vec!["work".into()]),
        ]);
        let mut found = index.files_with("WORK", tmp.path());
        found.sort();
        assert_eq!(found, [b.clone(), a.clone()]);
        assert_eq!(
            index.files_with("home", tmp.path()),
            std::slice::from_ref(&a)
        );
        assert!(
            index
                .files_with("home", &tmp.path().join("elsewhere"))
                .is_empty()
        );
        // Read again without tags: forgotten.
        index.learn(vec![(a.clone(), Vec::new())]);
        assert_eq!(
            index.files_with("work", tmp.path()),
            std::slice::from_ref(&b)
        );
        // Gone from the disk: forgotten when asked.
        std::fs::remove_file(&b).unwrap();
        assert!(index.files_with("work", tmp.path()).is_empty());
        assert!(index.state().tags.is_empty());
    }

    #[test]
    fn past_its_bound_it_learns_no_new_file_but_keeps_those_known_current() {
        let index = index();
        let read: Vec<(PathBuf, Vec<String>)> = (0..AT_MOST + 3)
            .map(|i| (PathBuf::from(format!("/r/{i}")), vec!["t".into()]))
            .collect();
        index.learn(read);
        assert_eq!(index.state().tags.len(), AT_MOST);
        index.learn(vec![(PathBuf::from("/r/0"), vec!["u".into()])]);
        assert_eq!(index.state().tags[Path::new("/r/0")], ["u"]);
    }

    /// Against a real Baloo of its own: a private D-Bus session, settings
    /// and index in a temporary directory, indexing only the fixture.
    /// Needs `baloo_file`, `baloosearch6` and `dbus-run-session`:
    /// `cargo nextest run -p bibli-plugin-tags --run-ignored only baloo`.
    #[test]
    #[ignore = "starts a private Baloo"]
    fn a_private_baloo_names_the_tagged_files() {
        use std::os::unix::process::CommandExt;
        let search = bibli_core::os_path::find_program("baloosearch6").expect("baloosearch6");
        let daemon = std::fs::canonicalize(&search)
            .ok()
            .and_then(|real| Some(real.parent()?.parent()?.join("libexec/kf6/baloo_file")))
            .filter(|p| p.exists())
            .or_else(|| {
                [
                    "/usr/libexec/kf6/baloo_file",
                    "/usr/lib/x86_64-linux-gnu/libexec/kf6/baloo_file",
                ]
                .into_iter()
                .map(PathBuf::from)
                .find(|p| p.exists())
            })
            .expect("baloo_file");
        let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::Builder::new()
            .prefix("baloo-")
            .tempdir_in(base)
            .unwrap();
        let tree = tmp.path().join("tree");
        let nested = tree.join("it's nested");
        std::fs::create_dir_all(&nested).unwrap();
        for (path, tags) in [
            (tree.join("a.txt"), Some("work,to do")),
            (nested.join("b.txt"), Some("Work")),
            (tree.join("c.txt"), Some("home")),
            (tree.join("d.txt"), None),
        ] {
            std::fs::write(&path, b"x").unwrap();
            if let Some(tags) = tags {
                xattr::set(&path, "user.xdg.tags", tags.as_bytes()).unwrap();
            }
        }
        let (config, data, cache, run) = (
            tmp.path().join("config"),
            tmp.path().join("data"),
            tmp.path().join("cache"),
            tmp.path().join("run"),
        );
        for dir in [&config, &data, &cache, &run] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let settings = config.join("baloofilerc");
        std::fs::write(
            &settings,
            format!(
                "[Basic Settings]\nIndexing-Enabled=true\n\n[General]\nfolders[$e]={}/\nonly basic indexing=true\n",
                tree.display()
            ),
        )
        .unwrap();
        let mut child = std::process::Command::new("dbus-run-session")
            .arg("--")
            .arg(&daemon)
            .env_remove("DBUS_SESSION_BUS_ADDRESS")
            .env("XDG_CONFIG_HOME", &config)
            .env("XDG_DATA_HOME", &data)
            .env("XDG_CACHE_HOME", &cache)
            .env("XDG_RUNTIME_DIR", &run)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()
            .expect("dbus-run-session");
        let baloo = Baloo {
            private: Some(PrivateBaloo {
                config: settings,
                data_home: data,
            }),
        };
        let cancel = CancelToken::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut found = Vec::new();
        while std::time::Instant::now() < deadline {
            found = baloo
                .candidates(keys::TAGS, "WORK", &tree, &cancel)
                .unwrap_or_default();
            if found.len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let only_nested = baloo.candidates(keys::TAGS, "work", &nested, &cancel);
        let two_words = baloo.candidates(keys::TAGS, "to do", &tree, &cancel);
        // The daemon and its bus, whatever the assertions say.
        let group = rustix::process::Pid::from_child(&child);
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::TERM);
        let _ = child.wait();
        found.sort();
        assert_eq!(found, [tree.join("a.txt"), nested.join("b.txt")]);
        assert_eq!(only_nested.unwrap(), [nested.join("b.txt")]);
        assert_eq!(two_words.unwrap(), [tree.join("a.txt")]);
    }

    #[test]
    fn baloo_is_asked_only_while_it_indexes_and_its_answers_are_paths() {
        assert!(indexing_enabled("[General]\nfolders[$e]=$HOME/\n"));
        assert!(indexing_enabled(
            "[Basic Settings]\nIndexing-Enabled=true\n"
        ));
        assert!(!indexing_enabled(
            "[General]\nIndexing-Enabled=true\n[Basic Settings]\nIndexing-Enabled=false\n"
        ));
        assert_eq!(baloo_query("to do").unwrap(), "tag:\"to do\"");
        assert_eq!(baloo_query("a\"b"), None);
        assert_eq!(
            baloo_paths(b"/r/it's a.txt\n/r/b\nTemps \xc3\xa9coul\xc3\xa9 : 0,1 ms\n"),
            [PathBuf::from("/r/it's a.txt"), PathBuf::from("/r/b")]
        );
    }
}
