//! What ssh asks while Skwir connects to a server reaches the window
//! (ADR-077). ssh runs `$SSH_ASKPASS prompt` for a password, a key's
//! passphrase or an unknown host; Skwir sets it to its own executable,
//! which, finding [`SOCKET_VAR`] in its environment, asks over that
//! socket ([`ask`]) instead of starting a window. The socket is the
//! connection's own ([`Server`]): made private in the user's runtime
//! folder for that one connection, and removed after it.
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
    #[cfg(unix)]
    {
        let mut stream = std::os::unix::net::UnixStream::connect(socket).ok()?;
        write_frame(&mut stream, &serde_json::to_vec(question).ok()?).ok()?;
        let answer: Answer = serde_json::from_slice(&read_frame(&mut stream).ok()?).ok()?;
        answer.answer
    }
    #[cfg(not(unix))]
    {
        let _ = (socket, question);
        None
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
    #[cfg(unix)]
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Server {
    /// Listen in `folder` (private to the user), answering each question
    /// with `answer`, one at a time, on a thread of its own.
    ///
    /// # Errors
    /// When the socket cannot be made, or off Unix.
    pub fn start(
        folder: &Path,
        answer: impl Fn(Question) -> Option<String> + Send + 'static,
    ) -> std::io::Result<Server> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = folder.join(format!(
                "askpass-{}-{}.sock",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_file(&path);
            let listener = std::os::unix::net::UnixListener::bind(&path)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            let stop = std::sync::Arc::new(AtomicBool::new(false));
            let stopped = std::sync::Arc::clone(&stop);
            std::thread::Builder::new()
                .name("skwir-askpass".into())
                .spawn(move || {
                    for stream in listener.incoming() {
                        if stopped.load(Ordering::Acquire) {
                            return;
                        }
                        let Ok(mut stream) = stream else { continue };
                        let Ok(question) = read_frame(&mut stream)
                            .ok()
                            .and_then(|bytes| serde_json::from_slice::<Question>(&bytes).ok())
                            .ok_or(())
                        else {
                            continue;
                        };
                        let reply = Answer {
                            answer: answer(question),
                        };
                        if let Ok(bytes) = serde_json::to_vec(&reply) {
                            let _ = write_frame(&mut stream, &bytes);
                        }
                    }
                })?;
            Ok(Server { path, stop })
        }
        #[cfg(not(unix))]
        {
            let _ = (folder, answer);
            Err(std::io::Error::other(
                "asking for a password through ssh is not available on Windows yet",
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
        #[cfg(unix)]
        {
            self.stop.store(true, std::sync::atomic::Ordering::Release);
            // Woken from its wait for a connection, the thread sees it stop.
            let _ = std::os::unix::net::UnixStream::connect(&self.path);
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(all(test, unix))]
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
}
