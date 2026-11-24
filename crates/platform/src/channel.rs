//! How this copy of skwir came (ADR-142): one word its package wrote in
//! `channel`, in the data folder it carries beside the programs
//! (`carried`). Only skwir's own packages write it, each as it is made
//! (`tools/package`). A build from source has none, nor a package
//! manager's (the AUR's `skwir` and `skwir-git`, Nix's, a distribution's),
//! and `skwir-bin` removes the tarball's: what a package manager updates
//! is never told of a new version by skwir. A package manager that
//! installs a release's own bytes is asked of the system instead
//! ([`managed`]): Skwir's repository for a `.deb` or an `.rpm`, Homebrew's
//! cask for the Mac app.

use std::path::Path;

/// The file's name in the data folder.
pub const FILE: &str = "channel";

/// Where Skwir's repository is said to apt, as `skwir-repository`
/// installs it (ADR-142, 3).
pub const APT_SOURCES: &str = "/etc/apt/sources.list.d/skwir.sources";
/// Where Skwir's repository is said to dnf, as `skwir-repository`
/// installs it.
pub const DNF_REPO: &str = "/etc/yum.repos.d/skwir.repo";

/// Where Homebrew keeps what each cask installed, under each prefix it
/// installs in: Apple silicon's, then Intel's (any other prefix Homebrew
/// does not support). A cask's folder there is there while the cask is
/// installed, whatever folder its app was put in (`--appdir`), and goes
/// with `brew uninstall`.
pub const CASKROOMS: [&str; 2] = ["/opt/homebrew/Caskroom", "/usr/local/Caskroom"];
/// Skwir's cask, in skwir-org's tap (`Casks/skwir.rb`, ADR-075).
pub const CASK: &str = "skwir";

/// The longest file read: one word and its line's end.
const LONGEST: u64 = 64;

/// How a copy came: the package it was installed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// The `AppImage` (ADR-052).
    AppImage,
    /// The `AppImage`'s tree as a tarball.
    Tarball,
    /// Windows' installer (ADR-074).
    WindowsInstaller,
    /// Windows' zip.
    WindowsZip,
    /// The Mac app (ADR-075).
    MacOs,
    /// A `.deb` of the release (ADR-040).
    Deb,
    /// An `.rpm` of the release.
    Rpm,
}

impl Channel {
    pub const ALL: [Channel; 7] = [
        Channel::AppImage,
        Channel::Tarball,
        Channel::WindowsInstaller,
        Channel::WindowsZip,
        Channel::MacOs,
        Channel::Deb,
        Channel::Rpm,
    ];

    /// The word a package writes.
    pub fn word(self) -> &'static str {
        match self {
            Channel::AppImage => "appimage",
            Channel::Tarball => "tarball",
            Channel::WindowsInstaller => "windows-installer",
            Channel::WindowsZip => "windows-zip",
            Channel::MacOs => "macos",
            Channel::Deb => "deb",
            Channel::Rpm => "rpm",
        }
    }

    /// The channel `text` names, its line's end and spaces aside; none for
    /// any other word.
    pub fn of_word(text: &str) -> Option<Channel> {
        let word = text.trim();
        Channel::ALL
            .into_iter()
            .find(|channel| channel.word() == word)
    }

    /// Whether the system's package manager updates such a copy once
    /// Skwir's repository is added (`skwir-repository`, ADR-142, 3).
    pub fn has_repository(self) -> bool {
        matches!(self, Channel::Deb | Channel::Rpm)
    }

    /// Whether a package manager may install such a copy, the release's
    /// own bytes, and update it ([`managed`]): a `.deb` or an `.rpm` from
    /// Skwir's repository, the Mac app from Homebrew's cask. Asked again as
    /// the system changes: the repository added or removed, the cask
    /// installed or uninstalled.
    pub fn may_be_managed(self) -> bool {
        self.has_repository() || self == Channel::MacOs
    }
}

/// How the running program came, by the word its package wrote; none for
/// a build not packaged by skwir, or a word not known. Reads the disk:
/// call it off the window's thread.
pub fn of_this_program() -> Option<Channel> {
    of_program_in(&crate::carried::program_folder()?)
}

/// How the program in `bin` came, as `of_this_program` reads it.
pub fn of_program_in(bin: &Path) -> Option<Channel> {
    let path = crate::carried::beside(bin, FILE, Path::is_file)?;
    match crate::file::read_regular_text(&path, LONGEST) {
        Ok(text) => Channel::of_word(&text),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "how skwir came is unread");
            None
        }
    }
}

/// Whether a package manager of the system updates a copy that came by
/// `channel` ([`Channel::may_be_managed`]): for a `.deb` or an `.rpm`, the
/// file `skwir-repository` installs is there (Linux's alone: no other
/// system has one); for the Mac app, Homebrew's cask is installed. Reads
/// the disk: on a worker.
pub fn managed(channel: Channel) -> bool {
    match channel {
        Channel::Deb => Path::new(APT_SOURCES).is_file(),
        Channel::Rpm => Path::new(DNF_REPO).is_file(),
        Channel::MacOs => cask_installed(&CASKROOMS.map(Path::new)),
        _ => false,
    }
}

/// Whether Skwir's cask is installed under one of `caskrooms`: its folder
/// there ([`CASK`]). The copy running need not be the cask's (one
/// downloaded beside it is told nothing either), as a `.deb` installed by
/// hand where the repository is added is updated by the system all the
/// same.
fn cask_installed(caskrooms: &[&Path]) -> bool {
    caskrooms.iter().any(|room| room.join(CASK).is_dir())
}

#[cfg(test)]
mod tests {
    use super::{Channel, cask_installed, of_program_in};

    #[test]
    fn each_word_names_its_channel_and_no_other_word_does() {
        for channel in Channel::ALL {
            assert_eq!(Channel::of_word(channel.word()), Some(channel));
            assert_eq!(
                Channel::of_word(&format!("{}\n", channel.word())),
                Some(channel)
            );
        }
        for word in ["", "aur", "nix", "AppImage", "deb rpm", "source"] {
            assert_eq!(Channel::of_word(word), None, "{word}");
        }
    }

    /// Read where each system's package puts it: above `bin` on Linux,
    /// beside the program on Windows, a Mac's resources; none without the
    /// file, or with a word not known.
    #[test]
    fn a_copy_says_how_it_came_where_its_package_puts_the_word() {
        let root = tempfile::tempdir().unwrap();
        let write = |folder: &std::path::Path, word: &str| {
            std::fs::create_dir_all(folder).unwrap();
            std::fs::write(folder.join("channel"), word).unwrap();
        };
        let linux = root.path().join("linux/usr");
        std::fs::create_dir_all(linux.join("bin")).unwrap();
        assert_eq!(of_program_in(&linux.join("bin")), None, "not packaged");
        write(&linux.join("share/skwir"), "appimage\n");
        assert_eq!(of_program_in(&linux.join("bin")), Some(Channel::AppImage));

        let windows = root.path().join("windows");
        write(&windows.join("share/skwir"), "windows-zip");
        assert_eq!(of_program_in(&windows), Some(Channel::WindowsZip));

        let mac = root.path().join("mac/skwir.app/Contents");
        std::fs::create_dir_all(mac.join("MacOS")).unwrap();
        write(&mac.join("Resources/share/skwir"), "macos");
        assert_eq!(of_program_in(&mac.join("MacOS")), Some(Channel::MacOs));

        let other = root.path().join("other/usr");
        std::fs::create_dir_all(other.join("bin")).unwrap();
        write(&other.join("share/skwir"), "aur");
        assert_eq!(of_program_in(&other.join("bin")), None, "a word not known");
    }

    /// Only a `.deb`, an `.rpm` and the Mac app may be a package manager's
    /// copy; the Mac app is Homebrew's while its cask's folder is in a
    /// Caskroom, any Caskroom of those asked, and no longer once it went.
    #[test]
    fn the_mac_app_is_homebrews_while_its_cask_is_installed() {
        let managed: Vec<Channel> = Channel::ALL
            .into_iter()
            .filter(|channel| channel.may_be_managed())
            .collect();
        assert_eq!(managed, [Channel::MacOs, Channel::Deb, Channel::Rpm]);

        let root = tempfile::tempdir().unwrap();
        let silicon = root.path().join("opt/homebrew/Caskroom");
        let intel = root.path().join("usr/local/Caskroom");
        let rooms = [silicon.as_path(), intel.as_path()];
        assert!(!cask_installed(&rooms), "no Homebrew");
        std::fs::create_dir_all(silicon.join("skwird")).unwrap();
        std::fs::create_dir_all(silicon.join("skwir-nightly")).unwrap();
        assert!(!cask_installed(&rooms), "another cask");
        std::fs::write(silicon.join("skwir"), "").unwrap();
        assert!(!cask_installed(&rooms), "a file of that name");
        std::fs::remove_file(silicon.join("skwir")).unwrap();
        std::fs::create_dir_all(intel.join("skwir/0.7.0")).unwrap();
        assert!(cask_installed(&rooms), "Intel's prefix");
        std::fs::remove_dir_all(intel.join("skwir")).unwrap();
        std::fs::create_dir_all(silicon.join("skwir/0.7.0")).unwrap();
        assert!(cask_installed(&rooms), "Apple silicon's");
        std::fs::remove_dir_all(silicon.join("skwir")).unwrap();
        assert!(!cask_installed(&rooms), "uninstalled");
    }
}
