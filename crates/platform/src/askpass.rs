//! What ssh asks while Skwir connects to a server reaches the window
//! (ADR-077). ssh runs `$SSH_ASKPASS prompt` for a password, a key's
//! passphrase or an unknown host; Skwir sets it to its own executable,
//! which, finding [`SOCKET_VAR`] in its environment, asks over that
//! socket ([`ask`]) instead of starting a window. The socket is the
//! connection's own ([`Server`]): made private in the user's runtime
//! folder for that one connection, and removed after it. On Windows it is
//! a named pipe of a name no one guesses, which only the user opens and
//! no other machine reaches (`windows::pipe`), there until dropped.
//!
//! What crosses is a question and its answer, never logged: a frame of
//! its length then its JSON, each way.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The variable naming the socket to ask through.
pub const SOCKET_VAR: &str = "SKWIR_ASKPASS";

/// What ssh asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A secret, typed and not shown: a password, a passphrase.
    Secret,
    /// Yes or no: a host never seen, whose key it shows.
    Confirm,
    /// Something to know, no answer (touch the security key).
    Notice,
}

/// A question as ssh put it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Question {
    pub prompt: String,
    pub kind: Kind,
}

/// What `prompt` asks, as ssh says it: `SSH_ASKPASS_PROMPT` names a
/// confirmation or a notice; a question ending in `(yes/no…)` wants a
/// word; anything else is a secret.
pub fn kind_of(prompt: &str, said: Option<&str>) -> Kind {
    match said {
        Some("none") => Kind::Notice,
        Some("confirm") => Kind::Confirm,
        _ if prompt.contains("(yes/no") => Kind::Confirm,
        _ => Kind::Secret,
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Answer {
    answer: Option<String>,
}

/// The longest frame read: a prompt, a password.
const FRAME: u32 = 64 * 1024;

fn write_frame(out: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    let len = u32::try_from(bytes.len()).map_err(|_| std::io::Error::other("frame too long"))?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(bytes)?;
    out.flush()
}

fn read_frame(input: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    input.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len);
    if len > FRAME {
        return Err(std::io::Error::other("frame too long"));
    }
    let mut bytes = vec![0u8; len as usize];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Ask through `socket` and wait for the answer: `None` for Cancel (or no
/// one there).
pub fn ask(socket: &Path, question: &Question) -> Option<String> {
    let mut stream = connect(socket)?;
    write_frame(&mut stream, &serde_json::to_vec(question).ok()?).ok()?;
    let answer: Answer = serde_json::from_slice(&read_frame(&mut stream).ok()?).ok()?;
    answer.answer
}

#[cfg(unix)]
fn connect(socket: &Path) -> Option<std::os::unix::net::UnixStream> {
    std::os::unix::net::UnixStream::connect(socket).ok()
}

/// The pipe opened as a client that lets the server know who it is, never
/// act as it: while its next instance is made, every one is busy, a
/// moment waited out.
#[cfg(windows)]
fn connect(pipe: &Path) -> Option<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    /// `ERROR_PIPE_BUSY`.
    const BUSY: i32 = 231;
    /// `SECURITY_IDENTIFICATION`.
    const IDENTIFICATION: u32 = 0x0001_0000;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .security_qos_flags(IDENTIFICATION)
            .open(pipe)
        {
            Ok(file) => return Some(file),
            Err(error)
                if error.raw_os_error() == Some(BUSY) && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }
}

#[cfg(not(any(unix, windows)))]
fn connect(_: &Path) -> Option<std::fs::File> {
    None
}

/// Read `stream`'s question, answer it with `answer`, write the answer.
fn serve(stream: &mut (impl Read + Write), answer: &impl Fn(Question) -> Option<String>) {
    let Some(question) = read_frame(stream)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Question>(&bytes).ok())
    else {
        return;
    };
    let reply = Answer {
        answer: answer(question),
    };
    if let Ok(bytes) = serde_json::to_vec(&reply) {
        let _ = write_frame(stream, &bytes);
    }
}

/// What the `skwir` executable does run as `SSH_ASKPASS` (or as rclone's
/// `--password-command`): ask the window `prompt`, print the answer for
/// the program reading it. Whether it was answered: the exit status a
/// confirmation is read from.
pub fn answer_as_askpass(socket: &Path, prompt: Option<&str>) -> bool {
    let prompt = prompt.unwrap_or("Password").to_owned();
    let said = std::env::var("SSH_ASKPASS_PROMPT").ok();
    let kind = kind_of(&prompt, said.as_deref());
    let Some(answer) = ask(socket, &Question { prompt, kind }) else {
        return kind == Kind::Notice;
    };
    if kind == Kind::Confirm && said.as_deref() == Some("confirm") {
        // ssh-agent's confirmation reads the status alone.
        return true;
    }
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(answer.as_bytes());
    let _ = out.write_all(b"\n");
    let _ = out.flush();
    true
}

/// A socket answering questions until dropped, then removed.
pub struct Server {
    path: PathBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// A number no other server of this process was given.
fn next_number() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Server {
    /// Listen in `folder` (private to the user), answering each question
    /// with `answer`, one at a time, on a thread of its own. On Windows a
    /// pipe, in no folder.
    ///
    /// # Errors
    /// When the socket cannot be made.
    pub fn start(
        folder: &Path,
        answer: impl Fn(Question) -> Option<String> + Send + 'static,
    ) -> std::io::Result<Server> {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let stopped = std::sync::Arc::clone(&stop);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = folder.join(format!(
                "askpass-{}-{}.sock",
                std::process::id(),
                next_number()
            ));
            let _ = std::fs::remove_file(&path);
            let listener = std::os::unix::net::UnixListener::bind(&path)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            std::thread::Builder::new()
                .name("skwir-askpass".into())
                .spawn(move || {
                    for stream in listener.incoming() {
                        if stopped.load(Ordering::Acquire) {
                            return;
                        }
                        if let Ok(mut stream) = stream {
                            serve(&mut stream, &answer);
                        }
                    }
                })?;
            Ok(Server { path, stop })
        }
        #[cfg(windows)]
        {
            use std::hash::{BuildHasher as _, Hasher as _};
            let _ = folder;
            // Seeded by the system's randomness: no one guesses it to
            // make it first.
            let random = std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish();
            let path = PathBuf::from(format!(
                r"\\.\pipe\skwir-askpass-{}-{}-{random:016x}",
                std::process::id(),
                next_number()
            ));
            let name = path.as_os_str().to_owned();
            let mut waiting = crate::windows::pipe::instance(&name, true)?;
            std::thread::Builder::new()
                .name("skwir-askpass".into())
                .spawn(move || {
                    loop {
                        let Ok(mut connected) = crate::windows::pipe::accept(waiting) else {
                            return;
                        };
                        if stopped.load(Ordering::Acquire) {
                            return;
                        }
                        // The next before this one is answered: a second
                        // asking finds the pipe busy a moment, never gone.
                        let next = crate::windows::pipe::instance(&name, false);
                        serve(&mut connected, &answer);
                        // Its answer read before the connection goes.
                        let _ = connected.sync_all();
                        drop(connected);
                        let Ok(next) = next else { return };
                        waiting = next;
                    }
                })?;
            Ok(Server { path, stop })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (folder, answer, stopped);
            Err(std::io::Error::other(
                "asking for a password through ssh is not available on this system yet",
            ))
        }
    }

    /// Where it listens: what [`SOCKET_VAR`] names.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        // Woken from its wait for a connection, the thread sees it stop.
        let _ = connect(&self.path);
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_ssh_asks_is_told_apart() {
        assert_eq!(kind_of("me@example.com's password: ", None), Kind::Secret);
        assert_eq!(
            kind_of(
                "Enter passphrase for key '/home/me/.ssh/id_ed25519': ",
                None
            ),
            Kind::Secret
        );
        assert_eq!(
            kind_of(
                "The authenticity of host 'example.com' can't be established.\n\
                 Are you sure you want to continue connecting (yes/no/[fingerprint])? ",
                None
            ),
            Kind::Confirm
        );
        assert_eq!(kind_of("Confirm user presence", Some("none")), Kind::Notice);
    }

    #[test]
    fn a_question_is_answered_over_the_socket_and_the_socket_goes() {
        let folder = tempfile::tempdir().unwrap();
        let server = Server::start(folder.path(), |q: Question| {
            (q.kind == Kind::Secret).then(|| format!("answer to {}", q.prompt))
        })
        .unwrap();
        let path = server.path().to_owned();
        let asked = ask(
            &path,
            &Question {
                prompt: "pw".into(),
                kind: Kind::Secret,
            },
        );
        assert_eq!(asked.as_deref(), Some("answer to pw"));
        let refused = ask(
            &path,
            &Question {
                prompt: "sure?".into(),
                kind: Kind::Confirm,
            },
        );
        assert_eq!(refused, None);
        drop(server);
        // A pipe's name is no file: asking it is all there is.
        #[cfg(unix)]
        assert!(!path.exists());
        assert_eq!(
            ask(
                &path,
                &Question {
                    prompt: "pw".into(),
                    kind: Kind::Secret
                }
            ),
            None
        );
    }

    #[test]
    fn questions_asked_at_once_are_each_answered_in_turn() {
        let folder = tempfile::tempdir().unwrap();
        let server = Server::start(folder.path(), |q: Question| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            Some(format!("answer to {}", q.prompt))
        })
        .unwrap();
        let path = server.path().to_owned();
        let askers: Vec<_> = (0..4)
            .map(|n| {
                let path = path.clone();
                std::thread::spawn(move || {
                    ask(
                        &path,
                        &Question {
                            prompt: format!("q{n}"),
                            kind: Kind::Secret,
                        },
                    )
                })
            })
            .collect();
        for (n, asker) in askers.into_iter().enumerate() {
            assert_eq!(asker.join().unwrap(), Some(format!("answer to q{n}")));
        }
    }

    /// A pipe of the name made by anyone first is never taken for the
    /// askpass's own.
    #[cfg(windows)]
    #[test]
    fn a_pipe_named_first_by_another_is_refused() {
        let name = std::ffi::OsString::from(format!(
            r"\\.\pipe\skwir-askpass-test-{}",
            std::process::id()
        ));
        let first = crate::windows::pipe::instance(&name, true).unwrap();
        assert!(crate::windows::pipe::instance(&name, false).is_ok());
        // Wine makes a second "first" instance; Windows refuses it.
        if !skwir_core::system::under_wine("FILE_FLAG_FIRST_PIPE_INSTANCE") {
            assert!(crate::windows::pipe::instance(&name, true).is_err());
        }
        drop(first);
    }
}
