//! What the user's login shell exports, for the variables the exclusions'
//! paths start from (ADR-125): a skwir started from the desktop inherits
//! the session's environment, not what `.zshrc` or `config.fish` export
//! (`CARGO_HOME`, `GNUPGHOME`). Core reads the values
//! (`skwir_core::environment`); this asks for them.
//!
//! The shell runs on a worker, never the window's thread, with a timeout
//! and its process group stopped whole (an `rc` that starts tmux); it is
//! asked again only once its files change, their stamps kept with the
//! answer in skwir's cache. Only the names asked for are kept, in memory
//! and in the cache: the rest of the environment holds tokens. Off with
//! `exclusions.shell_variables`. Windows has no login shell: a program
//! inherits the user's variables there.

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
/// shell's files have not changed since. Nothing where the setting is off
/// or the system has no login shell.
pub fn follow(app: &Arc<App>) {
    #[cfg(unix)]
    {
        if !enabled(app) {
            return;
        }
        let Some(shell) = login_shell() else {
            return;
        };
        let kept = read(app).filter(|kept| kept.shell == shell);
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
                    if stamps(&shell) != kept.stamps {
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
/// what it says.
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
    let stamps = stamps(shell);
    let said = match ask(shell, &names, &app.lifetime().child()) {
        Ok(said) => said,
        Err(error) => {
            tracing::debug!("the login shell said no variables: {error:#}");
            return;
        }
    };
    let kept = Kept {
        shell: shell.to_owned(),
        stamps,
        values: said
            .iter()
            .map(|(name, value)| (name.clone(), value.as_encoded_bytes().to_vec()))
            .collect(),
        asked: names.clone(),
    };
    if skwir_core::environment::learn(names, said) {
        app.wake();
    }
    if let Err(error) = write(&app, &kept) {
        tracing::debug!("the shell's variables could not be kept: {error:#}");
    }
}

#[cfg(unix)]
fn values_of(kept: &Kept) -> HashMap<String, OsString> {
    kept.values
        .iter()
        .map(|(name, bytes)| (name.clone(), skwir_core::os_path::from_bytes(bytes.clone())))
        .collect()
}

/// The shell's answer: its environment after its login and interactive
/// files ran, the values of `names` alone kept.
#[cfg(unix)]
fn ask(
    shell: &Path,
    names: &BTreeSet<String>,
    cancel: &skwir_core::cancel::CancelToken,
) -> anyhow::Result<HashMap<String, OsString>> {
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
    let finished = crate::process::run_capped(&mut command, None, cancel, AT_MOST, 1 << 20)?;
    anyhow::ensure!(
        finished.status.success(),
        "{} ended with {}",
        shell.display(),
        finished.status
    );
    Ok(parse(&finished.stdout, &marker, names))
}

/// The shell's arguments: a login and interactive shell where it can be
/// both at once (where `.bashrc`, `.zshrc` and `config.fish` run), then
/// the marker and `env -0`, which every shell runs alike.
#[cfg(unix)]
fn arguments(kind: &str, marker: &str) -> Vec<String> {
    let script = if kind == "nu" {
        format!("print -n '{marker}'; ^env -0")
    } else {
        format!("printf '%s' '{marker}'; env -0")
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
/// The files a login and interactive shell reads, as they stand now.
fn stamps(shell: &Path) -> Vec<Stamp> {
    let home = skwir_core::os_path::home().unwrap_or_default();
    let config = skwir_core::xdg::config_home().unwrap_or_else(|| home.join(".config"));
    let zdotdir = std::env::var_os("ZDOTDIR").map_or_else(|| home.clone(), PathBuf::from);
    let mut files: Vec<PathBuf> = [
        ".profile",
        ".bash_profile",
        ".bash_login",
        ".bashrc",
        ".kshrc",
        ".mkshrc",
    ]
    .iter()
    .map(|name| home.join(name))
    .chain(
        [".zshenv", ".zprofile", ".zshrc", ".zlogin"]
            .iter()
            .map(|name| zdotdir.join(name)),
    )
    .chain([
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
        shell.to_owned(),
    ])
    .collect();
    files.dedup();
    files
        .into_iter()
        .map(|path| {
            let meta = std::fs::metadata(&path).ok();
            Stamp {
                target: std::fs::canonicalize(&path).ok().filter(|t| *t != path),
                modified: meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos()),
                length: meta.as_ref().map(std::fs::Metadata::len),
                path,
            }
        })
        .collect()
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
        assert_eq!(said["SKWIR_TEST_STORE"], OsString::from("/srv/a store"));
        assert_eq!(said.len(), 1, "nothing but what was asked");
        let before = stamps(&shell);
        std::fs::write(&shell, "#!/bin/sh\nexit 1\n").unwrap();
        assert_ne!(stamps(&shell), before);
        assert!(ask(&shell, &names, &skwir_core::cancel::CancelToken::new()).is_err());
    }
}
