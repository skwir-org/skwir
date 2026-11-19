//! What a package carries beside skwir's programs, in its data folder
//! `share/skwir`: next to the program on Windows, above `bin` on Linux, a
//! Mac's `Resources/share` (`AGENTS.md`, "Build, test, lint"). Skwir's
//! own fonts (ADR-118, `fonts::bundled`) and the word saying how the copy
//! came (ADR-142, `channel`). None for a build not packaged.

use std::path::{Path, PathBuf};

/// `name` in the data folder beside the program in `bin`, where `found`
/// says it is there: the first of the places a package puts it.
pub(crate) fn beside(bin: &Path, name: &str, found: fn(&Path) -> bool) -> Option<PathBuf> {
    [
        bin.join("share"),
        bin.join("../share"),
        bin.join("../Resources/share"),
    ]
    .into_iter()
    .map(|share| share.join("skwir").join(name))
    .find(|path| found(path))
}

/// The folder holding the running program, as `beside` takes it.
pub(crate) fn program_folder() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}
