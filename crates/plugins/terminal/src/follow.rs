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
}

pub fn family(program: &str) -> Option<Family> {
    let name = Path::new(program).file_name()?.to_str()?;
    match name {
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "mksh" | "yash" | "ash" => Some(Family::Posix),
        "fish" => Some(Family::Fish),
        _ => None,
    }
}

/// The line that takes `family`'s shell to `dir`. It starts with a space,
/// which keeps it out of the history of shells that honour that, and with
/// Ctrl+E Ctrl+U, which clears what was typed at the prompt so the `cd`
/// runs alone. `None` for a path a shell line cannot carry.
pub fn cd_line(family: Family, dir: &Path) -> Option<Vec<u8>> {
    let dir = dir.to_str()?;
    // Absolute, so it cannot be read as an option; no control characters,
    // which a line editor would act on before the shell reads the word.
    if !dir.starts_with('/') || dir.chars().any(char::is_control) {
        return None;
    }
    let quoted = match family {
        Family::Posix => format!("'{}'", dir.replace('\'', r"'\''")),
        Family::Fish => format!("'{}'", dir.replace('\\', r"\\").replace('\'', r"\'")),
    };
    Some(format!("\x05\x15 cd {quoted}\r").into_bytes())
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
