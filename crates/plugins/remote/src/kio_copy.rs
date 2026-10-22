//! kioclient, for a batch of copies to or from a server of kio-fuse's
//! (ADR-081): eight `kioclient copy` at once, each its own KIO workers,
//! where through kio-fuse each file is a whole round of requests (made,
//! written, sent up whole at close, renamed into place) on five workers a
//! server. 100 files of 16 KiB against a server 20 ms away, on this
//! machine (2026-10-22, each twice): WebDAV (rclone's) up 11.1–11.3 s
//! through kio-fuse eight at a time, 0.78–1.06 by eight `kioclient`; down
//! 2.3–2.4 sixteen at a time, 0.85–0.87; within the server 11.3, 1.16. SMB
//! (Samba's) up 16.9–17.3, 5.6; down 6.5–6.6, 3.5; within 17.4, 6.2. FTP
//! (rclone's and pyftpdlib's) up 45.6–47.2, 3.1–3.3, every file written
//! through kio-fuse failing (`ways_of`); down 8.6–8.7, 2.6–5.6.
//!
//! `kioclient` takes addresses: a path of kio-fuse's mount is the address
//! its folders name (`webdav/nas:8080/a b` is `webdav://nas:8080/a%20b`).
//! Without `--overwrite` it never replaces a file. It sends a file whole,
//! with no progress of its own: a run says what it copied as it ends. What
//! it wrote beside the mount, kio-fuse does not know of until told to
//! forget what it listed (`forceNodeTimeout`), before what follows reads
//! the folder through it.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use skwir_core::op::transfer::{Aside, Control, Layout, NotServed, Told, Transfer};

/// How many `kioclient` run at once (as `gio`'s eight, which measured).
const AT_ONCE: usize = 8;

/// The most names one `kioclient` is given.
const MOST: usize = 64;

/// What `kioclient` serves of a server kio-fuse holds, by its folder's
/// scheme (module documentation): a batch up and down; a file alone where
/// kio-fuse cannot write it (FTP), elsewhere its step, which goes on from
/// a copy cut short (ADR-080); within one server as measured.
fn ways_of(kind: &str) -> Option<Ways> {
    let batches = Ways {
        down: true,
        alone_up: false,
        within: WITHIN,
    };
    match kind {
        "webdav" | "webdavs" | "dav" | "davs" | "smb" | "cifs" => Some(batches),
        // kio-fuse makes a file empty, then reads it back whole before the
        // first write: KIO's FTP worker fails to read an empty file, so no
        // new file is written through it there. Every copy up is ours.
        "ftp" => Some(Ways {
            alone_up: true,
            within: true,
            ..batches
        }),
        _ => None,
    }
}

/// Copies within one server of WebDAV or SMB: measured faster by
/// `kioclient` (module documentation).
const WITHIN: bool = true;

/// What `kioclient` serves of a kind: a batch up always.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ways {
    /// A batch down onto this machine.
    down: bool,
    /// A file alone up.
    alone_up: bool,
    /// A batch within one server.
    within: bool,
}

/// `kioclient` through this session's KIO.
#[derive(Default)]
pub struct KioCopy {
    /// Variables `kioclient` runs with beside skwir's own (`Kio::env`).
    env: Vec<(OsString, OsString)>,
    /// The runtime folder kio-fuse mounts in, when not the user's.
    runtime: Option<PathBuf>,
    /// The batches it copied.
    made: AtomicUsize,
    /// The batches of moved originals it removed.
    removed: AtomicUsize,
}

impl KioCopy {
    pub fn new(env: Vec<(OsString, OsString)>, runtime: Option<PathBuf>) -> Self {
        KioCopy {
            env,
            runtime,
            made: AtomicUsize::new(0),
            removed: AtomicUsize::new(0),
        }
    }

    /// The batches it copied (a test asks).
    pub fn made(&self) -> usize {
        self.made.load(Ordering::Relaxed)
    }

    /// The batches of moved originals it removed (a test asks).
    pub fn removed(&self) -> usize {
        self.removed.load(Ordering::Relaxed)
    }

    /// The server's folder of this session's kio-fuse holding `path`, its
    /// scheme's folder name, and the parts below it: read from the path,
    /// never from a server.
    fn split<'a>(
        &self,
        path: &'a Path,
    ) -> Option<(PathBuf, &'a str, &'a str, Vec<&'a std::ffi::OsStr>)> {
        let runtime = self.runtime.clone().or_else(skwir_core::xdg::runtime_dir)?;
        let rest = path.strip_prefix(&runtime).ok()?;
        let mut parts = rest.components().map(std::path::Component::as_os_str);
        let root = parts.next()?;
        if !root.to_str()?.starts_with("kio-fuse-") {
            return None;
        }
        let kind = parts.next()?.to_str()?;
        let authority = parts.next()?.to_str()?;
        let mount = runtime.join(root).join(kind).join(authority);
        Some((mount, kind, authority, parts.collect()))
    }

    /// How `kioclient` serves the server holding `path`.
    fn served(&self, path: &Path) -> Option<Ways> {
        let (_, kind, _, _) = self.split(path)?;
        ways_of(kind)
    }

    /// The address of `path`, a path of kio-fuse's mount; else its own
    /// (`file://`), a path of this machine.
    fn url_of(&self, path: &Path) -> String {
        let Some((_, kind, authority, below)) = self.split(path) else {
            return format!("file://{}", escaped(path.as_os_str().as_encoded_bytes()));
        };
        let mut url = format!("{kind}://{authority}");
        for part in below {
            url.push('/');
            url.push_str(&escaped(part.as_encoded_bytes()));
        }
        if url.matches('/').count() < 3 {
            url.push('/');
        }
        url
    }

    /// Whether `kioclient` copies between `from` and `into`: this
    /// machine's own folders and a server of kio-fuse's, or two folders of
    /// one (its worker copies there).
    fn between(
        &self,
        from: &Path,
        into: &Path,
        ways_from: Option<Ways>,
        ways_into: Option<Ways>,
    ) -> bool {
        match (ways_from, ways_into) {
            (Some(_), None) => skwir_core::fs::reach::of(into).is_none(),
            (None, Some(_)) => skwir_core::fs::reach::of(from).is_none(),
            (Some(_), Some(_)) => self
                .split(from)
                .zip(self.split(into))
                .is_some_and(|(a, b)| a.0 == b.0),
            (None, None) => false,
        }
    }

    /// `kioclient` with `words`, its addresses after, through this
    /// session's KIO, drawing nothing.
    fn kioclient(&self, kioclient: &Path, words: &[&str], addresses: &[String]) -> Command {
        let mut command = Command::new(kioclient);
        crate::gvfs::english(&mut command);
        command.env("QT_QPA_PLATFORM", "offscreen");
        command.envs(self.env.iter().map(|(k, v)| (k, v)));
        command.args(words).args(addresses);
        command
    }

    /// Run `kioclient copy` (or `move`) on each of `runs` (files going into
    /// a folder), `at_once` at a time, telling `ran` of each one ended well.
    fn each_at(
        &self,
        word: &str,
        runs: &[(PathBuf, Vec<(PathBuf, u64)>)],
        at_once: usize,
        ran: &(dyn Fn(u64) + Sync),
        control: &Control,
    ) -> anyhow::Result<()> {
        let kioclient =
            program().ok_or_else(|| anyhow::anyhow!("kioclient (KDE) is not installed"))?;
        let runs = runs
            .iter()
            .map(|(into, names)| {
                let mut addresses: Vec<String> =
                    names.iter().map(|(path, _)| self.url_of(path)).collect();
                let mut folder = self.url_of(into);
                if !folder.ends_with('/') {
                    folder.push('/');
                }
                addresses.push(folder);
                crate::runs::Run {
                    command: self.kioclient(kioclient, &[word], &addresses),
                    input: None,
                    bytes: names.iter().map(|(_, bytes)| bytes).sum(),
                }
            })
            .collect();
        crate::runs::at_once("kioclient", runs, at_once, ran, control)
    }

    /// Tell kio-fuse to read its servers again: what `kioclient` wrote
    /// beside it.
    fn forget(&self) {
        #[cfg(not(target_os = "linux"))]
        let _ = &self.env;
        #[cfg(target_os = "linux")]
        if let Err(error) = crate::bus::call::<_, ()>(
            &self.env,
            "org.kde.KIOFuse",
            "/org/kde/KIOFuse",
            "org.kde.KIOFuse.Private",
            "forceNodeTimeout",
            &(),
            std::time::Duration::from_secs(5),
        ) {
            tracing::warn!("kio-fuse was not told to read its servers again: {error:#}");
        }
    }
}

/// `bytes`, an address's path or one of its parts, `%XX`-escaped but for
/// what a path holds as it is.
fn escaped(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len());
    for &b in bytes {
        if b.is_ascii_alphanumeric() || b"-._~/!$&'()*+,;=:@".contains(&b) {
            text.push(char::from(b));
        } else {
            let _ = write!(text, "%{b:02X}");
        }
    }
    text
}

/// `kioclient`, found once: KDE 6's name, else what an older one wrote.
fn program() -> Option<&'static PathBuf> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
            ["kioclient", "kioclient6"]
                .into_iter()
                .find_map(skwir_core::os_path::find_program)
        })
        .as_ref()
}

/// `files` (each with its bytes) grouped by the folder each goes into,
/// in runs of at most [`MOST`], about as many as [`AT_ONCE`] runs hold.
fn runs(files: Vec<(PathBuf, PathBuf, u64)>) -> Vec<(PathBuf, Vec<(PathBuf, u64)>)> {
    let size = files.len().div_ceil(AT_ONCE).clamp(1, MOST);
    let mut by_folder: BTreeMap<PathBuf, Vec<(PathBuf, u64)>> = BTreeMap::new();
    for (src, into, bytes) in files {
        by_folder.entry(into).or_default().push((src, bytes));
    }
    let mut runs = Vec::new();
    for (into, names) in by_folder {
        for run in names.chunks(size) {
            runs.push((into.clone(), run.to_vec()));
        }
    }
    runs
}

impl Transfer for KioCopy {
    fn name(&self) -> &'static str {
        "kioclient"
    }

    /// What it removes, kio-fuse is told to forget at once.
    fn removes(&self, folder: &Path) -> bool {
        program().is_some() && self.served(folder).is_some()
    }

    fn remove(&self, folder: &Path, names: &[OsString], control: &Control) -> anyhow::Result<()> {
        let Some(kioclient) = program().filter(|_| self.served(folder).is_some()) else {
            return Err(NotServed.into());
        };
        let paths: Vec<PathBuf> = names.iter().map(|name| folder.join(name)).collect();
        // KIO removes a folder whole.
        let runs: Vec<crate::runs::Run> = paths
            .chunks(paths.len().div_ceil(AT_ONCE).clamp(1, MOST))
            .map(|chunk| {
                let addresses: Vec<String> = chunk.iter().map(|p| self.url_of(p)).collect();
                crate::runs::Run {
                    command: self.kioclient(kioclient, &["remove"], &addresses),
                    input: None,
                    bytes: 0,
                }
            })
            .collect();
        let outcome = crate::runs::at_once("kioclient", runs, AT_ONCE, &|_| {}, control);
        self.forget();
        outcome?;
        self.removed.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn serves(&self, from: &Path, into: &Path) -> bool {
        program().is_some() && {
            let (ways_from, ways_into) = (self.served(from), self.served(into));
            let takes = match (ways_from, ways_into) {
                (Some(ways), None) => ways.down,
                (None, Some(_)) => true,
                (Some(ways), Some(_)) => ways.within,
                (None, None) => false,
            };
            takes && self.between(from, into, ways_from, ways_into)
        }
    }

    fn serves_alone(&self, from: &Path, into: &Path) -> bool {
        program().is_some() && {
            let (ways_from, ways_into) = (self.served(from), self.served(into));
            let takes = match (ways_from, ways_into) {
                (None, Some(ways)) => ways.alone_up,
                _ => false,
            };
            takes && self.between(from, into, ways_from, ways_into)
        }
    }

    fn show(&self, folder: &Path) -> bool {
        if self.served(folder).is_none() {
            return false;
        }
        self.forget();
        true
    }

    fn copy(
        &self,
        from: &Path,
        names: &[OsString],
        into: &Path,
        aside: &Aside<'_>,
        told: &Told<'_>,
        control: &Control,
    ) -> anyhow::Result<()> {
        let (copied, placed, temp) = (told.copied, told.placed, aside.temp);
        let layout = match temp {
            Some(_) => Layout::files(from, names, into),
            None => Layout::made(from, names, into)?,
        };
        // Beside what is there, each is copied into `temp` first.
        let files: Vec<(PathBuf, PathBuf, u64)> = layout
            .files
            .iter()
            .map(|(src, dst)| {
                let folder = temp.map_or_else(
                    || dst.parent().unwrap_or(into).to_path_buf(),
                    Path::to_path_buf,
                );
                let bytes = std::fs::symlink_metadata(src).map_or(0, |m| m.len());
                (src.clone(), folder, bytes)
            })
            .collect();
        let total = AtomicU64::new(0);
        let ran = |bytes: u64| copied(total.fetch_add(bytes, Ordering::Relaxed) + bytes);
        let outcome = self.each_at("copy", &runs(files.clone()), AT_ONCE, &ran, control);
        // What it wrote beside the mount, kio-fuse reads anew.
        self.forget();
        if let Err(error) = outcome {
            if control.is_cancelled() || !error.is::<crate::runs::Failed>() {
                return Err(error);
            }
            // A run stopped at a file its server refused: what is missing,
            // a `kioclient` a folder, one at a time.
            let missing: Vec<(PathBuf, PathBuf, u64)> = files
                .into_iter()
                .filter(|(src, folder, _)| {
                    src.file_name()
                        .is_none_or(|name| std::fs::symlink_metadata(folder.join(name)).is_err())
                })
                .collect();
            tracing::debug!(
                missing = missing.len(),
                "kioclient copies again what was refused: {error:#}"
            );
            let mut by_folder: BTreeMap<PathBuf, Vec<(PathBuf, u64)>> = BTreeMap::new();
            for (src, folder, bytes) in missing {
                by_folder.entry(folder).or_default().push((src, bytes));
            }
            let again: Vec<(PathBuf, Vec<(PathBuf, u64)>)> = by_folder.into_iter().collect();
            let outcome = self.each_at("copy", &again, 1, &ran, control);
            self.forget();
            outcome?;
        }
        layout.finish()?;
        if let Some(temp) = temp {
            let made: Vec<(PathBuf, PathBuf)> = layout
                .files
                .iter()
                .filter_map(|(_, dst)| Some((temp.join(dst.file_name()?), dst.clone())))
                .collect();
            if self.served(into).is_some() {
                // Moved on the server, never over what is there: a name
                // taken meanwhile is left to its step.
                let moves = made
                    .iter()
                    .map(|(made, _)| (made.clone(), into.to_path_buf(), 0))
                    .collect();
                let outcome = self.each_at("move", &runs(moves), AT_ONCE, &|_| {}, control);
                self.forget();
                let left: std::collections::HashSet<OsString> = std::fs::read_dir(temp)?
                    .flatten()
                    .map(|e| e.file_name())
                    .collect();
                for (made, _) in &made {
                    if let Some(name) = made.file_name().filter(|n| !left.contains(*n)) {
                        placed(name);
                    }
                }
                outcome?;
            } else {
                for (made, dst) in made {
                    match skwir_core::op::transfer::place(&made, &dst) {
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Ok(()) => placed(dst.file_name().unwrap_or_default()),
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }
        self.made.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kio() -> KioCopy {
        KioCopy::new(Vec::new(), Some(PathBuf::from("/run/user/1000")))
    }

    #[test]
    fn a_path_of_kio_fuse_is_the_address_its_folders_name() {
        let kio = kio();
        assert_eq!(
            kio.url_of(Path::new(
                "/run/user/1000/kio-fuse-AbCdEf/webdav/nas:8080/a b/it's 100%.txt"
            )),
            "webdav://nas:8080/a%20b/it's%20100%25.txt"
        );
        assert_eq!(
            kio.url_of(Path::new("/run/user/1000/kio-fuse-AbCdEf/smb/WORK;me@nas")),
            "smb://WORK;me@nas/"
        );
        assert_eq!(
            kio.url_of(Path::new("/home/me/a b.txt")),
            "file:///home/me/a%20b.txt"
        );
    }

    #[test]
    fn kioclient_takes_what_it_was_measured_faster_for() {
        let kio = kio();
        let dav = Path::new("/run/user/1000/kio-fuse-AbCdEf/webdav/nas/up");
        let smb = Path::new("/run/user/1000/kio-fuse-AbCdEf/smb/nas/share");
        let sftp = Path::new("/run/user/1000/kio-fuse-AbCdEf/sftp/me@box/x");
        assert!(kio.served(dav).is_some() && kio.served(smb).is_some());
        assert_eq!(kio.served(sftp), None, "sshfs's, not measured here");
        assert_eq!(
            kio.served(Path::new("/run/user/1001/kio-fuse-AbCdEf/webdav/nas/up")),
            None,
            "another session's"
        );
        let files: Vec<(PathBuf, PathBuf, u64)> = (0..100)
            .map(|i| (PathBuf::from(format!("/a/{i}")), dav.to_path_buf(), 1))
            .collect();
        let runs = runs(files);
        assert_eq!(runs.len(), 8);
        assert!(runs.iter().all(|(_, names)| names.len() <= 13));
    }
}
