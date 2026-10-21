//! What the window and the plugins ask of remote folders (ADR-077): what
//! a plugin knows of each (`Registry::remotes`) beside what the mount
//! table says (`fs::reach`). A remote folder is a folder; these say only
//! what differs there.

use std::path::Path;
use std::sync::Arc;

use crate::app::App;
use crate::fs::reach::{self, Known, Remote};
use crate::location::Location;

/// How often the window coming back reads remote folders again.
pub const REFRESHED_WITHIN: std::time::Duration = std::time::Duration::from_secs(10);

impl App {
    /// The remote holding `path`, and what a plugin knows of it.
    pub fn remote(&self, path: &Path) -> Option<(Arc<Remote>, Option<Known>)> {
        let remote = reach::of(path)?;
        let known = self
            .registry
            .remotes
            .find(|known| known.point == remote.point);
        Some((remote, known))
    }

    /// Whether a thumbnail of the file at `path`, of `size` bytes if
    /// known, may be made unasked: anywhere on this machine; on a remote,
    /// as its rule says (none where no plugin knows it).
    pub fn shows_thumbnail(&self, path: &Path, size: Option<u64>) -> bool {
        match self.remote(path) {
            None => true,
            Some((_, known)) => known
                .map_or_else(reach::Thumbnails::default, |known| known.thumbnails)
                .shows(size),
        }
    }

    /// `location` as the address it is reached by, when it is a folder of
    /// a remote whose address is known (`sftp://me@host/home/me/docs`):
    /// what a tab keeps of it, so a later start connects again. `None`
    /// for anything else.
    pub fn address_of(&self, location: &Location) -> Option<Location> {
        let path = location.path()?;
        let (remote, known) = self.remote(&path)?;
        let address = known?.address?;
        let below = path.strip_prefix(&remote.point).ok()?;
        Some(address_below(&address, below))
    }

    /// Read again each of `folders` a pane shows or keeps beside what it
    /// shows: a plan's transfer changed them beside the mount showing them
    /// (`Executor::set_changed_beside`), and no watch of this machine's saw
    /// it.
    pub fn refresh_showing(&self, folders: &[std::path::PathBuf]) {
        let open: Vec<_> = {
            let ws = self.workspace.read();
            ws.panes()
                .iter()
                .filter(|p| !p.is_dormant())
                .flat_map(|p| p.open_in(folders))
                .collect()
        };
        for open in open {
            open.refresh();
        }
    }

    /// Read again each folder shown that is on a remote: a server does not
    /// say what changed on it, so the window does when it comes back. At
    /// most once in [`REFRESHED_WITHIN`]: a window gone back and forth
    /// does not list a large folder across the network each time.
    pub fn refresh_remote(&self) {
        static LAST: parking_lot::Mutex<Option<std::time::Instant>> = parking_lot::Mutex::new(None);
        {
            let mut last = LAST.lock();
            if last.is_some_and(|at| at.elapsed() < REFRESHED_WITHIN) {
                return;
            }
            *last = Some(std::time::Instant::now());
        }
        let open: Vec<_> = {
            let ws = self.workspace.read();
            ws.panes()
                .iter()
                .filter(|p| !p.is_dormant())
                .filter_map(|p| p.open.clone())
                .filter(|open| {
                    open.location()
                        .path()
                        .is_some_and(|path| reach::of(&path).is_some())
                })
                .collect()
        };
        for open in open {
            open.refresh();
        }
    }
}

/// The address of what is `below` the folder `address` is of.
pub fn address_below(address: &Location, below: &Path) -> Location {
    let mut rest = address.rest.trim_end_matches('/').to_owned();
    for part in below.components() {
        rest.push('/');
        rest.push_str(&crate::location::escape(
            part.as_os_str().as_encoded_bytes(),
        ));
    }
    if rest.matches('/').count() < 3 {
        // `//host` names the server's top: written as `//host/`.
        rest.push('/');
    }
    Location::new(&address.scheme, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_below_a_remote_is_its_address_and_its_path() {
        let top = Location::new("sftp", "//me@example.com/");
        assert_eq!(
            address_below(&top, Path::new("home/me/Two Words")).to_string(),
            "sftp://me@example.com/home/me/Two Words"
        );
        assert_eq!(
            address_below(&top, Path::new("")).to_string(),
            "sftp://me@example.com/"
        );
        let share = Location::new("smb", "//nas/photos/");
        assert_eq!(
            address_below(&share, Path::new("2024/a%b")).to_string(),
            "smb://nas/photos/2024/a%25b",
            "what would end it escaped"
        );
    }
}
