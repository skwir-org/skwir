//! Where skwird is (ADR-050, amended 2026-11-26): the one lookup, which
//! the plugin's start of an engine, its requirement and its note that
//! skwird is missing all ask.
//!
//! On `PATH` first: an installed package (a `.deb`, an `.rpm`, the AUR's,
//! Homebrew's, which `find_program` looks in on macOS) takes over from
//! any copy carried, so that one machine has one engine's index, never
//! two formats rebuilt in turn. Else the copy a package of Skwir's
//! carries beside Skwir's own program: Windows' installer and zip
//! (`skwird.exe`, `skwirdw.exe` beside it), the Mac's app (in
//! `Contents/MacOS`) and the Linux tarball (in its `bin`), found from the
//! folder of the program that runs, its links followed
//! (`skwir_core::folders::program`), as Skwir's helpers are. Only beside
//! it: the packages that keep helpers in `lib/skwir` or `libexec/skwir`
//! (the `.deb`, the `.rpm`, Nix) carry no skwird, but recommend skwird's
//! own package, and the `AppImage` carries none, as its mount ends with
//! Skwir while skwird outlives its last client by a minute.
//!
//! A carried copy is started on demand exactly as one on `PATH` is.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use skwir_core::location::Waker;

/// skwird's name, as `PATH` is searched for it.
const NAME: &str = "skwird";

/// The file a package carries beside Skwir's program.
#[cfg(windows)]
const FILE: &str = "skwird.exe";
#[cfg(not(windows))]
const FILE: &str = "skwird";

/// Where skwird is: on `PATH`, else beside Skwir's own program (the
/// module's header); none where neither holds one that runs. Reads the
/// disk: call off the window's thread.
pub(crate) fn find() -> Option<PathBuf> {
    find_from(skwir_core::folders::program())
}

/// [`find`], Skwir's program being in `bin`.
fn find_from(bin: Option<&Path>) -> Option<PathBuf> {
    skwir_core::os_path::find_program(NAME).or_else(|| beside(bin?))
}

/// The skwird carried beside a program in `bin`, if it runs there.
fn beside(bin: &Path) -> Option<PathBuf> {
    skwir_core::os_path::find_program(bin.join(FILE))
}

/// Whether skwird can answer here: its program found ([`find`]), or the
/// system's engine serving, whose socket a service of skwird's own makes.
/// Reads the disk: call off the window's thread.
pub(crate) fn found() -> bool {
    find().is_some() || skwird_client::system_socket().exists()
}

/// How whether skwird is here is looked at: [`found`], or a test's.
pub(crate) type Look = Arc<dyn Fn() -> bool + Send + Sync>;

/// Whether skwird was missing at the last look ([`found`]): known from
/// memory where the window asks it (`NameIndex::absent`: a walked search's
/// footer, the Index page), looked at again off the window's thread as a
/// window comes to the front (`Plugin::on_looked_at`) and as a search
/// finds no engine, the windows woken once it changes. Not missing until
/// looked at.
pub(crate) struct Presence {
    /// How it is looked at; none where skwird is never said missing: a
    /// test's engine, and a headless test's app, which asks the machine
    /// nothing (`App::index_services`).
    look: Option<Look>,
    missing: AtomicBool,
    /// What wakes the windows once it changes.
    wake: Option<Waker>,
}

impl Presence {
    pub(crate) fn new(look: Option<Look>, wake: Option<Waker>) -> Presence {
        Presence {
            look,
            missing: AtomicBool::new(false),
            wake,
        }
    }

    /// One that never says skwird is missing: a test's index.
    #[cfg(test)]
    pub(crate) fn never() -> Presence {
        Presence::new(None, None)
    }

    /// Look again. Reads the disk: call off the window's thread.
    pub(crate) fn look(&self) {
        if let Some(look) = &self.look {
            self.set(look());
        }
    }

    /// An engine answered: skwird is here, whatever a look would find.
    pub(crate) fn answered(&self) {
        if self.look.is_some() {
            self.set(true);
        }
    }

    fn set(&self, found: bool) {
        let was_missing = self.missing.swap(!found, Ordering::Relaxed);
        if was_missing == found
            && let Some(wake) = &self.wake
        {
            wake();
        }
    }

    /// Whether skwird was missing at the last look: from memory.
    pub(crate) fn missing(&self) -> bool {
        self.missing.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PATH` made of `folders` alone. One test a process under nextest:
    /// set before anything else runs in it.
    #[allow(unsafe_code)]
    fn path_of(folders: &[&Path]) {
        let path = std::env::join_paths(folders).unwrap();
        // SAFETY: one test per process under nextest, set before it starts
        // any thread.
        unsafe { std::env::set_var("PATH", path) };
    }

    /// A file at `path`, executable where the system says so.
    fn program(path: &Path) {
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    /// A program folder holding skwird beside Skwir's program: found there
    /// where `PATH` holds none, the one on `PATH` taken over it where it
    /// does, and none where neither does.
    #[test]
    fn skwird_on_path_is_taken_over_the_copy_beside_skwir() {
        let tmp = tempfile::tempdir().unwrap();
        let (bin, path, empty) = (
            tmp.path().join("skwir bin"),
            tmp.path().join("on path"),
            tmp.path().join("nothing"),
        );
        for folder in [&bin, &path, &empty] {
            std::fs::create_dir(folder).unwrap();
        }
        program(&bin.join(FILE));
        path_of(&[&empty]);
        assert_eq!(find_from(Some(&bin)), Some(bin.join(FILE)), "beside skwir");
        assert_eq!(find_from(None), None, "no program folder known");
        assert_eq!(find_from(Some(&empty)), None, "neither");
        program(&path.join(FILE));
        path_of(&[&empty, &path]);
        assert_eq!(find_from(Some(&bin)), Some(path.join(FILE)), "PATH first");
    }

    /// skwird is said missing only once a look found it so, and no longer
    /// once one finds it or an engine answers, the windows woken at each
    /// change and only then; never where nothing looks.
    #[test]
    fn skwird_is_missing_only_as_last_looked_at() {
        use std::sync::atomic::AtomicUsize;
        let here = Arc::new(AtomicBool::new(false));
        let woken = Arc::new(AtomicUsize::new(0));
        let presence = Presence::new(
            Some({
                let here = Arc::clone(&here);
                Arc::new(move || here.load(Ordering::Relaxed))
            }),
            Some({
                let woken = Arc::clone(&woken);
                Arc::new(move || {
                    woken.fetch_add(1, Ordering::Relaxed);
                })
            }),
        );
        let woken = || woken.load(Ordering::Relaxed);
        assert!(!presence.missing(), "not looked at yet");
        presence.look();
        assert!(presence.missing());
        assert_eq!(woken(), 1);
        presence.look();
        assert_eq!(woken(), 1, "no change, no wake");
        presence.answered();
        assert!(!presence.missing(), "an engine answered");
        assert_eq!(woken(), 2);
        presence.look();
        assert!(presence.missing());
        here.store(true, Ordering::Relaxed);
        presence.look();
        assert!(!presence.missing(), "installed since");
        assert_eq!(woken(), 4);
        let never = Presence::never();
        never.look();
        never.answered();
        assert!(!never.missing());
    }

    /// A file named skwird beside Skwir's program that cannot run there is
    /// no skwird: on Unix, one without its execute bit.
    #[cfg(unix)]
    #[test]
    fn a_skwird_that_cannot_run_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("skwird"), "").unwrap();
        assert_eq!(beside(tmp.path()), None);
    }

    /// On Windows skwird beside Skwir's program is `skwird.exe`: a file
    /// named `skwird` alone is none, nor is a script of that name.
    #[cfg(windows)]
    #[test]
    fn on_windows_the_copy_beside_skwir_is_skwird_exe() {
        let tmp = tempfile::tempdir().unwrap();
        let (exe, other) = (tmp.path().join("exe"), tmp.path().join("other"));
        std::fs::create_dir(&exe).unwrap();
        std::fs::create_dir(&other).unwrap();
        program(&exe.join("skwird.exe"));
        program(&other.join("skwird"));
        program(&other.join("skwird.cmd"));
        assert_eq!(beside(&exe), Some(exe.join("skwird.exe")));
        assert_eq!(beside(&other), None);
    }
}
