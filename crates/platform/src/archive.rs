//! Small native helper for archive metadata and bounded member reads.
use crate::inspection::{self, limits};
use crate::sandbox::{self, Otherwise};
use bibli_core::CancelToken;
use serde::Deserialize;
use std::path::Path;
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
    variable: "BIBLI_ARCHIVE_WORKER",
    executable: "bibli-archive-worker",
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
pub fn list(path: &Path, cancel: &CancelToken) -> anyhow::Result<Listing> {
    let work = tempfile::tempdir()?;
    let mut run = sandbox::untrusted(&worker()?, path, work.path(), Otherwise::Run)?;
    run.command
        .arg(&run.input)
        .args(stem(path))
        .arg("--members")
        .arg(limits::CONTAINER_MEMBERS.to_string())
        .arg("--names")
        .arg(limits::MEMBER_NAMES.to_string());
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
    let mut run = sandbox::untrusted(&worker()?, path, into, Otherwise::Run)?;
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
