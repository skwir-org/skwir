//! An sshfs mount as the tools that speak to its server reach it, rsync
//! and sftp (ADR-078). A mount Skwir made shares the connection its sshfs
//! holds: sshfs's ssh is the master of a control socket
//! ([`control_path`]), so the tool's ssh asks nothing again and Skwir
//! passes no credential. A mount the user made has no connection to
//! share: the tool logs in with the user's own ssh, as their configuration
//! says (a host's alias, its key, an agent), on the port sshfs was given,
//! in the server's folder the mount table names (`me@host:/srv`). Either
//! way its ssh asks nothing (`BatchMode`): it reaches only what logs in
//! without a question.
//!
//! sshfs keeps what it read of the server 20 s (`dcache_timeout`): a
//! folder's names, and each path's attributes. It lets go of a path's when
//! something is made, removed or renamed there through the mount, of all
//! below a name renamed, and of a path the server refuses to open. What a
//! tool changes beside the mount is shown so: a folder's names read again
//! once a folder of Skwir's is made and removed in it ([`Scratch`]); a file
//! a tool removed, once opened through the mount and refused; a folder, by
//! renaming it through the mount into such a folder before a tool removes
//! it there ([`remove_beside`], by rsync where the server runs it, else
//! sftp). Through the mount, a removal or a rename waits for the server
//! under the kernel's lock of its folder, one name at a time; an open does
//! not.

use std::collections::HashMap;
use std::ffi::OsString;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};

use parking_lot::Mutex;

/// Where the ssh carrying the sshfs mount at `point` listens for others
/// to share its connection: a name of the point's own in the runtime
/// folder, short enough for a socket's path.
pub fn control_path(point: &Path) -> Option<PathBuf> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in point.as_os_str().as_encoded_bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    Some(
        skwir_core::xdg::runtime_dir()?
            .join("skwir")
            .join("ssh")
            .join(format!("{h:016x}")),
    )
}

/// What sshfs's ssh is given so others share its connection
/// ([`control_path`]): a master, never one asked to persist.
pub fn master_options(control: &Path) -> Vec<String> {
    vec![
        "ControlMaster=auto".to_owned(),
        format!("ControlPath={}", control.display()),
        "ControlPersist=no".to_owned(),
    ]
}

/// An sshfs mount whose server a tool reaches.
pub(crate) struct Mount {
    pub(crate) point: PathBuf,
    /// The server as ssh is given it: `me@host`, an IPv6 host in brackets.
    pub(crate) login: String,
    pub(crate) port: Option<u16>,
    /// What of the server the mount shows: `/` for Skwir's, which mount
    /// the server's top; as sshfs was given it for the user's (`/srv`, or
    /// a folder of the home folder: `docs`, the home folder itself empty).
    pub(crate) root: String,
    /// The control socket of the connection sshfs holds (Skwir's mounts).
    pub(crate) control: Option<PathBuf>,
    /// Whether the mount shows a link as a link: sshfs was not given
    /// `follow_symlinks`, which shows it as what it leads to. Skwir's never
    /// are; the user's, as sshfs's command line says (`false` where it
    /// cannot be read).
    pub(crate) links_shown: bool,
}

impl Mount {
    /// The sshfs mount that holds `path`; `None` for any other path, or
    /// one of Skwir's whose connection is gone.
    pub(crate) fn of(path: &Path) -> Option<Mount> {
        let remote = skwir_core::fs::reach::of(path)?;
        if remote.fstype != crate::sshfs::FSTYPE {
            return None;
        }
        let point = remote.point.clone();
        if let Some(ours) = crate::mounter::ours("sshfs", &point) {
            let address = ours.address?;
            let control = control_path(&point).filter(|c| c.exists())?;
            let user = address
                .user
                .as_deref()
                .map(|user| format!("{user}@"))
                .unwrap_or_default();
            return Some(Mount {
                login: format!("{user}{}", address.host),
                port: address.port,
                root: "/".to_owned(),
                control: Some(control),
                links_shown: true,
                point,
            });
        }
        let (login, root) = source_of(remote.source.to_str()?)?;
        let seen = seen(&point);
        Some(Mount {
            login,
            port: seen.port,
            root,
            control: None,
            links_shown: seen.links_shown,
            point,
        })
    }

    /// `path`, at or below the mount, on the server.
    pub(crate) fn on_server(&self, path: &Path) -> Option<String> {
        let below = path.strip_prefix(&self.point).ok()?.to_str()?;
        Some(match (self.root.as_str(), below) {
            ("", "") => ".".to_owned(),
            (root, "") => root.to_owned(),
            ("", below) => below.to_owned(),
            (root, below) => format!("{}/{below}", root.trim_end_matches('/')),
        })
    }

    /// rsync's name for the folder `at` of the server: `me@host:/path/`.
    pub(crate) fn spec(&self, at: &str) -> String {
        format!("{}:{}/", self.login, at.trim_end_matches('/'))
    }

    /// The options its ssh is given (each for an `-o`): `extra` (a
    /// test's), the control socket of Skwir's mount, and never a question.
    pub(crate) fn ssh_options(&self, extra: &[String]) -> Vec<String> {
        let mut options = extra.to_vec();
        if let Some(control) = &self.control {
            options.extend([
                format!("ControlPath={}", control.display()),
                "ControlMaster=no".to_owned(),
            ]);
        }
        options.push("BatchMode=yes".to_owned());
        options
    }
}

/// A folder of Skwir's made through the mount in a folder of the server,
/// named as staging folders are (`.skwir-copy-…`): what a tool removes may
/// go in it first, and removing it through the mount makes sshfs read the
/// folder it is in from the server again (module documentation).
pub(crate) struct Scratch {
    path: PathBuf,
    removed: bool,
}

impl Scratch {
    /// A new one in `folder`: a round trip to the server.
    ///
    /// # Errors
    /// The server's refusal.
    pub(crate) fn in_folder(folder: &Path) -> std::io::Result<Scratch> {
        let path = folder.join(unique(".skwir-copy-"));
        std::fs::create_dir(&path)?;
        Ok(Scratch {
            path,
            removed: false,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Removed through the mount: at once when a tool emptied it, else
    /// with what it still holds.
    ///
    /// # Errors
    /// The server's refusal.
    pub(crate) fn remove(mut self) -> std::io::Result<()> {
        self.removed = true;
        match std::fs::remove_dir(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                std::fs::remove_dir_all(&self.path)
            }
            removed => removed,
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !self.removed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// `prefix` and 16 hexadecimal digits no other call says.
pub(crate) fn unique(prefix: &str) -> String {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    format!("{prefix}{:016x}", hasher.finish())
}

/// What a tool removes beside an sshfs mount ([`remove_beside`]): files of
/// the folder, and folders moved into a folder of Skwir's in it.
pub(crate) struct Beside<'a> {
    pub(crate) files: &'a [PathBuf],
    /// The folder of Skwir's, which the tool may remove too.
    pub(crate) scratch: &'a Path,
    /// The folders, in `scratch`.
    pub(crate) trees: &'a [PathBuf],
}

/// Remove `names` of `folder`, on an sshfs mount, by `tool` beside the
/// mount, sshfs shown what went (module documentation): a file removed by
/// the tool, then opened through the mount; a folder renamed into a
/// folder of Skwir's first, the tool removing it there; a link, or
/// anything else, removed through the mount. Cut short, a folder moved
/// aside goes back where it was.
///
/// # Errors
/// What the tool or the server said, or a file still there.
pub(crate) fn remove_beside(
    folder: &Path,
    names: &[OsString],
    tool: impl FnOnce(&Beside<'_>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    // Gone meanwhile is removed.
    let gone = |error: std::io::Error| match error.kind() {
        std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(error),
    };
    let (mut files, mut trees) = (Vec::new(), Vec::new());
    for name in names {
        let path = folder.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => files.push(path),
            Ok(meta) if meta.is_dir() => trees.push(name),
            Ok(_) => std::fs::remove_file(&path).or_else(gone)?,
            Err(error) => gone(error)?,
        }
    }
    if files.is_empty() && trees.is_empty() {
        return Ok(());
    }
    let scratch = Scratch::in_folder(folder)?;
    let mut moved = Vec::new();
    let removed = (|| -> anyhow::Result<()> {
        for name in &trees {
            let to = scratch.path().join(name);
            match std::fs::rename(folder.join(name), &to) {
                Ok(()) => moved.push(to),
                Err(error) => gone(error)?,
            }
        }
        tool(&Beside {
            files: &files,
            scratch: scratch.path(),
            trees: &moved,
        })
    })();
    if removed.is_err() {
        for tree in &moved {
            if let Some(name) = tree.file_name() {
                let _ = std::fs::rename(tree, folder.join(name));
            }
        }
    }
    // What sshfs keeps of each file removed: let go of once the server
    // refuses to open it.
    let still = asked_again(&files);
    // What it keeps of the folder's names: read again once the folder of
    // Skwir's goes through the mount, or one more where the tool removed
    // it.
    match scratch.remove() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => shown_again(folder),
        ended => ended?,
    }
    removed?;
    anyhow::ensure!(still.is_empty(), "“{}” is still there", still[0].display());
    Ok(())
}

/// Of `files`, removed beside an sshfs mount, those still there, each
/// opened through the mount at once: the server refusing to open it, sshfs
/// lets go of what it kept of it. An open waits for the server under no
/// lock of its folder's, where a removal would.
fn asked_again(files: &[PathBuf]) -> Vec<PathBuf> {
    /// Opens at once: requests sshfs answers together.
    const AT_ONCE: usize = 64;
    let next = std::sync::atomic::AtomicUsize::new(0);
    let still = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..AT_ONCE.min(files.len()) {
            s.spawn(|| {
                while let Some(file) =
                    files.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                {
                    match skwir_core::fs::file::open_regular_unfollowed(file) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        _ => still.lock().push(file.clone()),
                    }
                }
            });
        }
    });
    still.into_inner()
}

/// Make sshfs read `folder` from the server the next time it is listed,
/// after a tool wrote or removed in it beside the mount: a folder of
/// Skwir's made and removed in it, two round trips.
pub(crate) fn shown_again(folder: &Path) {
    if let Err(error) = Scratch::in_folder(folder).and_then(Scratch::remove) {
        tracing::warn!(folder = %folder.display(), "sshfs may show the folder as it was for 20 s: {error}");
    }
}

/// What the mount table says an sshfs mount is of: `me@host:/srv` (or
/// `host:docs`, a folder of the home folder) as the server and its folder;
/// an IPv6 host in its brackets.
fn source_of(source: &str) -> Option<(String, String)> {
    let source = source.strip_prefix("sshfs#").unwrap_or(source);
    let mut depth = 0usize;
    let at = source.char_indices().find_map(|(i, c)| {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
        None
    })?;
    let (login, root) = (&source[..at], &source[at + 1..]);
    (!login.is_empty() && !login.ends_with('@')).then(|| (login.to_owned(), root.to_owned()))
}

/// What the command line of the sshfs of a mount the user made says.
#[derive(Clone, Copy, Default)]
struct Seen {
    port: Option<u16>,
    links_shown: bool,
}

/// What the sshfs of the mount at `point` was given, read once.
fn seen(point: &Path) -> Seen {
    static SEEN: Mutex<Option<HashMap<PathBuf, Seen>>> = Mutex::new(None);
    if let Some(known) = SEEN.lock().as_ref().and_then(|seen| seen.get(point)) {
        return *known;
    }
    let seen = sshfs_args(point).map_or_else(Seen::default, |args| {
        let args: Vec<&[u8]> = args.iter().map(Vec::as_slice).collect();
        Seen {
            port: port_in(&args),
            links_shown: !follows_links(&args),
        }
    });
    SEEN.lock()
        .get_or_insert_with(HashMap::new)
        .insert(point.to_path_buf(), seen);
    seen
}

/// The command line of the sshfs serving `point`: Linux's `/proc` says;
/// elsewhere nothing does, and the user's ssh configuration alone names
/// the port.
fn sshfs_args(point: &Path) -> Option<Vec<Vec<u8>>> {
    #[cfg(target_os = "linux")]
    {
        let point = point.as_os_str().as_encoded_bytes();
        for process in std::fs::read_dir("/proc").ok()?.flatten() {
            let Ok(line) = std::fs::read(process.path().join("cmdline")) else {
                continue;
            };
            let args: Vec<&[u8]> = line.split(|b| *b == 0).collect();
            let sshfs = args.first().is_some_and(|program| {
                program
                    .rsplit(|b| *b == b'/')
                    .next()
                    .is_some_and(|name| name.windows(5).any(|w| w == b"sshfs"))
            });
            let serving = args
                .iter()
                .any(|arg| arg.strip_suffix(b"/").unwrap_or(arg) == point);
            if sshfs && serving {
                return Some(args.into_iter().map(<[u8]>::to_vec).collect());
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = point;
        None
    }
}

/// Each option among sshfs's arguments `args`: `-o a,b`, `-oa,b`.
fn options<'a>(args: &'a [&'a [u8]]) -> impl Iterator<Item = &'a str> + 'a {
    let mut args = args.iter().filter_map(|arg| std::str::from_utf8(arg).ok());
    std::iter::from_fn(move || {
        loop {
            let arg = args.next()?;
            let options = if arg == "-o" {
                args.next()
            } else {
                arg.strip_prefix("-o")
            };
            if let Some(options) = options {
                return Some(options);
            }
        }
    })
    .flat_map(|options| options.split(','))
}

/// Whether sshfs's arguments `args` show a link as what it leads to.
fn follows_links(args: &[&[u8]]) -> bool {
    options(args).any(|option| option == "follow_symlinks")
}

/// The port among sshfs's arguments `args`: `-p 2222`, `-p2222`, `-o
/// port=2222` (among other options, `-oreconnect,port=2222`).
fn port_in(args: &[&[u8]]) -> Option<u16> {
    let mut args = args.iter().filter_map(|arg| std::str::from_utf8(arg).ok());
    while let Some(arg) = args.next() {
        let options = match arg {
            "-p" => return args.next()?.parse().ok(),
            "-o" => args.next(),
            _ if arg.starts_with("-p") => return arg[2..].parse().ok(),
            _ => arg.strip_prefix("-o"),
        };
        if let Some(port) = options
            .into_iter()
            .flat_map(|o| o.split(','))
            .find_map(|option| {
                option
                    .strip_prefix("port=")
                    .or_else(|| option.strip_prefix("Port="))
            })
        {
            return port.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_path_is_its_mount_s_folder_and_below() {
        let ours = Mount {
            point: PathBuf::from("/run/user/1000/skwir/remote/x"),
            login: "me@example.com".to_owned(),
            port: Some(2222),
            root: "/".to_owned(),
            control: Some(PathBuf::from("/run/user/1000/skwir/ssh/0123")),
            links_shown: true,
        };
        assert_eq!(
            ours.on_server(&ours.point.join("srv/it's here")).as_deref(),
            Some("/srv/it's here")
        );
        assert_eq!(ours.on_server(&ours.point).as_deref(), Some("/"));
        assert_eq!(ours.on_server(Path::new("/elsewhere")), None);
        assert_eq!(ours.spec("/srv/a b"), "me@example.com:/srv/a b/");
        assert_eq!(ours.spec("/"), "me@example.com:/");
        let options = ours.ssh_options(&["IdentityFile=k".to_owned()]);
        assert_eq!(
            options,
            [
                "IdentityFile=k",
                "ControlPath=/run/user/1000/skwir/ssh/0123",
                "ControlMaster=no",
                "BatchMode=yes"
            ]
        );
        // The user's: its own ssh, and the folder sshfs was given.
        let theirs = Mount {
            point: PathBuf::from("/home/me/srv"),
            login: "[::1]".to_owned(),
            port: None,
            root: "docs".to_owned(),
            control: None,
            links_shown: true,
        };
        assert_eq!(theirs.on_server(&theirs.point).as_deref(), Some("docs"));
        assert_eq!(
            theirs.on_server(&theirs.point.join("a")).as_deref(),
            Some("docs/a")
        );
        assert_eq!(theirs.spec("docs/a"), "[::1]:docs/a/");
        assert_eq!(theirs.ssh_options(&[]), ["BatchMode=yes"]);
    }

    /// What the mount table says of an sshfs mount the user made, and the
    /// port among its command line's words.
    #[test]
    fn a_mount_the_user_made_names_its_server_and_port() {
        assert_eq!(
            source_of("me@host.example:/srv/it's"),
            Some(("me@host.example".to_owned(), "/srv/it's".to_owned()))
        );
        assert_eq!(source_of("host:"), Some(("host".to_owned(), String::new())));
        assert_eq!(
            source_of("me@[fe80::1]:/a:b"),
            Some(("me@[fe80::1]".to_owned(), "/a:b".to_owned()))
        );
        assert_eq!(
            source_of("sshfs#me@host:/x"),
            Some(("me@host".to_owned(), "/x".to_owned()))
        );
        assert_eq!(source_of("/dev/sda1"), None);
        assert_eq!(source_of(":/passive"), None);
        let port = |words: &[&str]| {
            let words: Vec<&[u8]> = words.iter().map(|w| w.as_bytes()).collect();
            port_in(&words)
        };
        assert_eq!(port(&["sshfs", "-p", "2222", "me@h:/", "/m"]), Some(2222));
        assert_eq!(port(&["sshfs", "-p2200", "me@h:/", "/m"]), Some(2200));
        assert_eq!(
            port(&["sshfs", "-o", "reconnect,port=2201", "h:", "/m"]),
            Some(2201)
        );
        assert_eq!(port(&["sshfs", "-oPort=2202", "h:", "/m"]), Some(2202));
        assert_eq!(port(&["sshfs", "h:", "/m", "-o", "reconnect"]), None);
        let follows = |words: &[&str]| {
            let words: Vec<&[u8]> = words.iter().map(|w| w.as_bytes()).collect();
            follows_links(&words)
        };
        assert!(follows(&[
            "sshfs",
            "-o",
            "reconnect,follow_symlinks",
            "h:",
            "/m"
        ]));
        assert!(follows(&["sshfs", "-ofollow_symlinks", "h:", "/m"]));
        assert!(!follows(&["sshfs", "-o", "reconnect", "h:", "/m"]));
        assert!(!follows(&["sshfs", "h:", "/follow_symlinks"]));
    }
}
