//! Where tagged files are, for a `tag:` search to consider first
//! (ADR-026): Baloo's index when Baloo runs, and an index of our own in
//! the cache (`index.rs`), learned from what the provider reads. Both
//! only hint: the search checks every file named and walks as it would
//! have.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bibli_core::cancel::CancelToken;
use bibli_core::entry::keys;
use bibli_core::hints::HintProvider;

/// The index as a hint provider.
pub struct Learned(pub Arc<crate::index::TagIndex>);

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
    stdout
        .split(|b| *b == b'\n')
        .filter(|line| line.starts_with(b"/"))
        .map(|line| PathBuf::from(bibli_core::os_path::from_bytes(line.to_vec())))
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

/// Spotlight, a Mac's file index, which indexes Finder's tags: asked with
/// `mdfind`, which every Mac has. Registered on macOS (compiled, never
/// run there); its query is read on every system.
#[cfg(target_os = "macos")]
pub struct Spotlight;

/// How long Spotlight may take: it answers from its index.
#[cfg(target_os = "macos")]
const SPOTLIGHT_TIMEOUT: Duration = Duration::from_secs(2);

/// Spotlight's query for files tagged `tag` (Finder's tags, any case);
/// `None` for a tag its query language would read otherwise (a quote, a
/// backslash, a wildcard).
#[cfg(any(target_os = "macos", test))]
fn spotlight_query(tag: &str) -> Option<String> {
    (!tag.contains(['"', '\\', '*', '?'])).then(|| format!("kMDItemUserTags == \"{tag}\"c"))
}

#[cfg(target_os = "macos")]
impl HintProvider for Spotlight {
    fn id(&self) -> &'static str {
        "tags.spotlight"
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
        let program = bibli_core::os_path::find_program("mdfind")?;
        let query = spotlight_query(value)?;
        let mut command = std::process::Command::new(program);
        command.arg("-onlyin").arg(root).arg(query);
        match bibli_platform::process::run(&mut command, cancel, SPOTLIGHT_TIMEOUT) {
            // One absolute path a line, as Baloo's.
            Ok(stdout) => Some(baloo_paths(&stdout)),
            Err(error) => {
                tracing::debug!(%error, "tags: Spotlight did not answer");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against a real Baloo of its own: a private D-Bus session, settings
    /// and index in a temporary directory, indexing only the fixture.
    /// Needs `baloo_file`, `baloosearch6` and `dbus-run-session`:
    /// `cargo nextest run -p bibli-plugin-tags --run-ignored only baloo`.
    #[cfg(target_os = "linux")]
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
                bibli_core::fs::xattr::set(&path, "user.xdg.tags", tags.as_bytes()).unwrap();
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

    #[test]
    fn spotlight_is_asked_for_finders_tags() {
        assert_eq!(
            spotlight_query("to do").as_deref(),
            Some("kMDItemUserTags == \"to do\"c")
        );
        assert_eq!(spotlight_query("a*"), None);
        assert_eq!(spotlight_query("say \"x\""), None);
    }
}
