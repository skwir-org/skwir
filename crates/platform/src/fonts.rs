//! The system's fonts as skwir asks of them (ADR-073): which families can
//! write a text skwir shows, which of those are monospaced, and the family
//! one of fontconfig's aliases stands for. On Linux fontconfig answers
//! through its programs (`fc-list`, `fc-match`), as the `MuPDF` worker
//! asks it; macOS and Windows say nothing yet, so every family the text
//! system has is offered there, for code too, and a name is taken as it
//! is.

use std::collections::HashSet;

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
