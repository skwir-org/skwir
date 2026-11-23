//! An engine of a test's making, on a socket of the test's own: for a
//! client's tests to run without skwird, and to answer what they ask it
//! to (a root covered or not, given entries).
//!
//! It answers `hello`; `coverage` and `sync` with the coverage given for
//! the longest root holding the path asked (`outside` for none); `query`
//! with the entries given below the root asked, a name matched as a
//! substring after lowercasing (globs are not its); `changes` with the
//! script's journal as it stands, or too old where it has none, each root
//! and token asked kept for the test to read ([`Fake::changes_asked`]);
//! `exclude` with what was asked; `watch` as the script says, or each
//! folder asked live since the epoch, with no share. Made `silent`, it
//! says hello and then nothing, as an engine that hangs: for a client's
//! test of giving up on a call.

use std::path::PathBuf;

use skwird_protocol::{Changes, Coverage, Entry, Watching};

/// What the fake engine answers.
#[derive(Clone, Debug, Default)]
pub struct Script {
    pub roots: Vec<PathBuf>,
    /// Each root's coverage, for the paths below it.
    pub coverage: Vec<(PathBuf, Coverage)>,
    pub entries: Vec<Entry>,
    /// Say hello, then answer nothing more.
    pub silent: bool,
    /// What every `watch` is answered, if given.
    pub watching: Option<Watching>,
    /// What every `changes` is answered, whatever its root and token: too
    /// old where none is given.
    pub changes: Option<Changes>,
}

/// A fake engine serving until dropped.
pub struct Fake {
    pub socket: PathBuf,
    /// Each `changes` asked, its root and the token it was asked from.
    #[cfg(any(unix, windows))]
    asked: std::sync::Arc<std::sync::Mutex<Vec<(PathBuf, String)>>>,
    #[cfg(any(unix, windows))]
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(any(unix, windows))]
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(any(unix, windows))]
mod imp {
    use std::io::BufReader;
    #[cfg(unix)]
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};

    use skwird_protocol::{
        self as wire, Answer, Ask, Coverage, Done, Proof, Request, Response, Welcome, Why, WirePath,
    };

    use super::{Fake, Script};
    use crate::Conn;

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

    /// Each `changes` asked: its root and token.
    type Asked = Arc<Mutex<Vec<(PathBuf, String)>>>;

    fn answer(
        script: &Script,
        asked: &Asked,
        ask: Ask,
        out: &mut Conn,
        id: u64,
    ) -> std::io::Result<()> {
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
            Ask::Changes { root, since } => {
                asked
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push((root.to_path(), since));
                say(match &script.changes {
                    Some(changes) => Answer::Changes(changes.clone()),
                    None => Answer::TooOld {},
                })
            }
            Ask::Exclude { add, .. } => say(Answer::Excluded { folders: add }),
            Ask::Watch { add, .. } => say(Answer::Watching(
                script.watching.clone().unwrap_or(wire::Watching {
                    folders: add
                        .into_iter()
                        .map(|path| wire::Kept {
                            path,
                            state: wire::Keeping::Live(wire::Instant { sec: 0, nsec: 0 }),
                            folders: None,
                            within: None,
                        })
                        .collect(),
                    share: None,
                }),
            )),
        }
    }

    fn serve(stream: Conn, script: &Script, asked: &Asked) {
        let Ok(mut out) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(stream);
        while let Ok(Some(Request { id, ask })) = wire::read::<_, Request>(&mut reader) {
            if script.silent && !matches!(ask, Ask::Hello(_)) {
                continue;
            }
            if answer(script, asked, ask, &mut out, id).is_err() {
                return;
            }
        }
    }

    impl Fake {
        /// A fake engine on `socket`, answering as `script` says.
        #[cfg(unix)]
        pub fn start(socket: PathBuf, script: Script) -> std::io::Result<Fake> {
            let _ = std::fs::remove_file(&socket);
            let listener = UnixListener::bind(&socket)?;
            let stop = Arc::new(AtomicBool::new(false));
            let asked: Asked = Arc::default();
            let thread = {
                let stop = Arc::clone(&stop);
                let asked = Arc::clone(&asked);
                std::thread::spawn(move || {
                    for stream in listener.incoming() {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        if let Ok(stream) = stream {
                            let (script, asked) = (script.clone(), Arc::clone(&asked));
                            std::thread::spawn(move || serve(stream, &script, &asked));
                        }
                    }
                })
            };
            Ok(Fake {
                socket,
                asked,
                stop,
                thread: Some(thread),
            })
        }

        /// A fake engine on the pipe `socket` names, answering as `script`
        /// says.
        #[cfg(windows)]
        pub fn start(socket: PathBuf, script: Script) -> std::io::Result<Fake> {
            let server = crate::pipe::Server::bind(socket.as_os_str())?;
            let stop = Arc::new(AtomicBool::new(false));
            let asked: Asked = Arc::default();
            let thread = {
                let stop = Arc::clone(&stop);
                let asked = Arc::clone(&asked);
                std::thread::spawn(move || {
                    while let Ok(Some(stream)) = server.accept(None) {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let (script, asked) = (script.clone(), Arc::clone(&asked));
                        std::thread::spawn(move || serve(stream, &script, &asked));
                    }
                })
            };
            Ok(Fake {
                socket,
                asked,
                stop,
                thread: Some(thread),
            })
        }
    }

    use std::path::PathBuf;

    impl Fake {
        /// Each `changes` asked so far, in order: its root and the token it
        /// was asked from, for a test to check what its client sent.
        pub fn changes_asked(&self) -> Vec<(PathBuf, String)> {
            self.asked
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            // Wake the listener so that it sees it is to stop.
            #[cfg(unix)]
            let _ = UnixStream::connect(&self.socket);
            #[cfg(windows)]
            let _ = crate::pipe::Stream::connect(
                self.socket.as_os_str(),
                Some(std::time::Duration::from_secs(1)),
            );
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
            #[cfg(unix)]
            let _ = std::fs::remove_file(&self.socket);
        }
    }
}

#[cfg(not(any(unix, windows)))]
impl Fake {
    pub fn start(_: PathBuf, _: Script) -> std::io::Result<Fake> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "skwird's client speaks to engines on Linux, macOS and Windows alone",
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
            watching: None,
            changes: None,
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
                    links: skwird_protocol::Links::Own,
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
        assert_eq!(
            fake.changes_asked(),
            vec![(PathBuf::from("/home/user"), "x.0".to_owned())],
            "kept even when too old"
        );
        drop(fake);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The journal the script gives is every `changes`' answer, as it
    /// stands, through the real client; each root and token asked is kept,
    /// in order, for the test to check what its client sent.
    #[test]
    fn the_fake_answers_the_scripts_journal_and_keeps_what_was_asked() {
        // Under the user's cache, never `/tmp` (AGENTS.md, "Testing").
        let dir = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap()
            .join("skwird-tests")
            .join(format!("fake-changes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let journal = skwird_protocol::Changes {
            events: vec![skwird_protocol::Event::Added(Entry {
                path: WirePath(b"/home/user/new.txt".to_vec()),
                kind: Kind::File,
                id: Id::default(),
                folder: Id::default(),
                attrs: None,
            })],
            token: "0000000000000001.9".into(),
            complete: skwird_protocol::Complete {
                names: true,
                attrs: true,
            },
            holes: Vec::new(),
        };
        let fake = Fake::start(
            dir.join("socket"),
            Script {
                roots: vec![PathBuf::from("/home/user")],
                changes: Some(journal.clone()),
                ..Script::default()
            },
        )
        .unwrap();
        let mut c = Client::connect(&Find {
            socket: Some(fake.socket.clone()),
            start: None,
            ..Find::new("a test")
        })
        .unwrap();
        let home = std::path::Path::new("/home/user");
        assert_eq!(c.changes(home, "").unwrap(), Some(journal.clone()));
        assert_eq!(
            c.changes(home, "0000000000000001.4").unwrap(),
            Some(journal)
        );
        assert_eq!(
            fake.changes_asked(),
            vec![
                (PathBuf::from("/home/user"), String::new()),
                (PathBuf::from("/home/user"), "0000000000000001.4".to_owned()),
            ]
        );
        drop(fake);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
