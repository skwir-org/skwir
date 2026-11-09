//! What the user's login shell exports, for the variables the exclusions'
//! paths start from (ADR-125): a skwir started from the desktop inherits
//! the session's environment, not what `.zshrc` or `config.fish` export
//! (`CARGO_HOME`, `GNUPGHOME`). Core reads the values
//! (`skwir_core::environment`); this asks for them.
//!
//! The shell runs on a worker, never the window's thread, with a timeout
//! and its process group stopped whole (an `rc` that starts tmux); it is
//! asked again only once its files change, their stamps kept with the
//! answer in skwir's cache, a failure too: a shell whose files take longer
//! than it may is not run at every start. Only the names asked for are
//! kept, in memory and in the cache: the rest of the environment holds
//! tokens. Off with `exclusions.shell_variables`, which takes effect at
//! once. Windows has no login shell: a program inherits the user's
//! variables there.

#[cfg(any(unix, test))]
use std::collections::{BTreeSet, HashMap};
#[cfg(any(unix, test))]
use std::ffi::OsString;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::sync::Arc;

use skwir_core::app::App;

#[cfg(unix)]
/// The cache's file, in skwir's cache folder.
const KEPT: &str = "shell-environment.json";

/// How long the shell may take: a framework's prompt may take seconds.
#[cfg(unix)]
const AT_MOST: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(unix)]
/// What the shell said, and of what, as kept in the cache.
#[derive(serde::Serialize, serde::Deserialize, Default, PartialEq, Eq, Debug)]
struct Kept {
    shell: PathBuf,
    stamps: Vec<Stamp>,
    asked: BTreeSet<String>,
    /// Their values' bytes (`OsStr::as_encoded_bytes`).
    values: HashMap<String, Vec<u8>>,
    /// Where zsh found its files, as the shell said (its bytes): what
    /// `.zshenv` sets, and a desktop's start has not.
    #[serde(default)]
    zdotdir: Option<Vec<u8>>,
    /// The shell did not answer when last asked: the values are those of
    /// the last answer, and it is asked again once its files change.
    #[serde(default)]
    failed: bool,
}

#[cfg(unix)]
/// One of the shell's files as it stood: where a link leads (a dotfile
/// manager's, Home Manager's store) and when it last changed.
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug, Clone)]
struct Stamp {
    path: PathBuf,
    target: Option<PathBuf>,
    modified: Option<u128>,
    length: Option<u64>,
}

/// Learn what the cache kept at once, then follow the shell: the asker
/// core calls for names not asked yet, and a check on a worker that the
/// shell's files have not changed since. The setting off, the asker stands
/// waiting for it and nothing is read; nothing where the system has no
/// login shell.
pub fn follow(app: &Arc<App>) {
    #[cfg(unix)]
    {
        let Some(shell) = login_shell() else {
            return;
        };
        let kept = read(app)
            .filter(|kept| kept.shell == shell)
            .filter(|_| enabled(app));
        if let Some(kept) = &kept {
            skwir_core::environment::learn(kept.asked.clone(), values_of(kept));
        }
        let weak = Arc::downgrade(app);
        let asking = Arc::new(parking_lot::Mutex::new(()));
        {
            let (weak, asking, shell) = (weak.clone(), Arc::clone(&asking), shell.clone());
            skwir_core::environment::set_asker(move |names| {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                let (weak, asking, shell) = (weak.clone(), Arc::clone(&asking), shell.clone());
                app.scheduler.spawn(
                    skwir_core::scheduler::Priority::Background,
                    "shell-environment",
                    app.lifetime().child(),
                    move || refresh(&weak, &asking, &shell, names),
                );
            });
        }
        // The cache's answer, if any, may be stale.
        if let Some(kept) = kept {
            let lifetime = app.lifetime().child();
            app.scheduler.spawn(
                skwir_core::scheduler::Priority::Background,
                "shell-environment-check",
                lifetime,
                move || {
                    if stamps(&shell, zdotdir_of(Some(&kept)).as_deref()) != kept.stamps {
                        refresh(&weak, &asking, &shell, kept.asked);
                    }
                },
            );
        }
    }
    #[cfg(not(unix))]
    let _ = app;
}

#[cfg(unix)]
fn enabled(app: &App) -> bool {
    app.settings
        .get::<bool>(skwir_core::exclusions::SHELL_VARIABLES_KEY)
        .unwrap_or(true)
}

/// The user's login shell: `$SHELL`, a whole path.
#[cfg(unix)]
fn login_shell() -> Option<PathBuf> {
    std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|shell| shell.is_absolute())
}

/// Ask the shell for `names`, one asking at a time, and learn and keep
/// what it says; not at all where what was kept still stands (its files
/// did not change, and it answered for these names or did not answer).
#[cfg(unix)]
fn refresh(
    weak: &std::sync::Weak<App>,
    asking: &parking_lot::Mutex<()>,
    shell: &Path,
    names: BTreeSet<String>,
) {
    let _one = asking.lock();
    let Some(app) = weak.upgrade() else {
        return;
    };
    if names.is_empty() || !enabled(&app) {
        return;
    }
    let before = read(&app).filter(|kept| kept.shell == shell);
    let looked_in = zdotdir_of(before.as_ref());
    let mut stamped = stamps(shell, looked_in.as_deref());
    if let Some(before) = &before
        && before.stamps == stamped
        && (before.failed || names.is_subset(&before.asked))
    {
        let asked = before.asked.union(&names).cloned().collect();
        if skwir_core::environment::learn(asked, values_of(before)) {
            app.wake();
        }
        return;
    }
    let cancel = app.lifetime().child();
    let said = match ask(shell, &names, &cancel) {
        Ok(said) => said,
        Err(error) => {
            tracing::debug!("the login shell said no variables: {error:#}");
            // Skwir closing is no answer of the shell's.
            if !cancel.is_cancelled() {
                remember_failure(&app, shell, &names, stamped, before);
            }
            return;
        }
    };
    // Its own say on where zsh looks, once it is known: the stamps of a
    // first asking were of what skwir's environment said.
    if said.zdotdir != looked_in {
        stamped = stamps(shell, said.zdotdir.as_deref());
    }
    let kept = Kept {
        shell: shell.to_owned(),
        stamps: stamped,
        values: said
            .values
            .iter()
            .map(|(name, value)| (name.clone(), value.as_encoded_bytes().to_vec()))
            .collect(),
        asked: names.clone(),
        zdotdir: said
            .zdotdir
            .as_deref()
            .map(|dir| dir.as_os_str().as_encoded_bytes().to_vec()),
        failed: false,
    };
    if skwir_core::environment::learn(names, said.values) {
        app.wake();
    }
    if let Err(error) = write(&app, &kept) {
        tracing::debug!("the shell's variables could not be kept: {error:#}");
    }
}

/// The shell did not answer for `names`: remembered with its files as they
/// stand, what it said before kept. ADR-125: asked again once they change.
#[cfg(unix)]
fn remember_failure(
    app: &App,
    shell: &Path,
    names: &BTreeSet<String>,
    stamps: Vec<Stamp>,
    before: Option<Kept>,
) {
    let before = before.unwrap_or_default();
    let kept = Kept {
        shell: shell.to_owned(),
        stamps,
        asked: before.asked.union(names).cloned().collect(),
        values: before.values,
        zdotdir: before.zdotdir,
        failed: true,
    };
    if let Err(error) = write(app, &kept) {
        tracing::debug!("the shell's failure could not be kept: {error:#}");
    }
}

/// Where zsh looks for its files, as far as is known: what the shell said
/// when last asked, else skwir's own environment.
#[cfg(unix)]
fn zdotdir_of(kept: Option<&Kept>) -> Option<PathBuf> {
    kept.and_then(|kept| kept.zdotdir.clone())
        .map(|bytes| PathBuf::from(skwir_core::os_path::from_bytes(bytes)))
        .or_else(|| std::env::var_os("ZDOTDIR").map(PathBuf::from))
        .filter(|dir| dir.is_absolute())
}

#[cfg(unix)]
fn values_of(kept: &Kept) -> HashMap<String, OsString> {
    kept.values
        .iter()
        .map(|(name, bytes)| (name.clone(), skwir_core::os_path::from_bytes(bytes.clone())))
        .collect()
}

/// What a shell said: the values of the names asked, and where zsh looks.
#[cfg(unix)]
struct Said {
    values: HashMap<String, OsString>,
    zdotdir: Option<PathBuf>,
}

/// The name of the variable that says where zsh looks.
#[cfg(unix)]
const ZDOTDIR: &str = "ZDOTDIR";

/// The shell's answer: its environment after its login and interactive
/// files ran, the values of `names` alone kept. In a session of its own: an
/// interactive shell started under a terminal stops itself otherwise.
#[cfg(unix)]
fn ask(
    shell: &Path,
    names: &BTreeSet<String>,
    cancel: &skwir_core::cancel::CancelToken,
) -> anyhow::Result<Said> {
    let marker = format!("__skwir_environment_{}__", std::process::id());
    let kind = shell
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut command = std::process::Command::new(shell);
    command.args(arguments(&kind, &marker));
    command
        .current_dir(skwir_core::os_path::home().unwrap_or_else(|| PathBuf::from("/")))
        // What an `rc` may test to skip what only a terminal needs.
        .env("SKWIR_RESOLVING_ENVIRONMENT", "1")
        .env("TERM", "dumb");
    let finished =
        crate::process::run_capped_without_terminal(&mut command, None, cancel, AT_MOST, 1 << 20)?;
    anyhow::ensure!(
        finished.status.success(),
        "{} ended with {}",
        shell.display(),
        finished.status
    );
    let mut read = names.clone();
    read.insert(ZDOTDIR.to_owned());
    let mut values = parse(&finished.stdout, &marker, &read);
    let zdotdir = if names.contains(ZDOTDIR) {
        values.get(ZDOTDIR).cloned()
    } else {
        values.remove(ZDOTDIR)
    }
    .map(PathBuf::from)
    .filter(|dir| dir.is_absolute());
    Ok(Said { values, zdotdir })
}

/// The shell's arguments: a login and interactive shell where it can be
/// both at once (where `.bashrc`, `.zshrc` and `config.fish` run), then
/// the marker and `env -0`, which every shell runs alike. zsh's
/// `ZDOTDIR` need not be exported, and says where its files were.
#[cfg(unix)]
fn arguments(kind: &str, marker: &str) -> Vec<String> {
    let script = match kind {
        "nu" => format!("print -n '{marker}'; ^env -0"),
        "zsh" => format!("export ZDOTDIR; printf '%s' '{marker}'; env -0"),
        _ => format!("printf '%s' '{marker}'; env -0"),
    };
    match kind {
        "bash" | "zsh" | "fish" | "ksh" | "mksh" | "dash" | "sh" | "yash" | "nu" => {
            vec!["-l".into(), "-i".into(), "-c".into(), script]
        }
        _ => vec!["-c".into(), script],
    }
}

/// The values of `names` in what `env -0` wrote after `marker`; what an
/// `rc` printed before it is not read.
#[cfg(any(unix, test))]
fn parse(output: &[u8], marker: &str, names: &BTreeSet<String>) -> HashMap<String, OsString> {
    let Some(at) = output
        .windows(marker.len())
        .rposition(|window| window == marker.as_bytes())
    else {
        return HashMap::new();
    };
    output[at + marker.len()..]
        .split(|byte| *byte == 0)
        .filter_map(|pair| {
            let equals = pair.iter().position(|byte| *byte == b'=')?;
            let name = std::str::from_utf8(&pair[..equals]).ok()?;
            names.contains(name).then(|| {
                (
                    name.to_owned(),
                    skwir_core::os_path::from_bytes(pair[equals + 1..].to_vec()),
                )
            })
        })
        .collect()
}

#[cfg(unix)]
/// The files a login and interactive shell reads, as they stand now:
/// each of a folder it reads whole (`conf.d`, `profile.d`) too, as a file
/// edited in place moves no folder's time. `zdotdir` is where zsh looks,
/// beside the home folder, where it finds the `.zshenv` that says so.
fn stamps(shell: &Path, zdotdir: Option<&Path>) -> Vec<Stamp> {
    const ZSH: [&str; 4] = [".zshenv", ".zprofile", ".zshrc", ".zlogin"];
    let home = skwir_core::os_path::home().unwrap_or_default();
    let config = skwir_core::xdg::config_home().unwrap_or_else(|| home.join(".config"));
    let mut files: Vec<PathBuf> = [
        ".profile",
        ".bash_profile",
        ".bash_login",
        ".bashrc",
        ".kshrc",
        ".mkshrc",
    ]
    .iter()
    .chain(ZSH.iter())
    .map(|name| home.join(name))
    .collect();
    if let Some(zdotdir) = zdotdir.filter(|dir| *dir != home) {
        files.extend(ZSH.iter().map(|name| zdotdir.join(name)));
    }
    files.extend([
        config.join("fish/config.fish"),
        config.join("fish/conf.d"),
        config.join("nushell/env.nu"),
        config.join("nushell/config.nu"),
        PathBuf::from("/etc/profile"),
        PathBuf::from("/etc/profile.d"),
        PathBuf::from("/etc/bashrc"),
        PathBuf::from("/etc/bash.bashrc"),
        PathBuf::from("/etc/zshenv"),
        PathBuf::from("/etc/zsh/zshenv"),
        PathBuf::from("/etc/zsh/zshrc"),
        PathBuf::from("/etc/zshrc"),
        PathBuf::from("/etc/fish/config.fish"),
        PathBuf::from("/etc/fish/conf.d"),
        shell.to_owned(),
    ]);
    files.iter().flat_map(|path| stamped(path)).collect()
}

/// `path` as it stands, and what it holds where it is a folder, in the
/// order of their names.
#[cfg(unix)]
fn stamped(path: &Path) -> Vec<Stamp> {
    let mut all = vec![stamp_of(path)];
    if std::fs::metadata(path).is_ok_and(|meta| meta.is_dir()) {
        let mut names: Vec<PathBuf> = std::fs::read_dir(path)
            .map(|held| held.flatten().map(|entry| entry.path()).collect())
            .unwrap_or_default();
        names.sort();
        all.extend(names.iter().map(|name| stamp_of(name)));
    }
    all
}

#[cfg(unix)]
fn stamp_of(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok();
    Stamp {
        target: std::fs::canonicalize(path).ok().filter(|t| t != path),
        modified: meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos()),
        length: meta.as_ref().map(std::fs::Metadata::len),
        path: path.to_owned(),
    }
}

#[cfg(unix)]
fn read(app: &App) -> Option<Kept> {
    let path = app.cache.root().join(KEPT);
    let bytes = skwir_core::fs::file::read_regular(&path, 1 << 20).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(unix)]
fn write(app: &App, kept: &Kept) -> anyhow::Result<()> {
    let root = app.cache.root();
    std::fs::create_dir_all(root)?;
    let mut staged = tempfile::NamedTempFile::new_in(root)?;
    serde_json::to_writer(&mut staged, kept)?;
    skwir_core::fs::file::take_place(staged, &root.join(KEPT))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_names_asked_are_read_after_the_marker() {
        let names = BTreeSet::from(["CARGO_HOME".to_owned(), "GNUPGHOME".to_owned()]);
        let mut output = b"welcome to zsh\nCARGO_HOME=/wrong\n__mark__".to_vec();
        output
            .extend_from_slice(b"CARGO_HOME=/data/it's \xc3\xbc\0OPENAI_API_KEY=sk-x\0PATH=/bin\0");
        let said = parse(&output, "__mark__", &names);
        assert_eq!(said.len(), 1);
        assert_eq!(said["CARGO_HOME"], OsString::from("/data/it's ü"));
        assert!(parse(b"no marker", "__mark__", &names).is_empty());
    }

    /// A shell of the test's own (a script standing in, ADR-035) answers as
    /// a login shell would, and a stamp moves once its file changes.
    #[cfg(unix)]
    #[test]
    fn a_shell_is_asked_and_its_files_stamped() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let shell = tmp.path().join("sh");
        std::fs::write(
            &shell,
            "#!/bin/sh\n# the arguments: -l -i -c script\necho noise from an rc\nexport SKWIR_TEST_STORE='/srv/a store'\neval \"$4\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        let names = BTreeSet::from(["SKWIR_TEST_STORE".to_owned()]);
        let said = ask(&shell, &names, &skwir_core::cancel::CancelToken::new()).unwrap();
        assert_eq!(
            said.values["SKWIR_TEST_STORE"],
            OsString::from("/srv/a store")
        );
        assert_eq!(said.values.len(), 1, "nothing but what was asked");
        let before = stamps(&shell, None);
        std::fs::write(&shell, "#!/bin/sh\nexit 1\n").unwrap();
        assert_ne!(stamps(&shell, None), before);
        assert!(ask(&shell, &names, &skwir_core::cancel::CancelToken::new()).is_err());
    }

    #[cfg(unix)]
    fn stand_in(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let shell = dir.join(name);
        std::fs::write(&shell, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        shell
    }

    /// Run under a terminal, an interactive shell stops itself unless it
    /// is in a session of its own, which has none.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_shell_is_a_session_of_its_own() {
        let tmp = tempfile::tempdir().unwrap();
        let shell = stand_in(
            tmp.path(),
            "sh",
            "export SKWIR_TEST_PID=$$\n\
             export SKWIR_TEST_SESSION=$(cut -d' ' -f6 /proc/$$/stat)\n\
             eval \"$4\"",
        );
        let names = BTreeSet::from(["SKWIR_TEST_PID".to_owned(), "SKWIR_TEST_SESSION".to_owned()]);
        let said = ask(&shell, &names, &skwir_core::cancel::CancelToken::new()).unwrap();
        assert!(said.values.contains_key("SKWIR_TEST_PID"), "the script ran");
        assert_eq!(
            said.values["SKWIR_TEST_SESSION"],
            said.values["SKWIR_TEST_PID"]
        );
    }

    /// `.zshenv` says where zsh looks: the shell's word, not skwir's
    /// environment, says which files to stamp, and is not kept among the
    /// values unless asked.
    #[cfg(unix)]
    #[test]
    fn where_zsh_looks_is_the_shells_word() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("it's zsh ü");
        std::fs::create_dir(&dir).unwrap();
        let shell = stand_in(
            tmp.path(),
            "zsh",
            &format!(
                "export ZDOTDIR='{}'\nexport SKWIR_TEST_STORE=/srv/x\neval \"$4\"",
                dir.display().to_string().replace('\'', "'\\''")
            ),
        );
        let names = BTreeSet::from(["SKWIR_TEST_STORE".to_owned()]);
        let said = ask(&shell, &names, &skwir_core::cancel::CancelToken::new()).unwrap();
        assert_eq!(said.zdotdir.as_deref(), Some(dir.as_path()));
        assert!(!said.values.contains_key("ZDOTDIR"), "not asked, not kept");

        let rc = dir.join(".zshrc");
        std::fs::write(&rc, "export A=1\n").unwrap();
        let before = stamps(&shell, Some(&dir));
        assert!(before.iter().any(|stamp| stamp.path == rc));
        std::fs::write(&rc, "export A=22\n").unwrap();
        assert_ne!(stamps(&shell, Some(&dir)), before);
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_of_files_is_stamped_by_each_of_them() {
        let tmp = tempfile::tempdir().unwrap();
        let conf = tmp.path().join("conf.d");
        std::fs::create_dir(&conf).unwrap();
        let file = conf.join("a tool.fish");
        std::fs::write(&file, "set -x A 1\n").unwrap();
        let before = stamped(&conf);
        assert_eq!(before.len(), 2, "the folder and what it holds");
        // In place: the folder's own time does not move.
        std::fs::write(&file, "set -x A 22\n").unwrap();
        assert_eq!(stamped(&conf)[0], before[0]);
        assert_ne!(stamped(&conf), before);
    }

    /// A shell that does not answer is not run again until its files
    /// change, and what was kept answers while they have not.
    #[cfg(unix)]
    #[test]
    fn a_failure_is_remembered_until_the_shells_files_change() {
        use skwir_core::app::AppConfig;
        let tmp = tempfile::tempdir().unwrap();
        let count = tmp.path().join("runs");
        let body = |exit: &str| {
            format!(
                "echo run >> '{}'\n{exit}",
                count.display().to_string().replace('\'', "'\\''")
            )
        };
        let shell = stand_in(tmp.path(), "sh", &body("exit 3"));
        let runs = || std::fs::read_to_string(&count).map_or(0, |runs| runs.lines().count());
        let app = App::new(AppConfig::ephemeral(tmp.path().join("app"))).unwrap();
        let (weak, asking) = (Arc::downgrade(&app), parking_lot::Mutex::new(()));
        let names = BTreeSet::from(["SKWIR_TEST_RC".to_owned()]);
        refresh(&weak, &asking, &shell, names.clone());
        assert_eq!(runs(), 1);
        assert!(read(&app).unwrap().failed);
        refresh(&weak, &asking, &shell, names.clone());
        assert_eq!(runs(), 1, "its files did not change");
        let more: BTreeSet<String> =
            ["SKWIR_TEST_RC".to_owned(), "SKWIR_TEST_OTHER".to_owned()].into();
        refresh(&weak, &asking, &shell, more);
        assert_eq!(runs(), 1, "nor for a name it was not asked");

        // Its files changed: asked again, and an answer is no failure.
        std::fs::write(
            &shell,
            format!(
                "#!/bin/sh\n{}\nexport SKWIR_TEST_RC=/srv/rc\neval \"$4\"\n",
                body(":")
            ),
        )
        .unwrap();
        refresh(&weak, &asking, &shell, names.clone());
        assert_eq!(runs(), 2);
        assert!(!read(&app).unwrap().failed);
        assert_eq!(
            skwir_core::environment::var("SKWIR_TEST_RC"),
            Some(OsString::from("/srv/rc"))
        );

        // Asked again as it stands: what was kept answers, not the shell.
        skwir_core::environment::forget();
        refresh(&weak, &asking, &shell, names);
        assert_eq!(runs(), 2);
        assert_eq!(
            skwir_core::environment::var("SKWIR_TEST_RC"),
            Some(OsString::from("/srv/rc"))
        );
    }
}
