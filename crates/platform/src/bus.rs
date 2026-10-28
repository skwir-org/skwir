//! The session bus, one connection for the whole program (Linux): the
//! desktop's style followed, the media controls served, the devices GVfs
//! and KDE's kmtpd offer followed. Each zbus connection keeps a thread and
//! buffers of its own for as long as it lives; these share one.
//!
//! A shared connection is never closed by one of its users, and it waits
//! for its slowest listener: zbus queues what a stream is told and drops
//! nothing, so a stream left full holds every message behind it, for every
//! user. A listener takes each message at once and hands what may wait (a
//! question to a program that answers slowly) to a thread of its own
//! ([`follow`]).
//!
//! The bus may end while Skwir runs (the session's bus started again):
//! what follows or serves there for the program's life does it through
//! [`keep`], which gives it the next connection once the bus is back, and
//! it listens and serves again there.

use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures_lite::{Stream, StreamExt};
use skwir_core::CancelToken;
use zbus::blocking::Connection;

static SESSION: Mutex<Option<Connection>> = Mutex::new(None);

/// How long [`keep`] waits to connect again once a connection that worked
/// ended, after trying at once: the first, then twice as long each time
/// it fails, up to the last. Only its own thread wakes meanwhile.
const AGAIN: (Duration, Duration) = (Duration::from_secs(1), Duration::from_secs(60));

/// The session bus's connection, made when first asked, kept until it
/// ends (the session's bus started again): the next asking makes another.
///
/// # Errors
/// When there is no session bus (a headless run).
pub fn session() -> zbus::Result<Connection> {
    let mut held = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(bus) = held.as_ref().filter(|bus| !bus.inner().is_closed()) {
        return Ok(bus.clone());
    }
    let bus = Connection::session()?;
    *held = Some(bus.clone());
    Ok(bus)
}

/// Give `connected` the connection `connect` makes (the session's:
/// [`session`]), on this thread, then a new one each time it ends (the
/// session's bus started again), until `stop`. `connected` may hold the
/// thread while it uses the connection (a [`listen`], which ends with
/// it), or return at once, what it serves there left served: it lists,
/// listens and serves anew on each connection, as what it did on the last
/// is gone with it. A connection that ended is made again at once, then
/// after a second, twice as long each time it fails, up to a minute; this
/// thread alone wakes meanwhile. When the first cannot be made there is
/// no bus (a headless run): it returns.
pub fn keep<E: std::fmt::Display>(
    mut connect: impl FnMut() -> Result<Connection, E>,
    stop: &CancelToken,
    mut connected: impl FnMut(&Connection),
) {
    let mut bus = match connect() {
        Ok(bus) => bus,
        Err(error) => {
            tracing::debug!("no session bus: {error}");
            return;
        }
    };
    loop {
        if stop.is_cancelled() {
            return;
        }
        connected(&bus);
        if !ends(&bus, stop) {
            return;
        }
        tracing::debug!("the session bus ended: connecting again once it is back");
        let stopped = stop.subscribe();
        let mut wait = Duration::ZERO;
        bus = loop {
            if stopped.receiver().recv_timeout(wait).is_ok() {
                return;
            }
            match connect() {
                Ok(bus) => break bus,
                Err(error) => {
                    tracing::trace!(?wait, "the session bus is not back: {error}");
                    wait = (wait * 2).clamp(AGAIN.0, AGAIN.1);
                }
            }
        };
        tracing::debug!("the session bus is back");
    }
}

/// Wait until `bus` ends (true), or `stop` (false).
fn ends(bus: &Connection, stop: &CancelToken) -> bool {
    if bus.inner().is_closed() {
        return true;
    }
    let stopped = match Stopped::new(stop) {
        Ok(stopped) => stopped,
        Err(error) => {
            tracing::debug!("not followed on the bus: {error}");
            return false;
        }
    };
    zbus::block_on(futures_lite::future::or(
        async {
            bus.inner().closed().await;
            true
        },
        async {
            stopped.wait().await;
            false
        },
    ))
}

/// Woken once `stop` is, for what waits on the bus meanwhile, as the
/// connection is not closed: a thread waits for the stop (a token says it
/// on a channel) and wakes it. Ends with this, stopped or not.
struct Stopped {
    token: CancelToken,
    woken: Arc<event_listener::Event>,
}

impl Stopped {
    fn new(stop: &CancelToken) -> std::io::Result<Stopped> {
        let (token, woken) = (stop.child(), Arc::new(event_listener::Event::new()));
        let (wakes, signal) = (Arc::clone(&woken), token.subscribe());
        std::thread::Builder::new()
            .name("skwir-bus-end".into())
            .spawn(move || {
                let _ = signal.receiver().recv();
                wakes.notify(usize::MAX);
            })?;
        Ok(Stopped { token, woken })
    }

    /// Once stopped, or this ended.
    async fn wait(&self) {
        loop {
            let listener = self.woken.listen();
            if self.token.is_cancelled() {
                return;
            }
            listener.await;
        }
    }
}

impl Drop for Stopped {
    fn drop(&mut self) {
        self.token.cancel();
    }
}

/// Call `told` with each signal on `bus` that one of `rules` matches, on a
/// thread named `name`, until `stop` (or the bus goes): never closing
/// `bus`, which others may share. `told` takes each message at once, as a
/// listener that stops reading holds every listener of the connection.
/// The thread is returned, which ends with the stop.
///
/// # Errors
/// When the thread could not start.
pub fn follow(
    bus: &Connection,
    rules: Vec<zbus::MatchRule<'static>>,
    name: &str,
    stop: &CancelToken,
    told: impl FnMut(&zbus::Message) + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    let (bus, stop) = (bus.clone(), stop.clone());
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || listen(&bus, rules, &stop, || {}, told))
}

/// [`follow`] on this thread, which it holds until `stop` (or the bus
/// goes, after which [`keep`] gives the next): `ready` once every rule is
/// listened to, before anything is told, so what is read then misses
/// nothing said after.
pub fn listen(
    bus: &Connection,
    rules: Vec<zbus::MatchRule<'static>>,
    stop: &CancelToken,
    ready: impl FnOnce(),
    mut told: impl FnMut(&zbus::Message),
) {
    let stopped = match Stopped::new(stop) {
        Ok(stopped) => stopped,
        Err(error) => {
            tracing::debug!("not followed on the bus: {error}");
            return;
        }
    };
    zbus::block_on(async move {
        type Told = Pin<Box<dyn Stream<Item = zbus::Result<zbus::Message>> + Send>>;
        let mut merged: Option<Told> = None;
        for rule in rules {
            let stream = match zbus::MessageStream::for_match_rule(rule, bus.inner(), None).await {
                Ok(stream) => stream,
                Err(error) => {
                    tracing::debug!("not followed on the bus: {error}");
                    return;
                }
            };
            merged = Some(match merged.take() {
                Some(before) => Box::pin(before.or(stream)),
                None => Box::pin(stream),
            });
        }
        let Some(mut merged) = merged else { return };
        ready();
        loop {
            if stop.is_cancelled() {
                return;
            }
            let next = futures_lite::future::or(async { Some(merged.next().await) }, async {
                stopped.wait().await;
                None
            })
            .await;
            match next {
                // Stopped, or the bus gone: whoever asks next connects
                // again (`session`).
                None | Some(None) => return,
                Some(Some(Ok(message))) => told(&message),
                Some(Some(Err(_))) => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    /// A bus daemon of the test's own in `runtime`, listening at `listen`
    /// (`unix:dir=` for any socket, `unix:path=` for one started again at
    /// the same address): it and the address it prints. `None` without
    /// `dbus-daemon`.
    fn daemon(runtime: &std::path::Path, listen: &str) -> Option<(std::process::Child, String)> {
        let config = runtime.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>{listen}</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#
            ),
        )
        .unwrap();
        let mut daemon = std::process::Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let mut address = String::new();
        std::io::BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        Some((daemon, address.trim().to_owned()))
    }

    fn rule(member: &'static str) -> zbus::MatchRule<'static> {
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.example.Test")
            .and_then(|r| r.member(member))
            .unwrap()
            .build()
    }

    fn until(what: &str, done: &dyn Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn emit(bus: &Connection, member: &str) {
        bus.emit_signal(
            None::<&str>,
            "/org/example",
            "org.example.Test",
            member,
            &(),
        )
        .unwrap();
    }

    /// Two listeners on one connection are each told their own, and one
    /// stopped leaves the connection to the other.
    #[test]
    fn listeners_share_a_connection_and_stop_alone() {
        let runtime = tempfile::Builder::new().prefix("b").tempdir().unwrap();
        let listen = format!("unix:dir={}", runtime.path().display());
        let Some((mut daemon, address)) = daemon(runtime.path(), &listen) else {
            eprintln!("skipped: dbus-daemon is not installed");
            return;
        };
        let bus = zbus::blocking::connection::Builder::address(address.as_str())
            .unwrap()
            .build()
            .unwrap();
        let (first, second) = (
            Arc::new(Mutex::new(Vec::<String>::new())),
            Arc::new(Mutex::new(Vec::<String>::new())),
        );
        let note = |into: &Arc<Mutex<Vec<String>>>| {
            let into = Arc::clone(into);
            move |message: &zbus::Message| {
                let member = message.header().member().map(ToString::to_string);
                into.lock().unwrap().extend(member);
            }
        };
        let (stop_first, stop_second) = (CancelToken::new(), CancelToken::new());
        let following = follow(
            &bus,
            vec![rule("A"), rule("B")],
            "test-first",
            &stop_first,
            note(&first),
        )
        .unwrap();
        follow(
            &bus,
            vec![rule("C")],
            "test-second",
            &stop_second,
            note(&second),
        )
        .unwrap();
        // Its rules listened to from the thread: told again until heard.
        until("both listen", &|| {
            emit(&bus, "A");
            emit(&bus, "C");
            !first.lock().unwrap().is_empty() && !second.lock().unwrap().is_empty()
        });
        first.lock().unwrap().clear();
        second.lock().unwrap().clear();
        emit(&bus, "B");
        emit(&bus, "C");
        until("each told its own", &|| {
            first.lock().unwrap().contains(&"B".to_owned())
                && second.lock().unwrap().contains(&"C".to_owned())
        });
        assert!(!first.lock().unwrap().contains(&"C".to_owned()));
        assert!(!second.lock().unwrap().contains(&"B".to_owned()));
        // The first stopped: the second still told, the connection open.
        stop_first.cancel();
        following.join().unwrap();
        first.lock().unwrap().clear();
        second.lock().unwrap().clear();
        emit(&bus, "A");
        emit(&bus, "C");
        until("the second still told", &|| {
            second.lock().unwrap().contains(&"C".to_owned())
        });
        assert!(first.lock().unwrap().is_empty(), "the first stopped");
        stop_second.cancel();
        let _ = daemon.kill();
        let _ = daemon.wait();
    }

    /// A listener kept (`keep`) listens again once its bus is back: the
    /// daemon stopped, then started again at the same address, it is ready
    /// once more and told what is said on the new one, once, until stopped.
    #[test]
    fn a_kept_listener_listens_again_once_the_bus_is_back() {
        let runtime = tempfile::Builder::new().prefix("b").tempdir().unwrap();
        // Named without the daemon's guid, which the next has another of.
        let address = format!("unix:path={}", runtime.path().join("bus").display());
        let Some((mut first, _)) = daemon(runtime.path(), &address) else {
            eprintln!("skipped: dbus-daemon is not installed");
            return;
        };
        let connect = {
            let address = address.clone();
            move || zbus::blocking::connection::Builder::address(address.as_str())?.build()
        };
        let (readied, told) = (
            Arc::new(AtomicUsize::new(0)),
            Arc::new(Mutex::new(Vec::<String>::new())),
        );
        let stop = CancelToken::new();
        let keeping = {
            let (readied, told, stop, connect) = (
                Arc::clone(&readied),
                Arc::clone(&told),
                stop.clone(),
                connect.clone(),
            );
            std::thread::spawn(move || {
                keep(connect, &stop, |bus| {
                    listen(
                        bus,
                        vec![rule("A")],
                        &stop,
                        || {
                            readied.fetch_add(1, Ordering::SeqCst);
                        },
                        |message| {
                            let member = message.header().member().map(ToString::to_string);
                            told.lock().unwrap().extend(member);
                        },
                    );
                });
            })
        };
        let heard = |on: &Connection| {
            told.lock().unwrap().clear();
            until("told on the bus", &|| {
                emit(on, "A");
                !told.lock().unwrap().is_empty()
            });
        };
        until("ready", &|| readied.load(Ordering::SeqCst) == 1);
        heard(&connect().unwrap());

        // The bus gone and started again: ready once more, told there.
        let _ = first.kill();
        let _ = first.wait();
        let (mut second, _) = daemon(runtime.path(), &address).unwrap();
        until("ready again", &|| readied.load(Ordering::SeqCst) == 2);
        heard(&connect().unwrap());
        assert_eq!(readied.load(Ordering::SeqCst), 2, "connected again once");

        stop.cancel();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !keeping.is_finished() {
            assert!(Instant::now() < deadline, "stopped");
            std::thread::sleep(Duration::from_millis(10));
        }
        keeping.join().unwrap();
        let _ = second.kill();
        let _ = second.wait();
    }
}
