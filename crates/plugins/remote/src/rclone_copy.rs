//! Copies within one rclone mount Skwir made, by rclone itself (ADR-077,
//! point 8's second step, the user's choice of the night): where its
//! backend copies on the server (`Copy`: WebDAV's `COPY`, a cloud's own),
//! a file is copied there (`operations/copyfile`) and a folder walked
//! between pause boundaries, its bytes never coming down and going back
//! up through the mount. Asked through rclone's remote control, which a mount Skwir
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
use skwir_core::CancelToken;
use skwir_core::op::Cancelled;
use skwir_core::op::transfer::{Aside, Control, NotServed, Told, Transfer};

/// The folder of the sockets of the mounts Skwir made, in the runtime
/// folder beside theirs.
pub(crate) fn sockets() -> Option<PathBuf> {
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
pub(crate) fn call(
    socket: &Path,
    method: &str,
    args: &serde_json::Value,
    cancel: &CancelToken,
) -> anyhow::Result<serde_json::Value> {
    check_cancelled(cancel)?;
    let body = serde_json::to_vec(args)?;
    let mut stream = std::os::unix::net::UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(PATIENCE))?;
    stream.set_write_timeout(Some(PATIENCE))?;
    let interrupted = stream.try_clone()?;
    let cancelled = cancel.subscribe();
    let result = std::thread::scope(|scope| {
        let (done, finished) = crossbeam_channel::bounded::<()>(0);
        scope.spawn(move || {
            crossbeam_channel::select! {
                recv(cancelled.receiver()) -> _ => {
                    let _ = interrupted.shutdown(std::net::Shutdown::Both);
                }
                recv(finished) -> _ => {}
            }
        });
        // Dropped even on an I/O error, before the scope joins its listener.
        let _done = done;
        request(&mut stream, method, &body)
    });
    check_cancelled(cancel)?;
    result
}

/// Keep the request synchronous: rclone ties its job to the HTTP request's
/// context, so closing the connection cancels the server's work too. An
/// asynchronous job would outlive a lost response naming its job id.
fn request(
    stream: &mut std::os::unix::net::UnixStream,
    method: &str,
    body: &[u8],
) -> anyhow::Result<serde_json::Value> {
    let head = format!(
        "POST /{method} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
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

fn check_cancelled(cancel: &CancelToken) -> anyhow::Result<()> {
    if cancel.is_cancelled() {
        return Err(Cancelled.into());
    }
    Ok(())
}

/// Every unit, including children of a folder, starts only while running.
fn checkpoint(control: &Control) -> anyhow::Result<()> {
    control.wait_while_paused();
    check_cancelled(control.cancel())
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
    checkpoint(control)?;
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
            control.cancel(),
        )?;
        copied(meta.len());
        return Ok(());
    }
    anyhow::ensure!(meta.is_dir(), "“{}”: rclone keeps no link", src.display());
    call(
        socket,
        "operations/mkdir",
        &serde_json::json!({ "fs": fs, "remote": dst_at }),
        control.cancel(),
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
    fn copies_on_server(
        &self,
        point: &Path,
        socket: &Path,
        cancel: &CancelToken,
    ) -> anyhow::Result<Option<String>> {
        check_cancelled(cancel)?;
        if let Some(known) = self.known.lock().get(point) {
            return Ok(known.clone());
        }
        let asked = || -> anyhow::Result<Option<String>> {
            let listed = call(socket, "vfs/list", &serde_json::json!({}), cancel)?;
            let Some(fs) = listed["vfses"][0].as_str() else {
                return Ok(None);
            };
            let info = call(
                socket,
                "operations/fsinfo",
                &serde_json::json!({ "fs": fs }),
                cancel,
            )?;
            Ok(info["Features"]["Copy"]
                .as_bool()
                .unwrap_or(false)
                .then(|| fs.to_owned()))
        };
        let asked = asked();
        // A cancelled probe says nothing about this mount's capabilities.
        check_cancelled(cancel)?;
        let found = asked.unwrap_or_else(|error| {
            tracing::debug!("rclone's remote control did not say: {error:#}");
            None
        });
        self.known.lock().insert(point.to_path_buf(), found.clone());
        Ok(found)
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
        checkpoint(control)?;
        let fs = self
            .copies_on_server(&point, &socket, control.cancel())?
            .ok_or(NotServed)?;
        // Beside what is there, each is made in `temp` and placed after,
        // never over anything.
        let made_in = aside.temp.unwrap_or(into);
        let unsaid = |path: &Path| anyhow::anyhow!("“{}” cannot be said to rclone", path.display());
        let mut copied = 0u64;
        for name in names {
            checkpoint(control)?;
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
            checkpoint(control)?;
            let dir = on_server(&point, folder).ok_or_else(|| unsaid(folder))?;
            call(
                &socket,
                "vfs/refresh",
                &serde_json::json!({ "dir": if dir.is_empty() { "/".to_owned() } else { dir } }),
                control.cancel(),
            )?;
        }
        if let Some(temp) = aside.temp {
            for name in names {
                checkpoint(control)?;
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

    use std::io::{BufRead, BufReader};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::sync::{Arc, Weak};

    use crossbeam_channel::{Receiver, Sender};
    use skwir_core::op::{
        Change, ExecResult, Executor, Operation, Plan, PlanId, StepContext, StepStatus,
    };
    use skwir_core::scheduler::Scheduler;

    const DEADLINE: Duration = Duration::from_secs(5);

    struct Request {
        stream: UnixStream,
        method: String,
        args: serde_json::Value,
    }

    impl Request {
        fn read(mut stream: UnixStream) -> Option<Self> {
            stream.set_read_timeout(Some(DEADLINE)).unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                return None;
            }
            let method = line.split_whitespace().nth(1).unwrap()[1..].to_owned();
            let mut size = None;
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.strip_prefix("Content-Length: ") {
                    size = Some(value.trim().parse::<usize>().unwrap());
                }
            }
            let mut body = vec![0; size.unwrap()];
            reader.read_exact(&mut body).unwrap();
            let args = serde_json::from_slice(&body).unwrap();
            Some(Self {
                stream,
                method,
                args,
            })
        }

        fn answer(mut self, answer: &serde_json::Value) {
            let body = serde_json::to_vec(answer).unwrap();
            write!(
                self.stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            self.stream.write_all(&body).unwrap();
        }

        /// The stand-in changes only this test's tree, as the server would.
        fn apply(self, root: &Path) {
            match self.method.as_str() {
                "operations/mkdir" => {
                    std::fs::create_dir_all(root.join(self.args["remote"].as_str().unwrap()))
                        .unwrap();
                }
                "operations/copyfile" => {
                    let src = root.join(self.args["srcRemote"].as_str().unwrap());
                    let dst = root.join(self.args["dstRemote"].as_str().unwrap());
                    std::fs::copy(&src, &dst).unwrap();
                    let modified = std::fs::metadata(src).unwrap().modified().unwrap();
                    std::fs::File::options()
                        .write(true)
                        .open(dst)
                        .unwrap()
                        .set_times(std::fs::FileTimes::new().set_modified(modified))
                        .unwrap();
                }
                method => panic!("unexpected request: {method}"),
            }
            self.answer(&serde_json::json!({}));
        }
    }

    struct Server {
        socket: PathBuf,
        requests: Receiver<Request>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Server {
        fn new(root: &Path) -> Self {
            let socket = root.join("rc.sock");
            let listener = UnixListener::bind(&socket).unwrap();
            let (send, requests) = crossbeam_channel::unbounded();
            let thread = std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Some(request) = Request::read(stream.unwrap()) else {
                        break;
                    };
                    if send.send(request).is_err() {
                        break;
                    }
                }
            });
            Self {
                socket,
                requests,
                thread: Some(thread),
            }
        }

        fn next(&self) -> Request {
            self.requests.recv_timeout(DEADLINE).unwrap()
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            drop(UnixStream::connect(&self.socket));
            self.thread.take().unwrap().join().unwrap();
        }
    }

    /// The production recursion runs with a real executor-owned Control
    /// and staging folder, without requiring a mounted remote in a unit test.
    #[derive(Clone, Debug)]
    struct TreeCopy {
        root: PathBuf,
        socket: PathBuf,
        progress: Sender<u64>,
        pause: Option<(Weak<Executor>, PlanId)>,
    }

    impl Operation for TreeCopy {
        fn kind(&self) -> &'static str {
            "test_rclone_copy"
        }
        fn args(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn describe(&self) -> Change {
            Change::Copy {
                from: self.root.join("album"),
                to: self.root.join("destination/album"),
            }
        }
        fn check(&self, _: &StepContext) -> StepStatus {
            StepStatus::Pending { reversible: false }
        }
        fn clone_box(&self) -> Box<dyn Operation> {
            Box::new(self.clone())
        }
        fn apply(&self, ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
            let into = self.root.join("destination");
            let staging = ctx.stage(&into)?;
            let src = self.root.join("album");
            let dst = staging.path().join("album");
            let mut copied = 0;
            copy_tree(
                &self.socket,
                "test:",
                (&src, "album"),
                &on_server(&self.root, &dst).unwrap(),
                &mut |bytes| {
                    copied += bytes;
                    ctx.advance(bytes);
                    if copied == 1
                        && let Some((executor, id)) = &self.pause
                    {
                        assert!(executor.upgrade().unwrap().pause(*id));
                    }
                    self.progress.send(copied).unwrap();
                },
                &ctx.control(),
            )?;
            ctx.check_cancelled()?;
            staging.place(&dst, &into.join("album"))?;
            Ok(None)
        }
    }

    struct Running {
        executor: Arc<Executor>,
        id: PlanId,
        progress: Receiver<u64>,
        result: Receiver<ExecResult>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Running {
        fn new(root: &Path, socket: &Path, pause: bool) -> Self {
            let src = root.join("album");
            std::fs::create_dir_all(src.join("nested")).unwrap();
            for name in ["one", "two", "three"] {
                std::fs::write(src.join("nested").join(name), b"x").unwrap();
            }
            let into = root.join("destination");
            std::fs::create_dir(&into).unwrap();
            let mut plan = Plan::new("copy album");
            let id = plan.id;
            let executor = Executor::new(Scheduler::new(1));
            let (send_progress, progress) = crossbeam_channel::unbounded();
            plan.push(TreeCopy {
                root: root.to_path_buf(),
                socket: socket.to_path_buf(),
                progress: send_progress,
                pause: pause.then(|| (Arc::downgrade(&executor), id)),
            });
            let (send_result, result) = crossbeam_channel::bounded(1);
            let worker = executor.clone();
            let thread = std::thread::spawn(move || {
                let result = worker.execute_blocking(&plan, &CancelToken::new());
                send_result.send(result).unwrap();
            });
            Self {
                executor,
                id,
                progress,
                result,
                thread: Some(thread),
            }
        }

        fn finish(&self) -> ExecResult {
            self.result
                .recv_timeout(DEADLINE)
                .expect("the plan finishes promptly")
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.executor.cancel(self.id);
            self.thread.take().unwrap().join().unwrap();
        }
    }

    fn finish_first_child(server: &Server, root: &Path) {
        for method in [
            "operations/mkdir",
            "operations/mkdir",
            "operations/copyfile",
        ] {
            let request = server.next();
            assert_eq!(request.method, method);
            request.apply(root);
        }
    }

    #[test]
    fn a_recursive_copy_pauses_between_children_and_resumes_without_repeating_them() {
        let root = tempfile::tempdir().unwrap();
        let server = Server::new(root.path());
        let running = Running::new(root.path(), &server.socket, true);
        finish_first_child(&server, root.path());
        assert_eq!(running.progress.recv_timeout(DEADLINE).unwrap(), 1);
        assert!(
            server
                .requests
                .recv_timeout(Duration::from_millis(150))
                .is_err(),
            "a paused folder starts no next child"
        );
        assert!(running.executor.resume(running.id));
        for expected in [2, 3] {
            let request = server.next();
            assert_eq!(request.method, "operations/copyfile");
            request.apply(root.path());
            assert_eq!(running.progress.recv_timeout(DEADLINE).unwrap(), expected);
        }
        let result = running.finish();
        assert!(result.error.is_none(), "{:?}", result.error);
        assert_eq!(result.applied, 1);
        assert!(!result.cancelled);
        for name in ["one", "two", "three"] {
            assert_eq!(
                std::fs::read(root.path().join("destination/album/nested").join(name)).unwrap(),
                b"x"
            );
        }
        assert!(server.requests.try_recv().is_err(), "no repeated requests");
    }

    #[test]
    fn cancelling_a_paused_recursive_copy_starts_no_next_child() {
        let root = tempfile::tempdir().unwrap();
        let server = Server::new(root.path());
        let running = Running::new(root.path(), &server.socket, true);
        finish_first_child(&server, root.path());
        assert_eq!(running.progress.recv_timeout(DEADLINE).unwrap(), 1);
        assert!(running.executor.cancel(running.id));
        assert!(running.finish().cancelled);
        assert!(server.requests.try_recv().is_err());
        assert!(running.progress.try_recv().is_err());
        assert_eq!(
            std::fs::read_dir(root.path().join("destination"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn cancelling_a_stalled_response_closes_it_and_counts_only_finished_children() {
        let root = tempfile::tempdir().unwrap();
        let server = Server::new(root.path());
        let running = Running::new(root.path(), &server.socket, false);
        finish_first_child(&server, root.path());
        assert_eq!(running.progress.recv_timeout(DEADLINE).unwrap(), 1);
        let mut stalled = server.next();
        assert_eq!(stalled.method, "operations/copyfile");
        // Deliberately no response: the old one-hour timeout cannot end this.
        assert!(running.executor.cancel(running.id));
        assert!(running.finish().cancelled);
        assert_eq!(
            stalled.stream.read(&mut [0]).unwrap(),
            0,
            "the server sees cancellation"
        );
        assert!(
            running.progress.try_recv().is_err(),
            "no receipt for the unfinished child"
        );
        assert!(server.requests.try_recv().is_err(), "no later child starts");
        assert_eq!(
            std::fs::read_dir(root.path().join("destination"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn a_cancelled_capability_probe_does_not_disable_server_copies() {
        let root = tempfile::tempdir().unwrap();
        let server = Server::new(root.path());
        let transfer = RcloneCopy::default();
        let cancel = CancelToken::new();
        std::thread::scope(|scope| {
            let probe =
                scope.spawn(|| transfer.copies_on_server(root.path(), &server.socket, &cancel));
            let mut stalled = server.next();
            assert_eq!(stalled.method, "vfs/list");
            cancel.cancel();
            assert!(probe.join().unwrap().unwrap_err().is::<Cancelled>());
            assert_eq!(stalled.stream.read(&mut [0]).unwrap(), 0);
        });
        assert!(transfer.known.lock().is_empty());
        std::thread::scope(|scope| {
            let probe = scope.spawn(|| {
                transfer.copies_on_server(root.path(), &server.socket, &CancelToken::new())
            });
            server
                .next()
                .answer(&serde_json::json!({"vfses": ["test:"]}));
            let info = server.next();
            assert_eq!(info.method, "operations/fsinfo");
            info.answer(&serde_json::json!({"Features": {"Copy": true}}));
            assert_eq!(probe.join().unwrap().unwrap().as_deref(), Some("test:"));
        });
    }

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
