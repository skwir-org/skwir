//! Copies within one rclone mount Skwir made, by rclone itself (ADR-077,
//! point 8's second step, the user's choice of the night): where its
//! backend copies on the server (`Copy`: WebDAV's `COPY`, a cloud's own),
//! a file is copied there (`operations/copyfile`) and a folder whole
//! (`sync/copy`), its bytes never coming down and going back up through
//! the mount. Asked through rclone's remote control, which a mount Skwir
//! makes serves on a socket of the runtime folder (`--rc-addr unix://…`,
//! the user's alone: no other user can reach it, so it asks no login);
//! never on the network. The mount is then told to read the folder again
//! (`vfs/refresh`): it keeps what it listed for a minute. A mount the user
//! made serves no such socket: its copies go as before. What a backend
//! cannot copy on the server goes as before too (asked once a mount).

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use skwir_core::op::transfer::{Aside, Control, NotServed, Told, Transfer};

/// The folder of the sockets of the mounts Skwir made, in the runtime
/// folder beside theirs.
fn sockets() -> Option<PathBuf> {
    Some(skwir_core::xdg::runtime_dir()?.join("skwir").join("rc"))
}

/// The socket the remote control of the mount Skwir makes at `point`
/// answers on: named by the point, short enough for a socket's path.
pub fn socket_for(point: &Path) -> Option<PathBuf> {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in point.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    Some(sockets()?.join(format!("{hash:016x}.sock")))
}

/// The folder sockets are made in, made: a mount Skwir makes asks for its
/// own there.
///
/// # Errors
/// When it cannot be made.
pub fn socket_folder() -> anyhow::Result<PathBuf> {
    let folder = sockets().ok_or_else(|| anyhow::anyhow!("no runtime folder"))?;
    std::fs::create_dir_all(&folder)?;
    Ok(folder)
}

/// How long a call may take: a large file copied on the server.
const PATIENCE: Duration = Duration::from_secs(3600);

/// A call to the remote control on `socket`: `method` with `args`, its
/// answer's JSON. HTTP/1.1 over the socket, one call a connection.
fn call(
    socket: &Path,
    method: &str,
    args: &serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    let body = serde_json::to_vec(args)?;
    let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(PATIENCE))?;
    let head = format!(
        "POST /{method} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&body)?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer)?;
    let at = answer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("rclone's answer to {method} is no HTTP"))?;
    let status = String::from_utf8_lossy(&answer[..at]);
    let status = status
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or_default()
        .to_owned();
    let mut body = &answer[at + 4..];
    // Chunked, as Go's server sends a body it did not size.
    let unchunked;
    if String::from_utf8_lossy(&answer[..at])
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        unchunked = dechunk(body);
        body = &unchunked;
    }
    let json: serde_json::Value = serde_json::from_slice(body).unwrap_or(serde_json::Value::Null);
    if status != "200" {
        let said = json["error"].as_str().unwrap_or("no reason given");
        anyhow::bail!("rclone refused {method}: {said}");
    }
    Ok(json)
}

/// A chunked body's bytes.
fn dechunk(mut chunked: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(at) = chunked.windows(2).position(|w| w == b"\r\n") {
        let size = std::str::from_utf8(&chunked[..at])
            .ok()
            .and_then(|s| usize::from_str_radix(s.trim(), 16).ok())
            .unwrap_or(0);
        if size == 0 {
            break;
        }
        let start = at + 2;
        let Some(chunk) = chunked.get(start..start + size) else {
            break;
        };
        out.extend_from_slice(chunk);
        chunked = chunked.get(start + size + 2..).unwrap_or_default();
    }
    out
}

/// `path` below the mount at `point`, as rclone names it there.
fn on_server(point: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(point).ok()?;
    let rest = rest.to_str()?;
    Some(rest.trim_start_matches('/').to_owned())
}

/// The one each process has: the connector offers it, a test asks it.
pub fn shared() -> std::sync::Arc<RcloneCopy> {
    static SHARED: std::sync::OnceLock<std::sync::Arc<RcloneCopy>> = std::sync::OnceLock::new();
    std::sync::Arc::clone(SHARED.get_or_init(std::sync::Arc::default))
}

/// Copy `src` (read through the mount, `src_at` on the server) to
/// `dst_at`, on the server: a file by `operations/copyfile`, a folder made
/// by `operations/mkdir` and walked through the mount. Both named on the
/// mount's own remote (`fs`), the name rclone keeps it by: one made of a
/// connection string (`:webdav{7Nw38}:`) cannot be read again with a path
/// after it, as `sync/copy` would need. A link stops it: rclone keeps
/// none.
fn copy_tree(
    socket: &Path,
    fs: &str,
    (src, src_at): (&Path, &str),
    dst_at: &str,
    copied: &mut dyn FnMut(u64),
    control: &Control,
) -> anyhow::Result<()> {
    anyhow::ensure!(!control.is_cancelled(), "cancelled");
    let meta = std::fs::symlink_metadata(src)?;
    if meta.is_file() {
        call(
            socket,
            "operations/copyfile",
            &serde_json::json!({
                "srcFs": fs,
                "srcRemote": src_at,
                "dstFs": fs,
                "dstRemote": dst_at,
            }),
        )?;
        copied(meta.len());
        return Ok(());
    }
    anyhow::ensure!(meta.is_dir(), "“{}”: rclone keeps no link", src.display());
    call(
        socket,
        "operations/mkdir",
        &serde_json::json!({ "fs": fs, "remote": dst_at }),
    )?;
    for entry in std::fs::read_dir(src)? {
        let name = entry?.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("“{}” cannot be said to rclone", src.display()))?;
        copy_tree(
            socket,
            fs,
            (&src.join(name), &format!("{src_at}/{name}")),
            &format!("{dst_at}/{name}"),
            copied,
            control,
        )?;
    }
    Ok(())
}

/// rclone's copies on the server, within one mount Skwir made.
#[derive(Default)]
pub struct RcloneCopy {
    /// What each mount's remote is and whether it copies on the server,
    /// asked once a mount (`None`: it does not).
    known: Mutex<HashMap<PathBuf, Option<String>>>,
    /// The batches it copied (a test asks).
    made: AtomicUsize,
}

impl RcloneCopy {
    /// The batches it copied.
    pub fn made(&self) -> usize {
        self.made.load(Ordering::Relaxed)
    }

    /// The remote of the mount at `point`, when it copies on the server:
    /// asked of its remote control once (`vfs/list`, `operations/fsinfo`).
    fn copies_on_server(&self, point: &Path, socket: &Path) -> Option<String> {
        if let Some(known) = self.known.lock().get(point) {
            return known.clone();
        }
        let asked = || -> anyhow::Result<Option<String>> {
            let listed = call(socket, "vfs/list", &serde_json::json!({}))?;
            let Some(fs) = listed["vfses"][0].as_str() else {
                return Ok(None);
            };
            let info = call(
                socket,
                "operations/fsinfo",
                &serde_json::json!({ "fs": fs }),
            )?;
            Ok(info["Features"]["Copy"]
                .as_bool()
                .unwrap_or(false)
                .then(|| fs.to_owned()))
        };
        let found = asked().unwrap_or_else(|error| {
            tracing::debug!("rclone's remote control did not say: {error:#}");
            None
        });
        self.known.lock().insert(point.to_path_buf(), found.clone());
        found
    }
}

impl Transfer for RcloneCopy {
    fn name(&self) -> &'static str {
        "rclone"
    }

    /// Within one rclone mount Skwir made, whose remote control answers.
    fn serves(&self, from: &Path, into: &Path) -> bool {
        let (Some(a), Some(b)) = (
            skwir_core::fs::reach::of(from),
            skwir_core::fs::reach::of(into),
        ) else {
            return false;
        };
        a.point == b.point
            && a.fstype == crate::rclone::FSTYPE
            && socket_for(&a.point).is_some_and(|socket| socket.exists())
    }

    fn copy(
        &self,
        from: &Path,
        names: &[OsString],
        into: &Path,
        aside: &Aside<'_>,
        told: &Told<'_>,
        control: &Control,
    ) -> anyhow::Result<()> {
        let remote = skwir_core::fs::reach::of(into).ok_or(NotServed)?;
        let point = remote.point.clone();
        let socket = socket_for(&point).ok_or(NotServed)?;
        let fs = self.copies_on_server(&point, &socket).ok_or(NotServed)?;
        // Beside what is there, each is made in `temp` and placed after,
        // never over anything.
        let made_in = aside.temp.unwrap_or(into);
        let unsaid = |path: &Path| anyhow::anyhow!("“{}” cannot be said to rclone", path.display());
        let mut copied = 0u64;
        for name in names {
            anyhow::ensure!(!control.is_cancelled(), "cancelled");
            control.wait_while_paused();
            let src = from.join(name);
            let dst = made_in.join(name);
            let (src_at, dst_at) = (
                on_server(&point, &src).ok_or_else(|| unsaid(&src))?,
                on_server(&point, &dst).ok_or_else(|| unsaid(&dst))?,
            );
            copy_tree(
                &socket,
                &fs,
                (&src, &src_at),
                &dst_at,
                &mut |bytes| {
                    copied += bytes;
                    (told.copied)(copied);
                },
                control,
            )?;
        }
        // The mount reads the folders again: it keeps what it listed.
        for folder in [Some(made_in), aside.temp.map(|_| into)]
            .into_iter()
            .flatten()
        {
            let dir = on_server(&point, folder).ok_or_else(|| unsaid(folder))?;
            call(
                &socket,
                "vfs/refresh",
                &serde_json::json!({ "dir": if dir.is_empty() { "/".to_owned() } else { dir } }),
            )?;
        }
        if let Some(temp) = aside.temp {
            for name in names {
                match skwir_core::op::transfer::place(&temp.join(name), &into.join(name)) {
                    // Taken meanwhile: left to its step.
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Ok(()) => (told.placed)(name),
                    Err(e) => return Err(e.into()),
                }
            }
        }
        self.made.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_said_below_its_mount_and_an_answer_unchunked() {
        assert_eq!(
            on_server(Path::new("/m/point"), Path::new("/m/point/a/b")).as_deref(),
            Some("a/b")
        );
        assert_eq!(
            on_server(Path::new("/m/point"), Path::new("/m/point")).as_deref(),
            Some("")
        );
        assert_eq!(
            dechunk(b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"),
            b"hello world"
        );
    }
}
