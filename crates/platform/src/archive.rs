//! Small native helper for archive metadata, bounded member reads, and
//! whole archives extracted (ADR-048).
use crate::inspection::{self, limits};
use crate::sandbox::{self, Otherwise};
use anyhow::Context;
use serde::Deserialize;
use skwir_core::CancelToken;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
#[derive(Clone, Debug, Deserialize)]
pub struct Member {
    pub index: u64,
    pub name: String,
    pub size: i64,
    pub directory: bool,
    pub regular: bool,
    pub encrypted: bool,
}
impl Member {
    /// The last component of the member's path: what it is called.
    pub fn file_name(&self) -> &str {
        self.name.rsplit('/').next().unwrap_or(&self.name)
    }
}
/// The helper. Plugins that require it name this, not the literals.
pub const WORKER: crate::process::Worker = crate::process::Worker {
    variable: "SKWIR_ARCHIVE_WORKER",
    executable: "skwir-archive-worker",
};

/// Where the helper is, if it is.
pub fn worker_path() -> Option<std::path::PathBuf> {
    WORKER.find()
}
fn worker() -> anyhow::Result<std::path::PathBuf> {
    WORKER.require("open archives")
}
/// An archive's members as the helper listed them: the first
/// `limits::CONTAINER_MEMBERS`, within `limits::MEMBER_NAMES` bytes of names.
#[derive(Debug, Deserialize)]
pub struct Listing {
    pub members: Vec<Member>,
    /// Every member is listed. A longer archive shows its start, and says so.
    pub complete: bool,
}
/// The listings read last, by the archive's identity, size and time: a
/// member extracted, a page read, the listing shown again take the listing
/// read, where each read the whole archive again (a compressed tar
/// decompressed whole) before writing out one member.
static LISTINGS: parking_lot::Mutex<Vec<(ListingKey, Arc<Listing>)>> =
    parking_lot::const_mutex(Vec::new());
/// Listings kept at most.
const LISTINGS_KEPT: usize = 4;

type ListingKey = (std::path::PathBuf, u64, u64, Option<std::time::SystemTime>);

pub fn list(path: &Path, cancel: &CancelToken) -> anyhow::Result<Arc<Listing>> {
    let key = std::fs::metadata(path).ok().map(|meta| {
        let id = skwir_core::fs::stat::identity(path, &meta);
        (
            path.to_path_buf(),
            id.map_or(0, |id| id.ino),
            meta.len(),
            meta.modified().ok(),
        )
    });
    if let Some(key) = &key
        && let Some((_, listing)) = LISTINGS.lock().iter().find(|(k, _)| k == key)
    {
        return Ok(Arc::clone(listing));
    }
    let listing = Arc::new(read_listing(path, cancel)?);
    if let Some(key) = key {
        let mut kept = LISTINGS.lock();
        kept.retain(|(k, _)| k.0 != key.0);
        if kept.len() >= LISTINGS_KEPT {
            kept.remove(0);
        }
        kept.push((key, Arc::clone(&listing)));
    }
    Ok(listing)
}

fn read_listing(path: &Path, cancel: &CancelToken) -> anyhow::Result<Listing> {
    let worker = worker()?;
    read_listing_with(&worker, path, cancel)
        .map_err(|error| crate::process::explain(&worker, cancel, error))
}
fn read_listing_with(worker: &Path, path: &Path, cancel: &CancelToken) -> anyhow::Result<Listing> {
    let work = tempfile::tempdir()?;
    let mut run = sandbox::untrusted(worker, path, work.path(), Otherwise::Run)?;
    run.command
        .arg(&run.input)
        .args(stem(path))
        .arg("--members")
        .arg(limits::CONTAINER_MEMBERS.to_string())
        .arg("--names")
        .arg(limits::MEMBER_NAMES.to_string())
        // Told what it listed before the run's time is up.
        .arg("--seconds")
        .arg(
            limits::HELPER_TIMEOUT
                .saturating_sub(std::time::Duration::from_secs(3))
                .as_secs()
                .to_string(),
        );
    let bytes = crate::process::run(&mut run.command, cancel, limits::HELPER_TIMEOUT)?;
    Ok(serde_json::from_slice(&bytes)?)
}
/// The file's own stem, which a file only compressed (`notes.txt.gz`)
/// names its one member after: the helper reads it under another name in
/// its sandbox.
fn stem(path: &Path) -> Vec<std::ffi::OsString> {
    path.file_stem()
        .map(|stem| vec!["--stem".into(), stem.to_owned()])
        .unwrap_or_default()
}
/// Where a member named `name` is written inside `into`: a fixed file name,
/// plus an extension reduced to letters and digits so that previews, which
/// tell formats by extension, recognise it. Nothing else of a member's name
/// ever reaches the filesystem.
pub fn member_file(into: &Path, name: &str) -> std::path::PathBuf {
    let extension: String = Path::new(name)
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .filter(|extension| {
            (1..=16).contains(&extension.len())
                && extension.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .unwrap_or_default();
    let mut file = into.join("member");
    if !extension.is_empty() {
        file.set_extension(extension);
    }
    file
}
/// Stream `member`, as listed, to `output`. The helper finds it by index and
/// checks it still carries the listed name, so an archive rewritten since
/// its listing fails instead of showing another member. The declared size
/// is checked here first; the helper bounds the bytes it actually produces.
fn write_member(
    path: &Path,
    member: &Member,
    output: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        u64::try_from(member.size).unwrap_or(0) <= limits::BINARY_INPUT as u64,
        "Member preview exceeds {} limit",
        limits::describe(limits::BINARY_INPUT)
    );
    // It writes in the folder that will hold the member, and nowhere else.
    let (Some(into), Some(name)) = (output.parent(), output.file_name()) else {
        anyhow::bail!("Nowhere to write the member");
    };
    let worker = worker()?;
    let written = || -> anyhow::Result<()> {
        let mut run = sandbox::untrusted(&worker, path, into, Otherwise::Run)?;
        run.command
            .arg(&run.input)
            .args(stem(path))
            .arg(member.index.to_string())
            .arg(run.work.join(name))
            .arg("--name")
            .arg(&member.name)
            .arg("--bytes")
            .arg(limits::BINARY_INPUT.to_string());
        crate::process::run(&mut run.command, cancel, limits::HELPER_TIMEOUT)?;
        Ok(())
    };
    written().map_err(|error| crate::process::explain(&worker, cancel, error))
}
/// Write one member of an archive as a file of its own, named by
/// `member_file` after the last component of its name.
pub fn extract(
    path: &Path,
    member: &Member,
    into: &Path,
    cancel: &CancelToken,
) -> anyhow::Result<std::path::PathBuf> {
    let output = member_file(into, member.file_name());
    write_member(path, member, &output, cancel)?;
    Ok(output)
}
/// One member's bytes, bounded like `extract`.
pub fn read(path: &Path, member: &Member, cancel: &CancelToken) -> anyhow::Result<Vec<u8>> {
    let temp = tempfile::tempdir()?;
    let output = temp.path().join("member");
    write_member(path, member, &output, cancel)?;
    inspection::read_bounded(&output, limits::BINARY_INPUT, "Member preview", cancel)
}
/// Units of work the helper may run ahead of what it last reported while
/// extracting: each an entry, or a MiB of one. A paused extraction stops
/// within that many; the helper reports every few of them.
const AHEAD: u64 = 32;

/// What the helper reports as it extracts: the archive's bytes read so
/// far, and the units of work it took.
#[derive(Deserialize)]
struct Report {
    read: u64,
    units: u64,
}

/// Extract every member of the archive at `path` into `into`, an empty
/// folder, under the paths the archive gives them, with libarchive's own
/// guards (ADR-048): nothing written outside `into` (no `..`, no absolute
/// path, never through a link the archive made), no owner, no set-id
/// bits, modification times kept; pipes, devices and sockets left out.
/// The helper runs in the sandbox where the system has one, writing only
/// in `into`. `read` is told the archive's bytes read as they grow, for a
/// bar against the file's size; while `held` says so the helper is given
/// no more work and stops within `AHEAD` units of it (a paused plan).
/// Cancel stops it at once; what it wrote is the caller's to remove.
///
/// # Errors
/// Why the helper could not extract it all (an encrypted member, a name
/// that would leave `into`, a damaged archive), or that it was cancelled.
pub fn extract_all(
    path: &Path,
    into: &Path,
    cancel: &CancelToken,
    read: impl FnMut(u64),
    held: impl Fn() -> bool,
) -> anyhow::Result<()> {
    let worker = worker()?;
    extract_all_with(&worker, path, into, cancel, read, held)
        .map_err(|error| crate::process::explain(&worker, cancel, error))
}
fn extract_all_with(
    worker: &Path,
    path: &Path,
    into: &Path,
    cancel: &CancelToken,
    mut read: impl FnMut(u64),
    held: impl Fn() -> bool,
) -> anyhow::Result<()> {
    let mut run = sandbox::untrusted(worker, path, into, Otherwise::Run)?;
    run.command
        .arg(&run.input)
        .args(stem(path))
        .arg("--extract");
    // Its work, one ticket a unit, through a pipe never filled: at most
    // `AHEAD` tickets wait in it, far under a pipe's buffer.
    let (tickets_in, mut tickets) = std::io::pipe()?;
    // Its reports go to a file it appends to, read here through a handle
    // of our own as it grows: no pipe that could fill and stop it.
    let reports = tempfile::NamedTempFile::new()?;
    let mut reading = std::fs::File::open(reports.path())?;
    let errors = tempfile::tempfile()?;
    run.command
        .stdin(std::process::Stdio::from(tickets_in))
        .stdout(
            std::fs::OpenOptions::new()
                .append(true)
                .open(reports.path())?,
        )
        .stderr(errors.try_clone()?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        run.command.process_group(0);
    }
    anyhow::ensure!(!cancel.is_cancelled(), "the extraction was cancelled");
    let mut child = crate::process::ChildGuard::new(
        run.command
            .spawn()
            .context("the archive helper could not start")?,
    );
    // The command holds the pipe's other end; the helper has its own.
    drop(run);
    let (mut sent, mut used) = (0u64, 0u64);
    let mut lines = String::new();
    let mut take_reports = |lines: &mut String, used: &mut u64| -> anyhow::Result<()> {
        let mut bytes = Vec::new();
        reading.read_to_end(&mut bytes)?;
        lines.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(end) = lines.find('\n') {
            if let Ok(report) = serde_json::from_str::<Report>(&lines[..end]) {
                read(report.read);
                *used = report.units;
            }
            lines.drain(..=end);
        }
        Ok(())
    };
    loop {
        anyhow::ensure!(!cancel.is_cancelled(), "the extraction was cancelled");
        take_reports(&mut lines, &mut used)?;
        if !held() && sent < used + AHEAD {
            let more = usize::try_from(used + AHEAD - sent)?;
            // A helper that ended has no use for them: its status says why.
            if tickets.write_all(&vec![1u8; more]).is_ok() {
                sent += more as u64;
            }
        }
        if child.exited()? {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let status = child.finish()?;
    take_reports(&mut lines, &mut used)?;
    if !status.success() {
        let mut said = String::new();
        let mut errors = errors;
        std::io::Seek::rewind(&mut errors)?;
        errors.take(4096).read_to_string(&mut said)?;
        let said = said.trim();
        anyhow::bail!(if said.is_empty() {
            format!("the archive helper failed ({status})")
        } else {
            said.to_owned()
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_is_written_under_our_name_never_its_own() {
        let into = Path::new("/scratch/preview");
        let file = |name| member_file(into, name);
        assert_eq!(file("Report final.PDF"), into.join("member.pdf"));
        assert_eq!(file("sub/it's a café.tar.gz"), into.join("member.gz"));
        assert_eq!(file("Makefile"), into.join("member"));
        // Nothing of a hostile name survives: no separators, no dots, no length.
        assert_eq!(file("../../etc/passwd"), into.join("member"));
        assert_eq!(file("x./../y"), into.join("member"));
        assert_eq!(file("a.sh;rm -rf"), into.join("member"));
        assert_eq!(file(&format!("a.{}", "x".repeat(40))), into.join("member"));
    }
}
