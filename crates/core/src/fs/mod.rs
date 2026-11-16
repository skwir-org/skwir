//! The built-in filesystem provider: streamed listing, lazy stat, watching.

pub mod count;
pub mod dir;
pub mod file;
pub mod hidden;
mod hydrate;
pub mod kept_open;
mod path_provider;
pub mod reach;
pub mod stat;
pub mod volume;
pub(crate) mod watch;
#[cfg(windows)]
pub(crate) mod win;
pub mod xattr;

pub use dir::Dir;
pub use hydrate::{HYDRATE_JOB, hydrate_path, hydrate_paths, hydrate_paths_with, read_paths};
pub use path_provider::{PathProvider, wait_let_go};
pub use stat::{Statter, content_path, stat_entry, stat_path};
pub use watch::Settled;

/// The setting of how sizes are counted ([`SizeUnits`]).
pub const SIZE_UNITS_KEY: &str = "core.size_units";

/// How sizes are counted and said (`core.size_units`, ADR-139, 9): a
/// kilobyte of 1000 bytes (kB, MB, GB), as volumes are sold and as macOS's
/// Finder and GNOME's Files say them, or of 1024 (KiB, MiB, GiB), as KDE's
/// Dolphin does. What a search reads does not move with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SizeUnits {
    #[default]
    Decimal,
    Binary,
}

impl SizeUnits {
    /// The setting's value for it.
    pub fn key(self) -> &'static str {
        match self {
            SizeUnits::Decimal => "decimal",
            SizeUnits::Binary => "binary",
        }
    }

    /// The units the setting's value `key` names; Decimal for any other.
    pub fn of_key(key: &str) -> Self {
        if key == "binary" {
            SizeUnits::Binary
        } else {
            SizeUnits::Decimal
        }
    }

    /// The bytes of a kilobyte, and each unit that many of the one before.
    pub fn base(self) -> u64 {
        match self {
            SizeUnits::Decimal => 1000,
            SizeUnits::Binary => 1024,
        }
    }

    /// The units' names, from bytes up.
    pub fn names(self) -> [&'static str; 6] {
        match self {
            SizeUnits::Decimal => ["B", "kB", "MB", "GB", "TB", "PB"],
            SizeUnits::Binary => ["B", "KiB", "MiB", "GiB", "TiB", "PiB"],
        }
    }

    /// How many bytes `n` of the unit `power` above bytes are: `(100, 2)`
    /// is 100 MB, or 100 MiB.
    pub fn bytes(self, n: u64, power: u32) -> u64 {
        n.saturating_mul(self.base().saturating_pow(power))
    }
}

/// Whether sizes are counted in binary multiples: the setting as last
/// applied, read wherever a size is said.
static BINARY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How sizes are counted now.
pub fn size_units() -> SizeUnits {
    if BINARY.load(std::sync::atomic::Ordering::Relaxed) {
        SizeUnits::Binary
    } else {
        SizeUnits::Decimal
    }
}

/// Count sizes as `units` from now on (the setting, applied).
pub fn set_size_units(units: SizeUnits) {
    BINARY.store(
        units == SizeUnits::Binary,
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// A size the way the file list shows one, counted as the setting says
/// (`512 B`, `1.5 kB`, `3.2 GB`; `1.5 KiB`): every size skwir says
/// (`inspection::size`, `format::size`) is said by it.
pub fn size_said(bytes: u64) -> String {
    size_said_in(bytes, size_units())
}

/// A size said in `units`, whatever the setting.
pub fn size_said_in(bytes: u64, units: SizeUnits) -> String {
    let (base, names) = (units.base(), units.names());
    if bytes < base {
        return format!("{bytes} B");
    }
    let (mut value, base) = (bytes as f64, base as f64);
    let mut unit = 0;
    while value >= base && unit < names.len() - 1 {
        value /= base;
        unit += 1;
    }
    format!("{value:.1} {}", names[unit])
}

/// Whether a folder's entry `entry`, read with `std::fs::read_dir`, is
/// hidden: its name starts with a dot, or Windows marks it hidden (which
/// its listing says, free). A Mac's `UF_HIDDEN` would cost a stat per
/// entry here; a listing's `Entry::is_hidden` has it.
pub fn is_hidden(entry: &std::fs::DirEntry) -> bool {
    if entry.file_name().as_encoded_bytes().starts_with(b".") {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        /// `FILE_ATTRIBUTE_HIDDEN`.
        const HIDDEN: u32 = 2;
        entry
            .metadata()
            .is_ok_and(|meta| meta.file_attributes() & HIDDEN != 0)
    }
    #[cfg(not(windows))]
    false
}

/// The folders inside `dir`, in the panes' name order, links to folders
/// included; names starting with a dot only when `hidden`. What a folder's
/// trail menus list (`App::places_in`), and a trashed folder's. Reads the
/// folder: on a worker.
///
/// # Errors
/// When the folder cannot be read.
pub fn subfolders(dir: &std::path::Path, hidden: bool) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(dir)?.flatten() {
        if !hidden && is_hidden(&entry) {
            continue;
        }
        let is_dir = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => entry.path().is_dir(),
            Ok(kind) => kind.is_dir(),
            Err(_) => false,
        };
        if is_dir {
            dirs.push(entry.path());
        }
    }
    dirs.sort_by_cached_key(|dir| crate::collate::sort_key(dir.file_name().unwrap_or_default()));
    Ok(dirs)
}
