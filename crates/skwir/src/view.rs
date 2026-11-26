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
/// `skwir view`. Its arguments after the subcommand.
pub fn asked<'a>(program: &OsStr, args: &'a [OsString]) -> Option<&'a [OsString]> {
    let name = Path::new(program).file_stem();
    if name == Some(OsStr::new(LINK)) {
        return Some(args);
    }
    match args.split_first() {
        Some((first, rest)) if first == SUBCOMMAND => Some(rest),
        _ => None,
    }
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
/// `--help`.
pub fn read(args: &[OsString], cwd: &Path) -> Result<Viewing, i32> {
    let mut viewing = Viewing::default();
    let mut options = true;
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
            if viewing.piped.is_some() {
                eprintln!("skwir view: standard input is named twice");
                return Err(2);
            }
            let piped = piped(&skwir_core::cache::Cache::default_root())?;
            viewing.files.push(piped.clone());
            viewing.piped = Some(piped);
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
    Ok(viewing)
}

/// What standard input gives, read into a file of this program's in the
/// cache, named by the type its bytes have (`stdin.png`), or `stdin`.
/// Folders of earlier viewers left a day ago go first.
fn piped(cache: &Path) -> Result<PathBuf, i32> {
    let mut input = std::io::stdin().lock();
    if input.is_terminal() {
        eprintln!("skwir view: standard input is a terminal: give a file through a pipe");
        return Err(2);
    }
    let all = cache.join("view");
    sweep(&all);
    let folder = all.join(std::process::id().to_string());
    let failed = |error: std::io::Error| {
        eprintln!("skwir view: standard input cannot be kept: {error}");
        1
    };
    skwir_core::folders::private(&folder).map_err(failed)?;
    let read = folder.join("stdin");
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
        .map(|extension| folder.join(format!("stdin.{extension}")));
    Ok(match named {
        Some(named) if std::fs::rename(&read, &named).is_ok() => named,
        _ => read,
    })
}

/// Remove what standard input gave, and its folder: the Skwir that showed
/// it ended before its window closed, which would have.
pub fn let_go_of_piped(viewing: &Viewing) {
    if let Some(piped) = &viewing.piped {
        let _ = std::fs::remove_file(piped);
        if let Some(folder) = piped.parent() {
            let _ = std::fs::remove_dir(folder);
        }
    }
}

/// Remove what earlier viewers kept of standard input and left a day ago
/// or more: each folder is a viewer's, named by its process.
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
        if old && folder.file_type().is_ok_and(|kind| kind.is_dir()) {
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
    /// `view`.
    #[test]
    fn a_launch_views_as_skw_or_skwir_view() {
        let given = args(&["a picture.png"]);
        assert_eq!(asked("/usr/bin/skw".as_ref(), &given), Some(&given[..]));
        assert_eq!(asked("skwir".as_ref(), &given), None);
        let given = args(&["view", "a picture.png"]);
        assert_eq!(asked("/usr/bin/skwir".as_ref(), &given), Some(&given[1..]));
        assert_eq!(asked("skwir".as_ref(), &args(&["cmd"])), None);
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
