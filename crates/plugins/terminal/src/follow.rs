//! The terminal and the pane shown keep to the same folder, as in Dolphin.
//!
//! Pane to terminal: when the pane goes elsewhere, the plugin types a `cd`
//! for the user, but only while the shell itself has the terminal (its
//! process group is the foreground one): a `cd` typed into an editor would
//! be text in a file. A folder asked for while a program runs is caught up
//! with when the shell has the terminal back.
//!
//! Terminal to pane: after the shell's output, the plugin reads the shell's
//! working directory from the system (`/proc/<pid>/cwd`: no shell
//! integration to install, any shell works). A change the plugin did not
//! type is the user's, and `Screen::moved` advances for the pane to follow.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// How a shell reads a quoted word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// sh, bash, zsh and their kin: `'\''` closes, escapes and reopens.
    Posix,
    /// fish: inside single quotes a backslash escapes `'` and itself.
    Fish,
    /// PowerShell: inside single quotes `''` is a quote; `-LiteralPath`
    /// takes no wildcard.
    PowerShell,
    /// Windows' `cmd`: a name in double quotes, which no Windows name
    /// holds; `%` would be expanded even there, so a path with one is not
    /// typed.
    Cmd,
}

pub fn family(program: &str) -> Option<Family> {
    let name = Path::new(program).file_name()?.to_str()?;
    // A Windows program is named with its extension.
    let name = name
        .strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name);
    match name {
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "mksh" | "yash" | "ash" => Some(Family::Posix),
        "fish" => Some(Family::Fish),
        "pwsh" | "powershell" => Some(Family::PowerShell),
        "cmd" => Some(Family::Cmd),
        _ => None,
    }
}

/// What PowerShell's tokenizer takes as a single quote: `'` and U+2018 to
/// U+201B (‘ ’ ‚ ‛).
const POWERSHELL_QUOTES: [char; 5] = ['\'', '\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}'];

/// `path` as one word `family`'s shell reads back as it is, or `None`
/// for a path a shell line cannot carry: a name that is not UTF-8, a
/// control character (a line editor acts on it before the shell reads the
/// word), a relative path (it could read as an option), or `%` or `"` for
/// `cmd`, which expands the one even in double quotes and has no escape
/// for the other.
pub fn word(family: Family, path: &Path) -> Option<String> {
    let path = path.to_str()?;
    if path.chars().any(char::is_control) {
        return None;
    }
    // Absolute: `/…` on Unix, a drive (`C:\…`) or a share (`\\server\…`)
    // on Windows (PowerShell runs on both).
    let b = path.as_bytes();
    let absolute = path.starts_with('/')
        || path.starts_with(r"\\")
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && &b[1..3] == b":\\");
    if !absolute {
        return None;
    }
    Some(match family {
        Family::Posix => format!("'{}'", path.replace('\'', r"'\''")),
        Family::Fish => format!("'{}'", path.replace('\\', r"\\").replace('\'', r"\'")),
        Family::PowerShell => {
            // PowerShell ends a single-quoted string on any of its single
            // quotes, the typographic ones too (`John’s Music`); each is
            // doubled, which reads back as itself.
            let mut word = String::with_capacity(path.len() + 2);
            word.push('\'');
            for c in path.chars() {
                if POWERSHELL_QUOTES.contains(&c) {
                    word.push(c);
                }
                word.push(c);
            }
            word.push('\'');
            word
        }
        Family::Cmd => {
            if path.contains(['%', '"']) {
                return None;
            }
            format!("\"{path}\"")
        }
    })
}

/// The line that takes `family`'s shell to `dir`. On Unix it starts with
/// a space, which keeps it out of the history of shells that honour that,
/// and with Ctrl+E Ctrl+U, which clears what was typed at the prompt so
/// the `cd` runs alone; on Windows with Escape, which clears the line in
/// `cmd` and in PowerShell's line editor. `None` for a path a shell line
/// cannot carry (`word`).
pub fn cd_line(family: Family, dir: &Path) -> Option<Vec<u8>> {
    let dir = word(family, dir)?;
    let line = match family {
        Family::Posix | Family::Fish => format!("\x05\x15 cd {dir}\r"),
        Family::PowerShell => format!("\x1b Set-Location -LiteralPath {dir}\r"),
        Family::Cmd => format!("\x1b cd /d {dir}\r"),
    };
    Some(line.into_bytes())
}

/// What the plugin typed and what it saw, to tell the user's `cd` from its
/// own.
#[derive(Debug, Default)]
pub struct Follow {
    /// Where the shell was last seen.
    seen: Option<PathBuf>,
    /// The folders the plugin typed a `cd` to and has not seen reached,
    /// oldest first.
    typed: VecDeque<PathBuf>,
    /// A folder the pane went to while a program had the terminal.
    wanted: Option<PathBuf>,
    /// The user's changes of directory, counted.
    pub moved: u64,
}

/// `cd`s in flight at most: more is a pane going back and forth faster
/// than the shell reads, and the oldest no longer matter.
const TYPED: usize = 8;

impl Follow {
    pub fn starting_in(dir: PathBuf) -> Self {
        Self {
            seen: Some(dir),
            ..Self::default()
        }
    }

    pub fn seen(&self) -> Option<&Path> {
        self.seen.as_deref()
    }

    /// The pane shows `dir`. Returns the folder to type a `cd` to now, if
    /// any: the shell must have the terminal (`foreground`) and be
    /// elsewhere.
    pub fn pane_at(&mut self, dir: PathBuf, foreground: bool) -> Option<PathBuf> {
        if self.seen.as_deref() == Some(dir.as_path()) && self.typed.is_empty() {
            self.wanted = None;
            return None;
        }
        if !foreground {
            self.wanted = Some(dir);
            return None;
        }
        self.wanted = None;
        self.type_cd(dir)
    }

    fn type_cd(&mut self, dir: PathBuf) -> Option<PathBuf> {
        if self.typed.back() == Some(&dir) {
            return None;
        }
        if self.typed.len() == TYPED {
            self.typed.pop_front();
        }
        self.typed.push_back(dir.clone());
        Some(dir)
    }

    /// The shell was seen in `cwd`, having the terminal or not. Returns a
    /// folder to type a `cd` to: one the pane went to while a program ran,
    /// once the shell has the terminal back. Each such folder is typed
    /// once, so a `cd` that fails is not retried after every prompt.
    pub fn shell_at(&mut self, cwd: &Path, foreground: bool) -> Option<PathBuf> {
        if let Some(reached) = self.typed.iter().rposition(|dir| dir == cwd) {
            // One of ours, and every one typed before it is behind us, even
            // when the shell went through them between two looks and came
            // back to where it was seen last.
            self.typed.drain(..=reached);
        } else if self.seen.as_deref() != Some(cwd) {
            self.moved += 1;
            // The user went elsewhere: what the pane wanted earlier is not
            // what the user wants now.
            self.wanted = None;
        }
        self.seen = Some(cwd.to_owned());
        if !foreground {
            return None;
        }
        let wanted = self.wanted.take()?;
        (wanted != cwd).then(|| self.type_cd(wanted)).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cd_line_quotes_for_its_shell() {
        let dir = Path::new("/tmp/it's a \\ dir");
        assert_eq!(
            cd_line(Family::Posix, dir).unwrap(),
            b"\x05\x15 cd '/tmp/it'\\''s a \\ dir'\r"
        );
        assert_eq!(
            cd_line(Family::Fish, dir).unwrap(),
            b"\x05\x15 cd '/tmp/it\\'s a \\\\ dir'\r"
        );
        assert!(cd_line(Family::Posix, Path::new("relative")).is_none());
        assert!(cd_line(Family::Posix, Path::new("/a\nrm -rf b")).is_none());
        assert_eq!(
            family("/run/current-system/sw/bin/fish"),
            Some(Family::Fish)
        );
        assert_eq!(family("/usr/bin/nu"), None);
    }

    #[test]
    fn a_path_is_one_word_its_shell_reads_back() {
        let path = Path::new("/tmp/it's a \\ dir");
        assert_eq!(
            word(Family::Posix, path).unwrap(),
            r"'/tmp/it'\''s a \ dir'"
        );
        assert_eq!(word(Family::Fish, path).unwrap(), r"'/tmp/it\'s a \\ dir'");
        assert_eq!(
            word(Family::PowerShell, path).unwrap(),
            "'/tmp/it''s a \\ dir'"
        );
        assert_eq!(
            word(Family::Cmd, Path::new(r"C:\a b")).unwrap(),
            r#""C:\a b""#
        );
        for refused in ["/a\nrm -rf b", "/a\u{1b}[201~b", "relative", "-rf"] {
            assert!(
                word(Family::Posix, Path::new(refused)).is_none(),
                "{refused:?}"
            );
        }
        assert!(word(Family::Cmd, Path::new(r"C:\100%")).is_none());
    }

    #[test]
    fn powershell_reads_its_typographic_quotes_as_quotes() {
        // A name that ends the quoted word on a typographic quote would run
        // what follows it as commands.
        assert_eq!(
            word(
                Family::PowerShell,
                Path::new("/m/x\u{2019}; Remove-Item ~; #")
            )
            .unwrap(),
            "'/m/x\u{2019}\u{2019}; Remove-Item ~; #'"
        );
        assert_eq!(
            word(Family::PowerShell, Path::new("/m/‘a’ ‚b‛ 'c'")).unwrap(),
            "'/m/‘‘a’’ ‚‚b‛‛ ''c'''"
        );
        assert_eq!(
            cd_line(Family::PowerShell, Path::new("/m/Jo’s Music")).unwrap(),
            "\x1b Set-Location -LiteralPath '/m/Jo’’s Music'\r".as_bytes()
        );
    }

    #[test]
    fn a_windows_shell_is_sent_its_own_cd() {
        let dir = Path::new(r"C:\Users\it's a dir");
        assert_eq!(
            cd_line(Family::PowerShell, dir).unwrap(),
            b"\x1b Set-Location -LiteralPath 'C:\\Users\\it''s a dir'\r"
        );
        assert_eq!(
            cd_line(Family::Cmd, dir).unwrap(),
            b"\x1b cd /d \"C:\\Users\\it's a dir\"\r"
        );
        assert!(cd_line(Family::Cmd, Path::new(r"C:\100%")).is_none());
        assert!(cd_line(Family::PowerShell, Path::new(r"relative\x")).is_none());
        assert!(cd_line(Family::PowerShell, Path::new(r"\\server\share")).is_some());
        assert_eq!(family("pwsh.exe"), Some(Family::PowerShell));
        assert_eq!(family("/usr/bin/pwsh"), Some(Family::PowerShell));
        assert_eq!(
            cd_line(Family::PowerShell, Path::new("/home/me")).unwrap(),
            b"\x1b Set-Location -LiteralPath '/home/me'\r"
        );
        assert_eq!(family("cmd.exe"), Some(Family::Cmd));
    }

    #[test]
    fn the_shell_follows_the_pane_and_its_own_cds_are_not_the_users() {
        let mut follow = Follow::starting_in("/home".into());
        assert_eq!(follow.pane_at("/home".into(), true), None, "already there");
        assert_eq!(follow.pane_at("/a".into(), true), Some("/a".into()));
        assert_eq!(follow.pane_at("/b".into(), true), Some("/b".into()));
        // The shell reads both lines in turn: neither is the user's.
        assert_eq!(follow.shell_at(Path::new("/a"), true), None);
        assert_eq!(follow.shell_at(Path::new("/b"), true), None);
        assert_eq!(follow.moved, 0);
        // The user types a cd.
        assert_eq!(follow.shell_at(Path::new("/c"), true), None);
        assert_eq!(follow.moved, 1);
        // The pane follows it there, and nothing is typed back.
        assert_eq!(follow.pane_at("/c".into(), true), None);
        // There and back again before the shell was looked at.
        assert_eq!(follow.pane_at("/d".into(), true), Some("/d".into()));
        assert_eq!(follow.pane_at("/c".into(), true), Some("/c".into()));
        assert_eq!(follow.shell_at(Path::new("/c"), true), None);
        assert_eq!(follow.shell_at(Path::new("/d"), true), None);
        assert_eq!(follow.moved, 2, "/d was left behind: now it is the user's");
    }

    #[test]
    fn a_running_program_is_left_alone_and_caught_up_with_once() {
        let mut follow = Follow::starting_in("/home".into());
        assert_eq!(follow.pane_at("/a".into(), false), None);
        assert_eq!(follow.pane_at("/b".into(), false), None);
        // The program writes; the shell is still in the background.
        assert_eq!(follow.shell_at(Path::new("/home"), false), None);
        // The prompt comes back: go where the pane is now.
        assert_eq!(follow.shell_at(Path::new("/home"), true), Some("/b".into()));
        // The cd failed (no permission): it is not typed again.
        assert_eq!(follow.shell_at(Path::new("/home"), true), None);
        assert_eq!(follow.moved, 0);
    }

    #[test]
    fn the_users_cd_forgets_what_the_pane_wanted() {
        let mut follow = Follow::starting_in("/home".into());
        follow.pane_at("/a".into(), false);
        assert_eq!(follow.shell_at(Path::new("/elsewhere"), true), None);
        assert_eq!(follow.moved, 1);
    }
}
