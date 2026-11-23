//! An engine of a test's making, on a socket of the test's own: for a
//! client's tests to run without skwird, and to answer what they ask it
//! to (a root covered or not, given entries).
//!
//! It answers `hello`; `coverage` and `sync` with the coverage given for
//! the longest root holding the path asked (`outside` for none); `query`
//! with the entries given below the root asked, a name matched as a
//! substring after lowercasing (globs are not its); `changes` as too old;
//! `exclude` and `watch` with what was asked. Made `silent`, it says hello
//! and then nothing, as an engine that hangs: for a client's test of
//! giving up on a call.

use std::path::PathBuf;

use skwird_protocol::{Coverage, Entry};

/// What the fake engine answers.
#[derive(Clone, Debug, Default)]
pub struct Script {
    pub roots: Vec<PathBuf>,
    /// Each root's coverage, for the paths below it.
    pub coverage: Vec<(PathBuf, Coverage)>,
    pub entries: Vec<Entry>,
    /// Say hello, then answer nothing more.
    pub silent: bool,
}

/// A fake engine serving until dropped.
pub struct Fake {
    pub socket: PathBuf,
    #[cfg(unix)]
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(unix)]
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
mod imp {
    use std::io::BufReader;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use skwird_protocol::{
        self as wire, Answer, Ask, Coverage, Done, Proof, Request, Response, Welcome, Why, WirePath,
    };

    use super::{Fake, Script};

    fn outside() -> Coverage {
        Coverage {
            names: Proof::NotCovered(Why::Outside),
            attrs: Proof::NotCovered(Why::Outside),
            holes: Vec::new(),
        }
    }

    fn coverage(script: &Script, root: &Path) -> Coverage {
        script
            .coverage
            .iter()
            .filter(|(r, _)| root.starts_with(r))
            .max_by_key(|(r, _)| r.components().count())
            .map_or_else(outside, |(_, c)| c.clone())
    }

    fn answer(script: &Script, ask: Ask, out: &mut UnixStream, id: u64) -> std::io::Result<()> {
        let mut say = |answer| wire::write(out, &Response { id, answer });
        match ask {
            Ask::Hello(_) => say(Answer::Hello(Welcome {
                version: wire::VERSION,
                scope: wire::Scope::User,
                engine: "skwird-client's fake engine".into(),
                roots: script.roots.iter().map(|r| WirePath::of(r)).collect(),
            })),
            Ask::Coverage { root } | Ask::Sync { root } => {
                say(Answer::Coverage(coverage(script, &root.to_path())))
            }
            Ask::Query(q) => {
                let root = q.root.to_path();
                let text = q.name.as_ref().map(|n| n.text.to_lowercase());
                let found: Vec<_> = script
                    .entries
                    .iter()
                    .filter(|e| {
                        let path = e.path.to_path();
                        path.starts_with(&root)
                            && path != root
                            && text.as_ref().is_none_or(|t| {
                                path.file_name().is_some_and(|n| {
                                    n.to_string_lossy().to_lowercase().contains(t.as_str())
                                })
                            })
                    })
                    .cloned()
                    .collect();
                let count = found.len() as u64;
                if !found.is_empty() {
                    say(Answer::Results(found))?;
                }
                say(Answer::Done(Done {
                    coverage: coverage(script, &root),
                    count,
                    truncated: false,
                    cancelled: false,
                }))
            }
            Ask::Cancel { .. } => Ok(()),
            Ask::Changes { .. } => say(Answer::TooOld {}),
            Ask::Exclude { add, .. } => say(Answer::Excluded { folders: add }),
            Ask::Watch { add, .. } => say(Answer::Watching { folders: add }),
        }
    }

    fn serve(stream: UnixStream, script: &Script) {
        let Ok(mut out) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(stream);
        while let Ok(Some(Request { id, ask })) = wire::read::<_, Request>(&mut reader) {
            if script.silent && !matches!(ask, Ask::Hello(_)) {
                continue;
            }
            if answer(script, ask, &mut out, id).is_err() {
                return;
            }
        }
    }

    impl Fake {
        /// A fake engine on `socket`, answering as `script` says.
        pub fn start(socket: PathBuf, script: Script) -> std::io::Result<Fake> {
            let _ = std::fs::remove_file(&socket);
            let listener = UnixListener::bind(&socket)?;
            let stop = Arc::new(AtomicBool::new(false));
            let thread = {
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    for stream in listener.incoming() {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        if let Ok(stream) = stream {
                            let script = script.clone();
                            std::thread::spawn(move || serve(stream, &script));
                        }
                    }
                })
            };
            Ok(Fake {
                socket,
                stop,
                thread: Some(thread),
            })
        }
    }

    use std::path::PathBuf;

    impl Drop for Fake {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            // Wake the listener so that it sees it is to stop.
            let _ = UnixStream::connect(&self.socket);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
            let _ = std::fs::remove_file(&self.socket);
        }
    }
}

#[cfg(not(unix))]
impl Fake {
    pub fn start(_: PathBuf, _: Script) -> std::io::Result<Fake> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "skwird's client speaks to engines on Unix sockets so far",
        ))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{Client, Find};
    use skwird_protocol::{Id, Instant, Kind, Match, NameTerm, Proof, Query, Why, WirePath};

    /// A client asks the fake engine and gets what it was given: the
    /// coverage of the longest root, entries below the root asked.
    #[test]
    fn a_client_asks_a_fake_engine() {
        // Under the user's cache, never `/tmp` (AGENTS.md, "Testing").
        let dir = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap()
            .join("skwird-tests")
            .join(format!("fake-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let checked = Coverage {
            names: Proof::Checked(Instant { sec: 5, nsec: 0 }),
            attrs: Proof::AsOf(Instant { sec: 1, nsec: 0 }),
            holes: Vec::new(),
        };
        let entry = |p: &str| Entry {
            path: WirePath(p.as_bytes().to_vec()),
            kind: Kind::File,
            id: Id::default(),
            folder: Id::default(),
            attrs: None,
        };
        let script = Script {
            roots: vec![PathBuf::from("/home/user")],
            coverage: vec![(PathBuf::from("/home/user"), checked.clone())],
            entries: vec![
                entry("/home/user/Report.pdf"),
                entry("/home/user/notes.txt"),
            ],
            silent: false,
        };
        let fake = Fake::start(dir.join("socket"), script).unwrap();
        let mut c = Client::connect(&Find {
            socket: Some(fake.socket.clone()),
            start: None,
            ..Find::new("a test")
        })
        .unwrap();
        assert_eq!(c.welcome().roots, vec![WirePath(b"/home/user".to_vec())]);
        assert_eq!(
            c.coverage(std::path::Path::new("/home/user/a")).unwrap(),
            checked
        );
        assert_eq!(
            c.coverage(std::path::Path::new("/srv")).unwrap().names,
            Proof::NotCovered(Why::Outside)
        );
        let mut found = Vec::new();
        let done = c
            .query(
                &Query {
                    root: WirePath(b"/home/user".to_vec()),
                    name: Some(NameTerm {
                        how: Match::Skwir,
                        text: "report".into(),
                        case_sensitive: false,
                    }),
                    kinds: None,
                    size: None,
                    mtime: None,
                    limit: None,
                },
                |batch| {
                    found.extend_from_slice(batch);
                    true
                },
            )
            .unwrap();
        assert_eq!(done.count, 1);
        assert_eq!(found, vec![entry("/home/user/Report.pdf")]);
        assert!(
            c.changes(std::path::Path::new("/home/user"), "x.0")
                .unwrap()
                .is_none()
        );
        drop(fake);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
