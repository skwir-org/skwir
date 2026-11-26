//! `skwir view` and `skw` (ADR-151): files viewed in a window of the Skwir
//! running, which this launch hands them to and waits on, else in a program
//! of its own started with only what viewing needs. What its command line
//! asks is read here, before anything logs or reaches the display: a file
//! that is not there, or standard input past its bound, is said on standard
//! error, and no window opens.

use std::ffi::{OsStr, OsString};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use skwir_ui::viewer::Viewing;

/// The link the packages install beside the program: started under this
/// name, the program is `skwir view`.
pub const LINK: &str = "skw";

/// The subcommand.
const SUBCOMMAND: &str = "view";

/// The option that keeps a launch apart from the Skwir running, a program
/// of its own (ADR-122), as `skwir` takes it.
const NEW_INSTANCE: &str = "--new-instance";

/// The most read from standard input: room for a film. Past it nothing is
/// shown, as a file cut short would be shown as whole (ADR-151, 7).
const PIPED_MOST: u64 = 1 << 30;

/// How old a folder of what standard input gave is when a later viewer
/// removes it: one a viewer ended otherwise left.
const PIPED_KEPT: Duration = Duration::from_hours(24);

const HELP: &str = "\
skwir view, a file's preview in a window of its own

usage: skwir view [<file>…]     show the files, stepping from one to the next
                                with the arrows; one file steps through its
                                folder; none asks which in the system's dialog
       skwir view -             show what standard input gives
       skw [<file>…]            the same

A Skwir running shows them in a window of its own, and this command waits
until that window closes; --new-instance views them in a program of its
own instead. Escape closes the window, F shows the preview on the whole
screen.";

/// What this launch asks to view, if it views: started as `skw`, or as
/// `skwir view`, `--new-instance` before the subcommand as `skwir`'s
/// options come (`skwir --new-instance view a.png`). Its arguments but the
/// subcommand, that option kept for [`apart`].
pub fn asked(program: &OsStr, args: &[OsString]) -> Option<Vec<OsString>> {
    let name = Path::new(program).file_stem();
    if name == Some(OsStr::new(LINK)) {
        return Some(args.to_vec());
    }
    let at = args.iter().position(|arg| arg != NEW_INSTANCE)?;
    (args[at] == SUBCOMMAND).then(|| args[..at].iter().chain(&args[at + 1..]).cloned().collect())
}

/// `args` without `--new-instance` (before a `--`), and whether it was
/// there: the launch is then a program of its own.
pub fn apart(args: &[OsString]) -> (Vec<OsString>, bool) {
    let mut options = true;
    let mut apart = false;
    let mut kept = Vec::with_capacity(args.len());
    for arg in args {
        if options && arg == "--" {
            options = false;
        } else if options && arg == NEW_INSTANCE {
            apart = true;
            continue;
        }
        kept.push(arg.clone());
    }
    (kept, apart)
}

/// The files `args` name, read against `cwd`, and standard input's file
/// where `-` asks it; else the exit status, what was wrong said on
/// standard error (2 for a wrong usage, 1 for a file not there), or 0 for
/// `--help`. Standard input is read last, once every other argument is
/// known good: nothing it gave stays behind one that is not.
pub fn read(args: &[OsString], cwd: &Path) -> Result<Viewing, i32> {
    let mut viewing = Viewing::default();
    let mut options = true;
    // Where `-` stands among the files.
    let mut stdin_at = None;
    for arg in args {
        if options && arg == "--" {
            options = false;
            continue;
        }
        if options && (arg == "--help" || arg == "-h") {
            println!("{HELP}");
            return Err(0);
        }
        if options && arg == "-" {
            if stdin_at.is_some() {
                eprintln!("skwir view: standard input is named twice");
                return Err(2);
            }
            stdin_at = Some(viewing.files.len());
            continue;
        }
        if options && arg.as_encoded_bytes().starts_with(b"-") {
            eprintln!(
                "skwir view: {} is not an option of this command\n\n{HELP}",
                arg.to_string_lossy()
            );
            return Err(2);
        }
        let path = cwd.join(arg);
        let path = std::path::absolute(&path).unwrap_or(path);
        if std::fs::symlink_metadata(&path).is_err() {
            eprintln!("skwir view: {}: no such file", arg.to_string_lossy());
            return Err(1);
        }
        viewing.files.push(path);
    }
    if let Some(at) = stdin_at {
        let piped = piped()?;
        viewing.files.insert(at, piped.clone());
        viewing.piped = Some(piped);
    }
    Ok(viewing)
}

/// What standard input gives, read into this launch's spool in the cache
/// (`handover::spool_file`: the one place a Skwir it hands it to removes),
/// named by the type its bytes have (`stdin.png`), or `stdin`. Folders of
/// earlier viewers left a day ago go first.
fn piped() -> Result<PathBuf, i32> {
    use skwir_platform::handover::{spool_file, spool_folder, spools};
    let mut input = std::io::stdin().lock();
    if input.is_terminal() {
        eprintln!("skwir view: standard input is a terminal: give a file through a pipe");
        return Err(2);
    }
    sweep(&spools());
    let pid = std::process::id();
    let folder = spool_folder(pid);
    let failed = |error: std::io::Error| {
        eprintln!("skwir view: standard input cannot be kept: {error}");
        1
    };
    skwir_core::folders::private(&folder).map_err(failed)?;
    let read = spool_file(pid, None);
    let written = (|| {
        let mut file = std::fs::File::create(&read)?;
        let copied = std::io::copy(&mut (&mut input).take(PIPED_MOST + 1), &mut file)?;
        file.flush()?;
        Ok::<u64, std::io::Error>(copied)
    })();
    let let_go = || {
        let _ = std::fs::remove_file(&read);
        let _ = std::fs::remove_dir(&folder);
    };
    match written {
        Ok(copied) if copied > PIPED_MOST => {
            let_go();
            eprintln!(
                "skwir view: standard input gives more than {} GiB, which is not shown",
                PIPED_MOST >> 30
            );
            return Err(1);
        }
        Ok(_) => {}
        Err(error) => {
            let_go();
            return Err(failed(error));
        }
    }
    let named = skwir_platform::mime::by_content(read.as_path())
        .and_then(|mime| skwir_platform::mime::extensions(&mime).into_iter().next())
        .map(|extension| spool_file(pid, Some(&extension)));
    Ok(match named {
        Some(named) if named != read && std::fs::rename(&read, &named).is_ok() => named,
        _ => read,
    })
}

/// Remove what standard input gave, and its folder, once this launch's
/// wait ended: its window closed, or the Skwir that showed it ended
/// before (ADR-151, 7).
pub fn let_go_of_piped(viewing: &Viewing) {
    if let Some(piped) = &viewing.piped {
        skwir_platform::handover::let_go_of_spool(piped);
    }
}

/// Remove what earlier viewers kept of standard input and left a day ago
/// or more: each folder is a viewer's, named by its process, and kept
/// while that process runs (a viewer may stay up for days).
fn sweep(all: &Path) {
    let Ok(folders) = std::fs::read_dir(all) else {
        return;
    };
    let now = SystemTime::now();
    for folder in folders.flatten() {
        let old = folder
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= PIPED_KEPT);
        let ended = folder
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
            .is_none_or(|pid| skwir_platform::process::runs(pid) == Some(false));
        if old && ended && folder.file_type().is_ok_and(|kind| kind.is_dir()) {
            let _ = std::fs::remove_dir_all(folder.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{apart, asked, read};
    use std::ffi::OsString;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    /// Started as `skw`, every argument is a file; as `skwir`, only after
    /// `view`, which `--new-instance` may come before.
    #[test]
    fn a_launch_views_as_skw_or_skwir_view() {
        let given = args(&["a picture.png"]);
        assert_eq!(asked("/usr/bin/skw".as_ref(), &given), Some(given.clone()));
        assert_eq!(asked("skwir".as_ref(), &given), None);
        let given = args(&["view", "a picture.png"]);
        assert_eq!(
            asked("/usr/bin/skwir".as_ref(), &given),
            Some(given[1..].to_vec())
        );
        assert_eq!(asked("skwir".as_ref(), &args(&["cmd"])), None);
        let given = args(&["--new-instance", "view", "a picture.png"]);
        let viewed = asked("skwir".as_ref(), &given).unwrap();
        assert_eq!(viewed, args(&["--new-instance", "a picture.png"]));
        assert_eq!(apart(&viewed), (args(&["a picture.png"]), true));
        assert_eq!(asked("skwir".as_ref(), &args(&["--new-instance"])), None);
        assert_eq!(
            asked("skwir".as_ref(), &args(&["--new-instance", "a folder"])),
            None
        );
    }

    /// `-` is read last: an argument that is wrong after it ends the launch
    /// before standard input is read, so nothing it gave stays behind.
    #[test]
    fn standard_input_is_read_once_all_else_is_good() {
        let dir = tempfile::tempdir().unwrap();
        for (given, code) in [
            (&["-", "nowhere.txt"][..], 1),
            (&["-", "--wat"][..], 2),
            (&["-", "-"][..], 2),
            (&["-", "--help"][..], 0),
        ] {
            assert_eq!(
                read(&args(given), dir.path()).err(),
                Some(code),
                "{given:?}"
            );
        }
        assert!(
            !skwir_platform::handover::spool_folder(std::process::id()).exists(),
            "nothing kept"
        );
    }

    /// The sweep takes away a folder a viewer left a day ago once its
    /// process ended, never one whose process runs.
    #[cfg(unix)]
    #[test]
    fn the_sweep_spares_a_viewer_that_runs() {
        let root = tempfile::tempdir().unwrap();
        let own = root.path().join(std::process::id().to_string());
        // No process has this id (above Linux's largest, and macOS's).
        let ended = root.path().join("4194304");
        let old =
            std::time::SystemTime::now() - super::PIPED_KEPT - std::time::Duration::from_secs(60);
        for folder in [&own, &ended] {
            std::fs::create_dir(folder).unwrap();
            std::fs::write(folder.join("stdin"), b"x").unwrap();
            std::fs::File::open(folder)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        super::sweep(root.path());
        assert!(own.join("stdin").exists(), "its viewer runs");
        assert!(!ended.exists());
    }

    /// Files are read against the folder started in; one not there, or an
    /// option not known, opens nothing.
    #[test]
    fn files_are_read_against_the_folder_and_must_be_there() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("it's a note.md"), b"# x").unwrap();
        std::fs::write(dir.path().join("-dash"), b"x").unwrap();
        let viewing = read(&args(&["it's a note.md", "--", "-dash"]), dir.path()).unwrap();
        assert_eq!(
            viewing.files,
            [dir.path().join("it's a note.md"), dir.path().join("-dash")]
        );
        assert!(viewing.piped.is_none());
        assert_eq!(read(&args(&["nowhere.txt"]), dir.path()).err(), Some(1));
        assert_eq!(read(&args(&["--wat"]), dir.path()).err(), Some(2));
        assert_eq!(read(&args(&["--help"]), dir.path()).err(), Some(0));
        assert!(
            read(&[], dir.path()).unwrap().files.is_empty(),
            "the dialog asks"
        );
    }

    /// `--new-instance` keeps the launch apart, a file of that name after
    /// `--` stays a file.
    #[test]
    fn new_instance_keeps_the_launch_apart() {
        assert_eq!(
            apart(&args(&["--new-instance", "a.png"])),
            (args(&["a.png"]), true)
        );
        assert_eq!(
            apart(&args(&["a.png", "--", "--new-instance"])),
            (args(&["a.png", "--", "--new-instance"]), false)
        );
        assert_eq!(apart(&args(&["a.png"])), (args(&["a.png"]), false));
    }
}
