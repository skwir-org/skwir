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

use std::collections::HashMap;
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
                point,
            });
        }
        let (login, root) = source_of(remote.source.to_str()?)?;
        Some(Mount {
            login,
            port: port_of(&point),
            root,
            control: None,
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

/// The port the sshfs of the mount at `point` was given, read once.
fn port_of(point: &Path) -> Option<u16> {
    static PORTS: Mutex<Option<HashMap<PathBuf, Option<u16>>>> = Mutex::new(None);
    if let Some(known) = PORTS.lock().as_ref().and_then(|ports| ports.get(point)) {
        return *known;
    }
    let port = sshfs_port(point);
    PORTS
        .lock()
        .get_or_insert_with(HashMap::new)
        .insert(point.to_path_buf(), port);
    port
}

/// The port the sshfs serving `point` was given (`-p 2222`, `-o
/// port=2222`), from its command line: Linux's `/proc` says; elsewhere
/// the user's ssh configuration alone does.
fn sshfs_port(point: &Path) -> Option<u16> {
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
                return port_in(&args);
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

/// The port among sshfs's arguments `args`: `-p 2222`, `-p2222`, `-o
/// port=2222` (among other options, `-oreconnect,port=2222`).
#[cfg(any(target_os = "linux", test))]
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
    }
}
