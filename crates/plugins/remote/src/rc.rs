//! rclone's remote control, as Skwir reaches it (ADR-086): one call a
//! connection, HTTP/1.1, its JSON answer. On Unix through a socket of the
//! user's runtime folder, which no other user can reach, so it asks no
//! login. On Windows through a port of this machine's loopback, behind a
//! user and a password made for it and given to rclone in its environment
//! alone: a socket is no transport Wine has (`WSAEAFNOSUPPORT`, tried with
//! rclone's own Windows release), and a loopback port is reached by any
//! user of the machine.

use std::ffi::OsString;
use std::io::{Read, Write};
#[cfg(unix)]
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;
use skwir_core::CancelToken;
use skwir_core::op::Cancelled;

/// Where a remote control of rclone's answers.
#[derive(Clone, PartialEq, Eq)]
pub enum At {
    /// A socket of the user's runtime folder.
    #[cfg(unix)]
    Socket(PathBuf),
    /// A port of the loopback, behind a login of its own.
    Loopback {
        port: u16,
        user: String,
        pass: String,
    },
}

impl std::fmt::Debug for At {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            #[cfg(unix)]
            At::Socket(socket) => f.debug_tuple("Socket").field(socket).finish(),
            // Never its password.
            At::Loopback { port, .. } => f.debug_struct("Loopback").field("port", port).finish(),
        }
    }
}

/// How long a call may take: a large file copied on the server, a
/// browser's sign-in waited for.
const PATIENCE: Duration = Duration::from_secs(3600);

/// A login made for a remote control on the loopback.
#[derive(Clone, PartialEq, Eq)]
pub struct Login {
    pub user: String,
    pub pass: String,
}

impl std::fmt::Debug for Login {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never its password.
        f.debug_struct("Login")
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

impl Login {
    /// A login no one guesses.
    ///
    /// # Errors
    /// When the system gives no randomness.
    pub fn made() -> std::io::Result<Login> {
        Ok(Login {
            user: format!("skwir-{}", random_hex(4)?),
            pass: random_hex(24)?,
        })
    }

    /// Its line of an htpasswd file, which rclone reads as it starts
    /// (`--rc-htpasswd`) where nothing can be given in its environment:
    /// the password's SHA-1, never the password.
    pub fn htpasswd(&self) -> String {
        let digest = sha1_smol::Sha1::from(self.pass.as_bytes()).digest().bytes();
        format!("{}:{{SHA}}{}\n", self.user, base64(&digest))
    }

    /// Where it is taken, at `port`.
    pub fn at(&self, port: u16) -> At {
        At::Loopback {
            port,
            user: self.user.clone(),
            pass: self.pass.clone(),
        }
    }
}

impl At {
    /// A port of the loopback free now, and a login made for it.
    ///
    /// # Errors
    /// When no port can be had, or the system gives no randomness.
    pub fn loopback() -> std::io::Result<At> {
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))?
            .local_addr()?
            .port();
        Ok(Login::made()?.at(port))
    }

    /// What rclone serving here is given: its arguments, and what goes in
    /// its environment (the login, never on its command line).
    pub fn serving(&self) -> (Vec<OsString>, Vec<(&'static str, String)>) {
        match self {
            #[cfg(unix)]
            At::Socket(socket) => {
                let mut at = OsString::from("unix://");
                at.push(socket);
                (
                    vec!["--rc-no-auth".into(), "--rc-addr".into(), at],
                    Vec::new(),
                )
            }
            At::Loopback { port, user, pass } => (
                vec!["--rc-addr".into(), format!("127.0.0.1:{port}").into()],
                vec![
                    ("RCLONE_RC_USER", user.clone()),
                    ("RCLONE_RC_PASS", pass.clone()),
                ],
            ),
        }
    }

    /// Whether something answers there now.
    pub fn answers(&self) -> bool {
        match self {
            #[cfg(unix)]
            At::Socket(socket) => std::os::unix::net::UnixStream::connect(socket).is_ok(),
            At::Loopback { port, .. } => std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], *port)),
                Duration::from_millis(200),
            )
            .is_ok(),
        }
    }

    /// The `Authorization` header's value, where it takes a login.
    #[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
    fn authorization(&self) -> Option<String> {
        match self {
            #[cfg(unix)]
            At::Socket(_) => None,
            At::Loopback { user, pass, .. } => Some(format!(
                "Basic {}",
                base64(format!("{user}:{pass}").as_bytes())
            )),
        }
    }
}

/// `n` random bytes, in hexadecimal.
fn random_hex(n: usize) -> std::io::Result<String> {
    use std::fmt::Write as _;
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut hex = String::with_capacity(n * 2);
    for b in bytes {
        let _ = write!(hex, "{b:02x}");
    }
    Ok(hex)
}

/// `bytes` in base64, as HTTP's basic login writes it.
fn base64(bytes: &[u8]) -> String {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(LETTERS[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// One stream to a remote control, whichever its transport.
enum Stream {
    #[cfg(unix)]
    Socket(std::os::unix::net::UnixStream),
    Tcp(std::net::TcpStream),
}

impl Stream {
    fn open(at: &At) -> std::io::Result<Stream> {
        let stream = match at {
            #[cfg(unix)]
            At::Socket(socket) => Stream::Socket(std::os::unix::net::UnixStream::connect(socket)?),
            At::Loopback { port, .. } => {
                Stream::Tcp(std::net::TcpStream::connect(("127.0.0.1", *port))?)
            }
        };
        match &stream {
            #[cfg(unix)]
            Stream::Socket(s) => {
                s.set_read_timeout(Some(PATIENCE))?;
                s.set_write_timeout(Some(PATIENCE))?;
            }
            Stream::Tcp(s) => {
                s.set_read_timeout(Some(PATIENCE))?;
                s.set_write_timeout(Some(PATIENCE))?;
            }
        }
        Ok(stream)
    }

    fn try_clone(&self) -> std::io::Result<Stream> {
        Ok(match self {
            #[cfg(unix)]
            Stream::Socket(s) => Stream::Socket(s.try_clone()?),
            Stream::Tcp(s) => Stream::Tcp(s.try_clone()?),
        })
    }

    fn shutdown(&self) {
        let _ = match self {
            #[cfg(unix)]
            Stream::Socket(s) => s.shutdown(std::net::Shutdown::Both),
            Stream::Tcp(s) => s.shutdown(std::net::Shutdown::Both),
        };
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            Stream::Socket(s) => s.read(buf),
            Stream::Tcp(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            Stream::Socket(s) => s.write(buf),
            Stream::Tcp(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::Socket(s) => s.flush(),
            Stream::Tcp(s) => s.flush(),
        }
    }
}

/// A call to the remote control at `at`: `method` with `args`, its
/// answer's JSON. Cancelled, the connection is closed, which rclone takes
/// as the call's end too.
///
/// # Errors
/// What rclone refused, or why it could not be asked.
pub fn call(at: &At, method: &str, args: &Value, cancel: &CancelToken) -> anyhow::Result<Value> {
    check_cancelled(cancel)?;
    let body = serde_json::to_vec(args)?;
    let mut stream = Stream::open(at)?;
    let interrupted = stream.try_clone()?;
    let cancelled = cancel.subscribe();
    let authorization = at.authorization();
    let result = std::thread::scope(|scope| {
        let (done, finished) = crossbeam_channel::bounded::<()>(0);
        scope.spawn(move || {
            crossbeam_channel::select! {
                recv(cancelled.receiver()) -> _ => interrupted.shutdown(),
                recv(finished) -> _ => {}
            }
        });
        // Dropped even on an I/O error, before the scope joins its listener.
        let _done = done;
        request(&mut stream, method, &body, authorization.as_deref())
    });
    check_cancelled(cancel)?;
    result
}

fn check_cancelled(cancel: &CancelToken) -> anyhow::Result<()> {
    if cancel.is_cancelled() {
        return Err(Cancelled.into());
    }
    Ok(())
}

/// Keep the request synchronous: rclone ties its job to the HTTP request's
/// context, so closing the connection cancels the server's work too. An
/// asynchronous job would outlive a lost response naming its job id.
fn request(
    stream: &mut impl ReadWrite,
    method: &str,
    body: &[u8],
    authorization: Option<&str>,
) -> anyhow::Result<Value> {
    let login = authorization.map_or_else(String::new, |a| format!("Authorization: {a}\r\n"));
    let head = format!(
        "POST /{method} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         {login}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    // A server that refuses at once (a wrong login's 401) answers and
    // closes before reading what it was sent, which the system may then
    // reset: what it answered before is read all the same.
    let wrote = stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body));
    let mut answer = Vec::new();
    let read = stream.read_to_end(&mut answer);
    if answer.is_empty() {
        wrote?;
        read?;
    }
    let at = answer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("rclone's answer to {method} is no HTTP"))?;
    let head = String::from_utf8_lossy(&answer[..at]);
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or_default()
        .to_owned();
    let mut body = &answer[at + 4..];
    // Chunked, as Go's server sends a body it did not size.
    let unchunked;
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        unchunked = dechunk(body);
        body = &unchunked;
    }
    let json: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    if status == "401" {
        anyhow::bail!("rclone refused {method}: its login was refused");
    }
    if status != "200" {
        let said = json["error"].as_str().unwrap_or("no reason given");
        anyhow::bail!("rclone refused {method}: {said}");
    }
    Ok(json)
}

/// What a request is written to and read from.
trait ReadWrite: Read + Write {}

impl<T: Read + Write> ReadWrite for T {}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_s_htpasswd_line_holds_its_password_s_sha1_as_rclone_reads_it() {
        // Checked against rclone 1.75.2's `--rc-htpasswd`.
        let login = Login {
            user: "u2".into(),
            pass: "secretpass".into(),
        };
        assert_eq!(login.htpasswd(), "u2:{SHA}NkvfLtd6hUTTtxGgO2nurcxjydc=\n");
        assert!(!format!("{login:?}").contains("secretpass"));
    }

    #[test]
    fn a_login_is_written_as_http_s_basic_one() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"skwir:it's s3cret"), "c2t3aXI6aXQncyBzM2NyZXQ=");
    }

    /// A loopback's login is made for it, given to rclone in its
    /// environment, never on its line, and never said by `Debug`.
    #[test]
    fn a_loopback_s_login_stays_off_the_command_line() {
        let at = At::loopback().unwrap();
        let (args, env) = at.serving();
        #[allow(irrefutable_let_patterns)] // a socket too, on Unix
        let At::Loopback { port, pass, .. } = &at else {
            panic!("a loopback");
        };
        assert_eq!(pass.len(), 48);
        assert!(
            args.iter()
                .all(|a| !a.to_string_lossy().contains(pass.as_str()))
        );
        assert_eq!(args[1], OsString::from(format!("127.0.0.1:{port}")));
        assert!(env.iter().any(|(k, v)| *k == "RCLONE_RC_PASS" && v == pass));
        assert!(!format!("{at:?}").contains(pass.as_str()));
        assert_ne!(At::loopback().unwrap(), at, "a login each");
    }

    #[test]
    fn a_chunked_answer_is_read_whole() {
        assert_eq!(
            dechunk(b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n"),
            b"hello world"
        );
    }
}
