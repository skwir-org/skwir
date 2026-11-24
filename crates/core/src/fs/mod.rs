//! The built-in filesystem provider: streamed listing, lazy stat, watching.

pub mod count;
pub mod dir;
pub mod file;
pub mod hidden;
mod hydrate;
pub mod kept_open;
mod newest;
mod path_provider;
pub mod reach;
pub mod snapshot;
pub mod stat;
pub mod volume;
// Public for what hears that a drive is to be removed (the platform's
// Windows window, ADR-149, 16): the watches on it are let go of first.
pub mod watch;
#[cfg(windows)]
pub(crate) mod win;
pub mod xattr;

pub use dir::Dir;
pub use hydrate::{
    HYDRATE_JOB, Read, hydrate_path, hydrate_paths, hydrate_paths_with, read_paths,
    read_paths_newest,
};
pub use newest::{Newest, Reading, Tickets};
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

/// Count sizes as `units` from now on (the setting, applied): what keeps
/// words already said says its sizes again (`words::revision` moves).
pub fn set_size_units(units: SizeUnits) {
    let binary = units == SizeUnits::Binary;
    if BINARY.load(std::sync::atomic::Ordering::Relaxed) != binary {
        crate::locale::alone_in_process("the size units");
    }
    if BINARY.swap(binary, std::sync::atomic::Ordering::Relaxed) != binary {
        crate::locale::said_moved();
    }
}

/// A size the way the file list shows one, counted as the setting says
/// (`512 B`, `1.5 kB`, `3.2 GB`; `1.5 KiB`): every size skwir says
/// (`inspection::size`, `format::size`) is said by it.
pub fn size_said(bytes: u64) -> String {
    size_words(bytes).say()
}

/// A size said in `units`, whatever the setting.
pub fn size_said_in(bytes: u64, units: SizeUnits) -> String {
    size_words_in(bytes, units).say()
}

/// A size as words, as the setting counts it: each unit a message, the
/// number as the reader's language writes it ("1.5 MB", « 1,5 Mo »).
pub fn size_words(bytes: u64) -> crate::words::Words {
    size_words_in(bytes, size_units())
}

/// A size as words in `units`, whatever the setting.
pub fn size_words_in(bytes: u64, units: SizeUnits) -> crate::words::Words {
    size_words_to(bytes, units, 1)
}

/// A size as words in `units`, its number rounded to `digits` after the
/// point: none for what is sold in whole units, a disk's `4 TB`.
pub fn size_words_to(bytes: u64, units: SizeUnits, digits: u8) -> crate::words::Words {
    use crate::words;
    use crate::words::Decimal;
    if bytes < units.base() {
        return words!(
            fs_size_bytes = "{ $n }\u{a0}B",
            n = i64::try_from(bytes).unwrap_or(i64::MAX),
            note = "A size in bytes, as a file's size is shown (999 B). B is the symbol of a byte."
        );
    }
    let (mut value, base) = (bytes as f64, units.base() as f64);
    let mut power = 0;
    while value >= base && power < 5 {
        value /= base;
        power += 1;
    }
    // Rounded as it is said: what rounds up to a whole next unit is said in
    // it (999,990 B is 1.0 MB, never 1,000.0 kB).
    let mut n = Decimal::of(value, digits);
    if power < 5 && n.value >= Decimal::of(base, digits).value {
        value /= base;
        power += 1;
        n = Decimal::of(value, digits);
    }
    match (units, power) {
        (SizeUnits::Decimal, 1) => words!(
            fs_size_kb = "{ $n }\u{a0}kB",
            n = n,
            note = "A size in kilobytes (1000 bytes): n has one digit after the decimal sign (1.5 kB). Symbols of 1000-based units: kB, MB, GB, TB, PB."
        ),
        (SizeUnits::Decimal, 2) => words!(fs_size_mb = "{ $n }\u{a0}MB", n = n),
        (SizeUnits::Decimal, 3) => words!(fs_size_gb = "{ $n }\u{a0}GB", n = n),
        (SizeUnits::Decimal, 4) => words!(fs_size_tb = "{ $n }\u{a0}TB", n = n),
        (SizeUnits::Decimal, _) => words!(fs_size_pb = "{ $n }\u{a0}PB", n = n),
        (SizeUnits::Binary, 1) => words!(
            fs_size_kib = "{ $n }\u{a0}KiB",
            n = n,
            note = "A size in kibibytes (1024 bytes): n has one digit after the decimal sign (1.5 KiB). Symbols of 1024-based units: KiB, MiB, GiB, TiB, PiB."
        ),
        (SizeUnits::Binary, 2) => words!(fs_size_mib = "{ $n }\u{a0}MiB", n = n),
        (SizeUnits::Binary, 3) => words!(fs_size_gib = "{ $n }\u{a0}GiB", n = n),
        (SizeUnits::Binary, 4) => words!(fs_size_tib = "{ $n }\u{a0}TiB", n = n),
        (SizeUnits::Binary, _) => words!(fs_size_pib = "{ $n }\u{a0}PiB", n = n),
    }
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
