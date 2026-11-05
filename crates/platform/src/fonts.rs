//! The system's fonts as skwir asks of them (ADR-073): which families can
//! write a text skwir shows, which of those are monospaced, and the family
//! one of fontconfig's aliases stands for. On Linux fontconfig answers
//! through its programs (`fc-list`, `fc-match`), as the `MuPDF` worker
//! asks it; macOS and Windows say nothing yet, so every family the text
//! system has is offered there, for code too, and a name is taken as it
//! is. Skwir's own fonts, Inter and JetBrains Mono, come with its packages
//! (`CARRIED`, `bundled`, ADR-118).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The largest font file a package carries is read whole up to this.
const FONT_FILE: u64 = 8 << 20;

/// The families skwir's packages carry (ADR-118): Inter for the interface,
/// JetBrains Mono for code. A family's files are named as its release
/// names them (`tools/package/fonts.py`): the family without its spaces,
/// a dash, the style (`Inter-SemiBold.ttf`, `JetBrainsMono-Italic.ttf`).
pub const CARRIED: [&str; 2] = ["Inter", "JetBrains Mono"];

/// The font files of `families` (of `CARRIED`) skwir's packages carry,
/// each with its family, read whole: those in `skwir/fonts` of its data
/// folder beside the program (`share` next to it on Windows, above it on
/// Linux, a Mac's `Resources/share`). None for a build not packaged, nor
/// for a family not asked. Reads the disk: call it off the window's
/// thread (`fonts::read_ahead` in the UI).
pub fn bundled(families: &[&str]) -> Vec<(&'static str, Vec<u8>)> {
    let Some(folder) = std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(Path::parent)
        .and_then(bundled_beside)
    else {
        return Vec::new();
    };
    read_carried(&folder, families)
}

/// The font files of `families` (of `CARRIED`) skwir's packages carry, as
/// `bundled` finds them, by path: for a program that opens them itself
/// (the print helper). None for a build not packaged.
pub fn bundled_files(families: &[&str]) -> Vec<PathBuf> {
    std::env::current_exe()
        .ok()
        .as_deref()
        .and_then(Path::parent)
        .and_then(bundled_beside)
        .map(|folder| carried_in(&folder, families))
        .unwrap_or_default()
        .into_iter()
        .map(|(_, path)| path)
        .collect()
}

/// The font files of `families` in `folder`, as `bundled` reads them.
fn read_carried(folder: &Path, families: &[&str]) -> Vec<(&'static str, Vec<u8>)> {
    carried_in(folder, families)
        .into_iter()
        .filter_map(
            |(family, path)| match crate::file::read_regular(&path, FONT_FILE) {
                Ok(bytes) => Some((family, bytes)),
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "a font skwir carries is unread");
                    None
                }
            },
        )
        .collect()
}

/// The font files of `families` in `folder`, each with its family, in
/// order of their names.
fn carried_in(folder: &Path, families: &[&str]) -> Vec<(&'static str, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<(&'static str, PathBuf)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf")
                })
        })
        .filter_map(|path| {
            let family = family_of(path.file_name()?.to_str()?)?;
            families.contains(&family).then_some((family, path))
        })
        .collect();
    files.sort();
    files
}

/// The family of `CARRIED` a carried file's name says it is of.
fn family_of(name: &str) -> Option<&'static str> {
    CARRIED.into_iter().find(|family| {
        name.strip_prefix(&*family.replace(' ', ""))
            .is_some_and(|rest| rest.starts_with('-'))
    })
}

/// The folder of the fonts a package put beside the program in `bin`.
fn bundled_beside(bin: &Path) -> Option<PathBuf> {
    [
        bin.join("share"),
        bin.join("../share"),
        bin.join("../Resources/share"),
    ]
    .into_iter()
    .map(|share| share.join("skwir").join("fonts"))
    .find(|folder| folder.is_dir())
}

/// Whether `name` is one of fontconfig's generic families, which no text
/// system but fontconfig resolves: GNOME named `Sans 11` and
/// `Monospace 11` before its 48th release.
pub fn is_alias(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "sans" | "sans-serif" | "serif" | "monospace" | "mono" | "system-ui"
    )
}

/// The family `name` stands for: itself, unless it is an alias
/// (`is_alias`), which is the family fontconfig matches it to where it
/// can. It may run `fc-match`: on a worker.
pub fn unalias(name: &str) -> String {
    if !is_alias(name) {
        return name.to_owned();
    }
    imp::matched(name).unwrap_or_else(|| name.to_owned())
}

/// What the system says of its families, each by every name the text
/// system may know it by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Families {
    /// The families with a face that writes Latin: gpui writes in a face
    /// only when it has the letter `m`, the others (Balinese, Arabic,
    /// emoji) being fallbacks for what the one written in lacks.
    pub latin: HashSet<String>,
    /// Of those, the ones whose glyphs are of one width (or two, CJK's).
    pub monospaced: HashSet<String>,
}

/// The families as the system says them, or `None` where it does not:
/// `fc-list` is missing, or macOS and Windows, not read yet
/// (`docs/PORTING.md`). It runs `fc-list`: on a worker.
pub fn families() -> Option<Families> {
    imp::families()
}

/// The families of `fc-list ':charset=6d' --format
/// '%{spacing}\t%{family}\n'`: the faces with an `m`, monospaced when
/// their spacing is fontconfig's mono (100), dual (90, CJK's double width)
/// or charcell (110); a proportional face prints no spacing. A family has
/// its names apart by commas.
// Read only where fontconfig answers; tested on every system.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse(listed: &str) -> Families {
    let mut families = Families::default();
    for (spacing, names) in listed.lines().filter_map(|line| line.split_once('\t')) {
        let mono = matches!(spacing.trim(), "90" | "100" | "110");
        for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            families.latin.insert(name.to_owned());
            if mono {
                families.monospaced.insert(name.to_owned());
            }
        }
    }
    families
}

#[cfg(target_os = "linux")]
mod imp {
    use std::process::Command;
    use std::time::Duration;

    /// What fontconfig lists of every face: a line each, a few hundred
    /// bytes; a system with thousands of faces stays well under this.
    const LISTED: u64 = 4 * 1024 * 1024;

    fn run(command: &mut Command, output: u64) -> Option<String> {
        let finished = crate::process::run_capped(
            command,
            None,
            &skwir_core::CancelToken::new(),
            Duration::from_secs(5),
            output,
        )
        .ok()?;
        finished
            .status
            .success()
            .then(|| String::from_utf8_lossy(&finished.stdout).into_owned())
    }

    pub(super) fn matched(alias: &str) -> Option<String> {
        let family = run(
            Command::new("fc-match").args(["--format", "%{family[0]}", alias]),
            4096,
        )?;
        let family = family.trim();
        (!family.is_empty()).then(|| family.to_owned())
    }

    pub(super) fn families() -> Option<super::Families> {
        let listed = run(
            Command::new("fc-list").args([":charset=6d", "--format", "%{spacing}\t%{family}\n"]),
            LISTED,
        )?;
        Some(super::parse(&listed))
    }
}

// macOS and Windows: their font registries are not asked yet (ADR-073);
// every family is offered for code, and a name is the family it names.
#[cfg(not(target_os = "linux"))]
mod imp {
    pub(super) fn matched(_: &str) -> Option<String> {
        None
    }

    pub(super) fn families() -> Option<super::Families> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_fonts_a_package_carries_are_found_beside_the_program() {
        let root = tempfile::tempdir().unwrap();
        let program = |layout: &str| {
            let at = root.path().join(layout);
            std::fs::create_dir_all(&at).unwrap();
            at
        };
        // Linux's: share above bin.
        let bin = program("linux/usr/bin");
        std::fs::create_dir_all(root.path().join("linux/usr/share/skwir/fonts")).unwrap();
        assert!(super::bundled_beside(&bin).is_some_and(|f| f.ends_with("share/skwir/fonts")));
        // Windows': share beside the program.
        let win = program("windows/skwir");
        std::fs::create_dir_all(win.join("share/skwir/fonts")).unwrap();
        assert!(super::bundled_beside(&win).is_some());
        // A Mac's bundle: Resources/share beside MacOS.
        let mac = program("mac/skwir.app/Contents/MacOS");
        std::fs::create_dir_all(
            root.path()
                .join("mac/skwir.app/Contents/Resources/share/skwir/fonts"),
        )
        .unwrap();
        assert!(super::bundled_beside(&mac).is_some());
        // A build not packaged carries none.
        assert_eq!(super::bundled_beside(&program("build/debug")), None);
    }

    #[test]
    fn only_the_families_asked_are_read_each_file_with_its_family() {
        let folder = tempfile::tempdir().unwrap();
        for name in [
            "Inter-Regular.ttf",
            "Inter-SemiBold.ttf",
            "InterDisplay-Bold.ttf",
            "JetBrainsMono-Italic.ttf",
            "JetBrainsMono-Regular.otf",
            "Other-Regular.ttf",
            "README.txt",
        ] {
            std::fs::write(folder.path().join(name), name).unwrap();
        }
        let read = |families: &[&str]| -> Vec<(&str, String)> {
            super::read_carried(folder.path(), families)
                .into_iter()
                .map(|(family, bytes)| (family, String::from_utf8(bytes).unwrap()))
                .collect()
        };
        assert_eq!(
            read(&["JetBrains Mono"]),
            [
                ("JetBrains Mono", "JetBrainsMono-Italic.ttf".to_owned()),
                ("JetBrains Mono", "JetBrainsMono-Regular.otf".to_owned()),
            ]
        );
        // Inter Display is another family, whatever its name starts with.
        assert_eq!(
            read(&["Inter"]),
            [
                ("Inter", "Inter-Regular.ttf".to_owned()),
                ("Inter", "Inter-SemiBold.ttf".to_owned()),
            ]
        );
        assert_eq!(read(&super::CARRIED).len(), 4);
        assert!(read(&[]).is_empty());
        assert!(super::read_carried(&folder.path().join("none"), &super::CARRIED).is_empty());
    }

    use super::*;

    #[test]
    fn fontconfig_s_generic_names_are_aliases_and_a_family_is_not() {
        for alias in [
            "Sans",
            "sans-serif",
            "Serif",
            "Monospace",
            "mono",
            " system-ui ",
        ] {
            assert!(is_alias(alias), "{alias}");
        }
        for family in [
            "Noto Sans",
            "DejaVu Sans Mono",
            "Hack",
            "Sans Forgetica",
            "",
        ] {
            assert!(!is_alias(family), "{family}");
        }
        // A family is itself, without asking fontconfig.
        assert_eq!(unalias("Noto Sans"), "Noto Sans");
    }

    #[test]
    fn the_monospaced_families_are_those_fontconfig_spaces_by_cells() {
        let listed = "\tDejaVu Sans\n\
                      100\tDejaVu Sans Mono\n\
                      100\tNotoSansM Nerd Font Mono,NotoSansM NFM\n\
                      90\tNoto Sans Mono CJK JP\n\
                      110\tTerminus\n\
                      \tNoto Serif,Noto Serif Display\n\
                      a line without a tab\n";
        let families = parse(listed);
        let sorted = |set: &HashSet<String>| {
            let mut names: Vec<String> = set.iter().cloned().collect();
            names.sort_unstable();
            names
        };
        assert_eq!(
            sorted(&families.monospaced),
            [
                "DejaVu Sans Mono",
                "Noto Sans Mono CJK JP",
                "NotoSansM NFM",
                "NotoSansM Nerd Font Mono",
                "Terminus",
            ]
        );
        assert_eq!(families.latin.len(), 8, "every name listed writes Latin");
        assert!(families.latin.contains("Noto Serif Display"));
    }
}
