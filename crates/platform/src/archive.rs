//! Small native helper for archive metadata, bounded member reads, whole
//! archives extracted (ADR-048), and archives made (ADR-059).
use crate::inspection::{self, limits, limits::Bounds};
use crate::sandbox::{self, Otherwise};
use anyhow::Context;
use serde::Deserialize;
use skwir_core::CancelToken;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Member {
    pub index: u64,
    pub name: String,
    pub size: i64,
    pub directory: bool,
    pub regular: bool,
    pub encrypted: bool,
    /// Its modification time as libarchive reads it, seconds and
    /// nanoseconds since 1970: what extracting it gives the file. A ZIP
    /// made without its extended time says a local time with no zone,
    /// read in this machine's, to two seconds.
    #[serde(default)]
    pub mtime: Option<[i64; 2]>,
    /// What a link says.
    #[serde(default)]
    pub link: Option<String>,
    /// The first name of the file this is another name of (a tar's hard
    /// link).
    #[serde(default)]
    pub hardlink: Option<String>,
}
impl Member {
    /// The last component of the member's path: what it is called.
    pub fn file_name(&self) -> &str {
        self.name.rsplit('/').next().unwrap_or(&self.name)
    }

    /// Its modification time ([`Self::mtime`]), when the archive says one.
    pub fn modified(&self) -> Option<std::time::SystemTime> {
        let [seconds, nanoseconds] = self.mtime?;
        let nanoseconds = u32::try_from(nanoseconds).ok()?;
        let at = std::time::Duration::new(seconds.unsigned_abs(), 0);
        let whole = if seconds >= 0 {
            std::time::UNIX_EPOCH.checked_add(at)?
        } else {
            std::time::UNIX_EPOCH.checked_sub(at)?
        };
        whole.checked_add(std::time::Duration::from_nanos(u64::from(nanoseconds)))
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
/// An archive's members as the helper listed them: the first its bounds'
/// `members`, within their `member_names` bytes of names.
#[derive(Debug, Deserialize)]
pub struct Listing {
    pub members: Vec<Member>,
    /// Every member is listed. A longer archive shows its start, and says so.
    pub complete: bool,
}
/// The listings read last, by the archive's identity, size and time, and
/// the bounds they were read within: a member extracted, a page read, the
/// listing shown again take the listing read, where each read the whole
/// archive again (a compressed tar decompressed whole) before writing out
/// one member.
static LISTINGS: parking_lot::Mutex<Vec<(ListingKey, Bounds, Arc<Listing>)>> =
    parking_lot::const_mutex(Vec::new());
/// Listings kept at most.
const LISTINGS_KEPT: usize = 4;

type ListingKey = (std::path::PathBuf, u64, u64, Option<std::time::SystemTime>);

/// The key `path`'s listing is kept under, as it is now.
fn listing_key(path: &Path) -> Option<ListingKey> {
    std::fs::metadata(path).ok().map(|meta| {
        let id = skwir_core::fs::stat::identity(path, &meta);
        (
            path.to_path_buf(),
            id.map_or(0, |id| id.ino),
            meta.len(),
            meta.modified().ok(),
        )
    })
}

/// The listing of the archive at `path` read last, if it was read as it is
/// now: nothing is read but the file's metadata. What a step says it makes
/// without running the helper (`Operation::makes`).
pub fn listed(path: &Path) -> Option<Arc<Listing>> {
    let key = listing_key(path)?;
    LISTINGS
        .lock()
        .iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, _, listing)| Arc::clone(listing))
}

/// The archive's listing within the quick bounds: what an operation reads
/// (an extraction's names, a compression's check), and a comic's pages.
pub fn list(path: &Path, cancel: &CancelToken) -> anyhow::Result<Arc<Listing>> {
    list_within(path, Bounds::QUICK, cancel)
}

/// The archive's listing within `bounds`: one kept serves when it was read
/// within them, or lists every member.
pub fn list_within(
    path: &Path,
    bounds: Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<Arc<Listing>> {
    let key = listing_key(path);
    if let Some(key) = &key
        && let Some((_, _, listing)) = LISTINGS
            .lock()
            .iter()
            .find(|(k, b, listing)| k == key && (*b == bounds || listing.complete))
    {
        return Ok(Arc::clone(listing));
    }
    let listing = Arc::new(read_listing(path, bounds, cancel)?);
    if let Some(key) = key {
        let mut kept = LISTINGS.lock();
        // Another version of the archive, or the same read again, goes.
        kept.retain(|(k, b, _)| k.0 != key.0 || (*k == key && *b != bounds));
        if kept.len() >= LISTINGS_KEPT {
            kept.remove(0);
        }
        kept.push((key, bounds, Arc::clone(&listing)));
    }
    Ok(listing)
}

/// The member listed at `index`, as a preview showed it: from a listing
/// kept of the archive as it stands, whatever bounds it was read within
/// (Full preview's lists past the quick ones), else from the quick
/// listing, else Full preview's.
pub fn member(path: &Path, index: u64, cancel: &CancelToken) -> anyhow::Result<Member> {
    let held = |listing: &Listing| listing.members.iter().find(|m| m.index == index).cloned();
    if let Some(key) = listing_key(path)
        && let Some(member) = LISTINGS
            .lock()
            .iter()
            .filter(|(k, _, _)| *k == key)
            .find_map(|(_, _, listing)| held(listing))
    {
        return Ok(member);
    }
    for bounds in [Bounds::QUICK, Bounds::WHOLE] {
        let listing = list_within(path, bounds, cancel)?;
        if let Some(member) = held(&listing) {
            return Ok(member);
        }
        if listing.complete {
            break;
        }
    }
    anyhow::bail!("Missing archive member")
}

fn read_listing(path: &Path, bounds: Bounds, cancel: &CancelToken) -> anyhow::Result<Listing> {
    let worker = worker()?;
    read_listing_with(&worker, path, bounds, cancel)
        .map_err(|error| crate::process::explain(&worker, cancel, error))
}
fn read_listing_with(
    worker: &Path,
    path: &Path,
    bounds: Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<Listing> {
    let work = tempfile::tempdir()?;
    let mut run = sandbox::untrusted(worker, path, work.path(), Otherwise::Run)?;
    run.command
        .arg(&run.input)
        .args(stem(path))
        .arg("--members")
        .arg(bounds.members().to_string())
        .arg("--names")
        .arg(bounds.member_names().to_string())
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
/// Stream `member`, as listed, to `output`, within `bounds`' `binary_input`.
/// The helper finds it by index and checks it still carries the listed
/// name, so an archive rewritten since its listing fails instead of showing
/// another member. The declared size is checked here first, and a member
/// declared past the bound is refused by `bounds`; the helper bounds the
/// bytes it actually produces.
fn write_member(
    path: &Path,
    member: &Member,
    output: &Path,
    bounds: Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let most = bounds.binary_input();
    if u64::try_from(member.size).unwrap_or(0) > most as u64 {
        return Err(bounds.refuse(format!(
            "Member preview exceeds {} limit",
            limits::describe(most)
        )));
    }
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
            .arg(most.to_string());
        crate::process::run(&mut run.command, cancel, limits::HELPER_TIMEOUT)?;
        Ok(())
    };
    written().map_err(|error| crate::process::explain(&worker, cancel, error))
}
/// Write one member of an archive as a file of its own, named by
/// `member_file` after the last component of its name, within `bounds`.
pub fn extract(
    path: &Path,
    member: &Member,
    into: &Path,
    bounds: Bounds,
    cancel: &CancelToken,
) -> anyhow::Result<std::path::PathBuf> {
    let output = member_file(into, member.file_name());
    write_member(path, member, &output, bounds, cancel)?;
    Ok(output)
}
/// One member's bytes, within the quick bounds: a comic's page.
pub fn read(path: &Path, member: &Member, cancel: &CancelToken) -> anyhow::Result<Vec<u8>> {
    let temp = tempfile::tempdir()?;
    let output = temp.path().join("member");
    write_member(path, member, &output, Bounds::QUICK, cancel)?;
    inspection::read_bounded(&output, limits::BINARY_INPUT, "Member preview", cancel)
}
/// Units of work the helper may run ahead of what it last reported while
/// extracting: each an entry, or a MiB of one. A paused extraction stops
/// within that many; the helper reports every few of them.
const AHEAD: u64 = 32;

/// What the helper reports as it extracts: the archive's bytes read so
/// far and the units of work it took, or that it needs a passphrase (the
/// attempt, from 1).
#[derive(Deserialize)]
struct Report {
    #[serde(default)]
    read: Option<u64>,
    #[serde(default)]
    units: Option<u64>,
    #[serde(default)]
    passphrase: Option<u32>,
}

/// Bytes as hexadecimal digits, two each: how a name or a passphrase
/// reaches the helper whatever bytes it holds.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Extract every member of the archive at `path` into `into`, an empty
/// folder, under the paths the archive gives them, with libarchive's own
/// guards (ADR-048): nothing written outside `into` (no `..`, no absolute
/// path, never through a link the archive made), no owner, no set-id
/// bits, modification times kept; pipes, devices and sockets left out.
/// The helper runs in the sandbox where the system has one, writing only
/// in `into`. `read` is told the archive's bytes read as they grow, for a
/// bar against the file's size; while `held` says so the helper is given
/// no more work and stops within `AHEAD` units of it (a paused plan). An
/// encrypted member asks `passphrase` (the attempt, from 1: a second
/// says the first was wrong), whose answer reaches the helper through its
/// standard input, never its arguments; `None` gives up, and the
/// extraction fails. Cancel stops it at once; what it wrote is the
/// caller's to remove.
///
/// # Errors
/// Why the helper could not extract it all (an encrypted member without
/// its passphrase, a name that would leave `into`, a damaged archive), or
/// that it was cancelled.
pub fn extract_all(
    path: &Path,
    into: &Path,
    cancel: &CancelToken,
    read: impl FnMut(u64),
    held: impl Fn() -> bool,
    passphrase: impl FnMut(u32) -> Option<String>,
) -> anyhow::Result<()> {
    let worker = worker()?;
    extract_all_with(&worker, path, into, cancel, read, held, passphrase)
        .map_err(|error| crate::process::explain(&worker, cancel, error))?;
    #[cfg(target_os = "macos")]
    quarantine(path, into)?;
    Ok(())
}

/// What the Mac's quarantine attribute names: where a file came from,
/// which Gatekeeper reads before it first opens.
#[cfg(target_os = "macos")]
const QUARANTINE: &str = "com.apple.quarantine";

/// What is extracted from an archive the Mac holds in quarantine (one
/// downloaded) is held there too, as the Mac's own Archive Utility does:
/// Gatekeeper asks before an application or a script from it first runs.
/// `into` was just made by the helper: it is walked without following a
/// link, and each entry, a link's own, takes the archive's attribute.
/// Compiled on a Mac, never run there yet (ADR-035).
#[cfg(target_os = "macos")]
fn quarantine(archive: &Path, into: &Path) -> anyhow::Result<()> {
    use skwir_core::fs::xattr;
    let value = match xattr::get_deref(archive, QUARANTINE) {
        Ok(Some(value)) => value,
        Ok(None) => return Ok(()),
        Err(error) if xattr::refused(&error) => return Ok(()),
        Err(error) => return Err(error).context("the archive's quarantine could not be read"),
    };
    // A file the archive made read-only is opened to its owner for it,
    // and closed again: what is not held in quarantine is not placed.
    let hold = |path: &Path| -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let held = match xattr::set(path, QUARANTINE, &value) {
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                let meta = std::fs::symlink_metadata(path)?;
                if meta.file_type().is_symlink() {
                    return Err(error.into());
                }
                let mode = meta.permissions().mode();
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode | 0o200))?;
                let held = xattr::set(path, QUARANTINE, &value);
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
                held
            }
            other => other,
        };
        held.with_context(|| {
            format!(
                "“{}” could not be held in quarantine",
                path.file_name().unwrap_or_default().to_string_lossy()
            )
        })
    };
    hold(into)?;
    let mut folders = vec![into.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder)? {
            let entry = entry?;
            let path = entry.path();
            hold(&path)?;
            if entry.file_type()?.is_dir() {
                folders.push(path);
            }
        }
    }
    Ok(())
}
fn extract_all_with(
    worker: &Path,
    path: &Path,
    into: &Path,
    cancel: &CancelToken,
    mut read: impl FnMut(u64),
    held: impl Fn() -> bool,
    mut passphrase: impl FnMut(u32) -> Option<String>,
) -> anyhow::Result<()> {
    let mut run = sandbox::untrusted(worker, path, into, Otherwise::Run)?;
    run.command
        .arg(&run.input)
        .args(stem(path))
        .arg("--extract")
        .arg("--passwords");
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
    // A passphrase asked: answered at once, on the line the helper waits
    // for (none to give up), whatever tickets came before it.
    let mut take_reports = |lines: &mut String,
                            used: &mut u64,
                            tickets: &mut std::io::PipeWriter|
     -> anyhow::Result<()> {
        let mut bytes = Vec::new();
        reading.read_to_end(&mut bytes)?;
        lines.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(end) = lines.find('\n') {
            if let Ok(report) = serde_json::from_str::<Report>(&lines[..end]) {
                if let Some(attempt) = report.passphrase {
                    let given = passphrase(attempt).unwrap_or_default();
                    // A helper that ended has no use for it: its status
                    // says why.
                    let _ = tickets.write_all(format!("{}\n", hex(given.as_bytes())).as_bytes());
                }
                if let Some(bytes) = report.read {
                    read(bytes);
                }
                if let Some(units) = report.units {
                    *used = units;
                }
            }
            lines.drain(..=end);
        }
        Ok(())
    };
    loop {
        anyhow::ensure!(!cancel.is_cancelled(), "the extraction was cancelled");
        take_reports(&mut lines, &mut used, &mut tickets)?;
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
    take_reports(&mut lines, &mut used, &mut tickets)?;
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

/// What an archive is made as (ADR-059), told by the end of its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Zip,
    SevenZip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
}

impl Format {
    /// Every format, ZIP first: what is made unasked.
    pub const ALL: [Format; 7] = [
        Format::Zip,
        Format::SevenZip,
        Format::TarGz,
        Format::TarXz,
        Format::TarZst,
        Format::TarBz2,
        Format::Tar,
    ];

    /// Its name, the helper's and a script's (`format`): `zip`, `7z`,
    /// `tar`, `tar.gz`, `tar.bz2`, `tar.xz`, `tar.zst`.
    pub fn name(self) -> &'static str {
        match self {
            Format::Zip => "zip",
            Format::SevenZip => "7z",
            Format::Tar => "tar",
            Format::TarGz => "tar.gz",
            Format::TarBz2 => "tar.bz2",
            Format::TarXz => "tar.xz",
            Format::TarZst => "tar.zst",
        }
    }

    /// The format named `name` ([`Self::name`]), case ignored.
    pub fn named(name: &str) -> Option<Format> {
        let name = name.trim_start_matches('.');
        Format::ALL
            .into_iter()
            .find(|format| format.name().eq_ignore_ascii_case(name))
    }

    /// The extension of a name made for it: `.tar.gz`.
    pub fn extension(self) -> &'static str {
        self.endings()[0]
    }

    /// What a name ends with for it, its own extension first.
    fn endings(self) -> &'static [&'static str] {
        match self {
            Format::Zip => &[".zip"],
            Format::SevenZip => &[".7z"],
            Format::Tar => &[".tar"],
            Format::TarGz => &[".tar.gz", ".tgz"],
            Format::TarBz2 => &[".tar.bz2", ".tbz2", ".tbz"],
            Format::TarXz => &[".tar.xz", ".txz"],
            Format::TarZst => &[".tar.zst", ".tzst"],
        }
    }

    /// The format the file name `name` says, and the length of the name
    /// before its ending: `photos.tar.gz` gives `(TarGz, 6)`; case
    /// ignored. `None` when it ends with no format's, or with nothing
    /// before it.
    pub fn of_name(name: &std::ffi::OsStr) -> Option<(Format, usize)> {
        let bytes = name.as_encoded_bytes();
        Format::ALL.into_iter().find_map(|format| {
            format.endings().iter().find_map(|ending| {
                let at = bytes.len().checked_sub(ending.len()).filter(|&at| at > 0)?;
                bytes[at..]
                    .eq_ignore_ascii_case(ending.as_bytes())
                    .then_some((format, at))
            })
        })
    }

    /// Whether it keeps a file's other names as links to the first (a
    /// tar): elsewhere each name is stored whole.
    pub fn keeps_hard_links(self) -> bool {
        !matches!(self, Format::Zip | Format::SevenZip)
    }

    /// Whether it keeps extended attributes (a tar's pax headers): tags
    /// kept on the file travel in it.
    pub fn keeps_attributes(self) -> bool {
        self.keeps_hard_links()
    }

    /// Whether it can be encrypted here (ZIP's AES-256): libarchive
    /// encrypts no other.
    pub fn encrypts(self) -> bool {
        self == Format::Zip
    }
}

/// A part of an archive being made ([`create`]): its path in the archive
/// (`/` between its names, bytes as the system gave them), what it is, and
/// what is said of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub name: Vec<u8>,
    pub item: Item,
    /// Its permission bits.
    pub mode: u32,
    pub modified: Option<std::time::SystemTime>,
    /// Its extended attributes, kept where the format keeps them
    /// ([`Format::keeps_attributes`]).
    pub attributes: Vec<(Vec<u8>, Vec<u8>)>,
}

/// What a [`Part`] is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Folder,
    /// A file, read at `path` as the archive is written: `size` bytes, as
    /// it was walked.
    File {
        path: std::path::PathBuf,
        size: u64,
        /// The file walked: another put at its name since is not it.
        id: Option<skwir_core::entry::FileId>,
    },
    /// A link that says `target`.
    Link {
        target: Vec<u8>,
    },
    /// Another name of the file written as `first` (a tar keeps hard
    /// links).
    SecondName {
        first: Vec<u8>,
    },
}

impl Part {
    /// The line that tells the helper of it.
    fn header(&self) -> serde_json::Value {
        let (kind, size, target) = match &self.item {
            Item::Folder => ("dir", 0, None),
            Item::File { size, .. } => ("file", *size, None),
            Item::Link { target } => ("link", 0, Some(hex(target))),
            Item::SecondName { first } => ("hard", 0, Some(hex(first))),
        };
        let mtime = self.modified.map(|time| {
            match time.duration_since(std::time::UNIX_EPOCH) {
                Ok(after) => [
                    after.as_secs().cast_signed(),
                    i64::from(after.subsec_nanos()),
                ],
                // Before 1970: a second earlier and what is after it.
                Err(before) => {
                    let before = before.duration();
                    let (mut seconds, mut nanos) = (
                        -before.as_secs().cast_signed(),
                        -i64::from(before.subsec_nanos()),
                    );
                    if nanos < 0 {
                        seconds -= 1;
                        nanos += 1_000_000_000;
                    }
                    [seconds, nanos]
                }
            }
        });
        let attributes: Vec<[String; 2]> = self
            .attributes
            .iter()
            .map(|(name, value)| [hex(name), hex(value)])
            .collect();
        serde_json::json!({
            "name": hex(&self.name),
            "kind": kind,
            "size": size,
            "mode": self.mode & 0o7777,
            "mtime": mtime,
            "target": target,
            "xattrs": attributes,
        })
    }
}

/// Make the archive of `format` written at `output`, a name in a folder
/// nobody else writes in, of `parts` in order (a folder before what it
/// holds). The helper writes there and nowhere else, isolated where the
/// system can (`sandbox::fed`), and opens no file of the user's: they are
/// read here (`file::open_regular`) and handed to it, each the file walked
/// (its identity, size and time), read to its end and no further. `sent`
/// hears the files' bytes handed so far, as they grow; while `held` says
/// so no more is handed (a paused plan). With `passphrase`, a ZIP
/// encrypted by it (AES-256), handed through standard input, never its
/// arguments. Cancel stops it at once; what it wrote is the caller's to
/// remove.
///
/// # Errors
/// Why it could not be made: a file that could not be read, or that
/// changed as it was read (another at its name, longer, shorter, written
/// to), a name the format cannot hold, a full disk; or that it was
/// cancelled.
pub fn create(
    parts: &[Part],
    format: Format,
    passphrase: Option<&str>,
    output: &Path,
    cancel: &CancelToken,
    sent: impl FnMut(u64),
    held: impl Fn() -> bool,
) -> anyhow::Result<()> {
    let worker = worker()?;
    create_with(
        &worker, parts, format, passphrase, output, cancel, sent, held,
    )
    .map_err(|error| crate::process::explain(&worker, cancel, error))
}

#[allow(clippy::too_many_arguments)] // `create`'s, and the helper.
fn create_with(
    worker: &Path,
    parts: &[Part],
    format: Format,
    passphrase: Option<&str>,
    output: &Path,
    cancel: &CancelToken,
    mut sent: impl FnMut(u64),
    held: impl Fn() -> bool,
) -> anyhow::Result<()> {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    let (Some(into), Some(name)) = (output.parent(), output.file_name()) else {
        anyhow::bail!("Nowhere to write the archive");
    };
    anyhow::ensure!(
        passphrase.is_none() || format.encrypts(),
        "Only a ZIP archive can have a password"
    );
    let mut run = sandbox::fed(worker, into, Otherwise::Run)?;
    // A 7z is written whole to a temporary file first: on the disk it is
    // made on, in the folder it is made in, not in a sandbox's `/tmp`,
    // which is memory.
    let scratch = tempfile::Builder::new().prefix(".tmp-").tempdir_in(into)?;
    let scratch_inside = run
        .work
        .join(scratch.path().file_name().unwrap_or_default());
    // libarchive asks `TMPDIR` on Unix and `GetTempPathW` on Windows,
    // which reads `TMP`, then `TEMP`.
    run.command
        .env("TMPDIR", &scratch_inside)
        .env("TMP", &scratch_inside)
        .env("TEMP", &scratch_inside)
        .arg(run.work.join(name))
        .arg("--create")
        .arg(format.name());
    if passphrase.is_some() {
        run.command.arg("--encrypt");
    }
    let (helper_end, our_end) = std::io::pipe()?;
    let errors = tempfile::tempfile()?;
    run.command
        .stdin(std::process::Stdio::from(helper_end))
        .stdout(std::process::Stdio::null())
        .stderr(errors.try_clone()?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        run.command.process_group(0);
    }
    anyhow::ensure!(!cancel.is_cancelled(), "making the archive was cancelled");
    let mut child = crate::process::ChildGuard::new(
        run.command
            .spawn()
            .context("the archive helper could not start")?,
    );
    // The command holds the pipe's other end; the helper has its own.
    drop(run);
    let handed = AtomicU64::new(0);
    let (paused, stop) = (AtomicBool::new(false), AtomicBool::new(false));
    let mut told = 0u64;
    let (ended, handing) = std::thread::scope(|scope| {
        let feeder = scope.spawn(|| hand(our_end, parts, passphrase, &handed, &paused, &stop));
        let ended = loop {
            let now = handed.load(Ordering::Relaxed);
            if now > told {
                sent(now - told);
                told = now;
            }
            paused.store(held(), Ordering::Relaxed);
            if cancel.is_cancelled() {
                break Err(anyhow::anyhow!("making the archive was cancelled"));
            }
            match child.exited() {
                Ok(true) => break Ok(()),
                Ok(false) => {}
                Err(error) => break Err(error.into()),
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        if ended.is_err() {
            // What hands it bytes, or waits to, hears it gone.
            stop.store(true, Ordering::Relaxed);
            child.kill();
        }
        let handing = feeder
            .join()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("handing the files over failed")));
        (ended, handing)
    });
    let now = handed.load(Ordering::Relaxed);
    if now > told {
        sent(now - told);
    }
    ended?;
    let status = child.finish()?;
    // A file that could not be read says so, rather than the helper that
    // saw its input end.
    let piped = |error: &anyhow::Error| {
        error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::BrokenPipe)
    };
    if let Err(error) = &handing
        && !piped(error)
    {
        return handing;
    }
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
    handing
}

/// Hand the helper what it writes (`create`): the passphrase, each part's
/// line and a file's bytes after it, then the end; none while `paused`,
/// and nothing more once `stop`.
fn hand(
    feed: std::io::PipeWriter,
    parts: &[Part],
    passphrase: Option<&str>,
    handed: &std::sync::atomic::AtomicU64,
    paused: &std::sync::atomic::AtomicBool,
    stop: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<()> {
    use std::sync::atomic::Ordering;
    let mut out = std::io::BufWriter::with_capacity(256 * 1024, feed);
    let held = || -> anyhow::Result<()> {
        while paused.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        anyhow::ensure!(!stop.load(Ordering::Relaxed), "stopped");
        Ok(())
    };
    if let Some(passphrase) = passphrase {
        writeln!(out, "{}", hex(passphrase.as_bytes()))?;
    }
    let mut buffer = vec![0u8; 256 * 1024];
    for part in parts {
        held()?;
        writeln!(out, "{}", part.header())?;
        let Item::File { path, size, id } = &part.item else {
            continue;
        };
        let shown = String::from_utf8_lossy(&part.name).into_owned();
        let unread = || format!("“{shown}” could not be read");
        let changed = || anyhow::anyhow!("“{shown}” changed as it was read");
        let mut file = crate::file::open_regular(path).with_context(unread)?;
        // The file walked, as it was walked: not another put at its name
        // since (a link, a file saved anew), nor one written to meanwhile.
        let opened = file.metadata().with_context(unread)?;
        let walked = id.is_none_or(|id| skwir_core::fs::stat::identity(path, &opened) == Some(id))
            && opened.len() == *size
            && part
                .modified
                .is_none_or(|time| opened.modified().ok() == Some(time));
        if !walked {
            return Err(changed());
        }
        let mut left = *size;
        while left > 0 {
            held()?;
            let chunk = usize::try_from(left.min(buffer.len() as u64))?;
            let got = file.read(&mut buffer[..chunk]).with_context(unread)?;
            if got == 0 {
                return Err(changed());
            }
            out.write_all(&buffer[..got])?;
            left -= got as u64;
            handed.fetch_add(got as u64, Ordering::Relaxed);
        }
        // Its size and no more, and written to by nobody while it was read:
        // a file cut short in the archive is never taken for the file.
        let mut more = [0u8; 1];
        let after = file.metadata().with_context(unread)?;
        if file.read(&mut more).with_context(unread)? != 0
            || after.len() != *size
            || after.modified().ok() != opened.modified().ok()
        {
            return Err(changed());
        }
    }
    writeln!(out, "{{\"end\":true}}")?;
    out.flush()?;
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
