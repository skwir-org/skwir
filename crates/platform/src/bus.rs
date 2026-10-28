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

use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use futures_lite::{Stream, StreamExt};
use skwir_core::CancelToken;
use zbus::blocking::Connection;

static SESSION: Mutex<Option<Connection>> = Mutex::new(None);

/// The session bus's connection, made when first asked, kept until it
/// ends (the session's bus started again): the next asking makes another.
///
/// # Errors
/// When there is no session bus (a headless run).
pub fn session() -> zbus::Result<Connection> {
    let mut held = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(bus) = held.as_ref() {
        return Ok(bus.clone());
    }
    let bus = Connection::session()?;
    *held = Some(bus.clone());
    Ok(bus)
}

/// Let go of `bus` as the session's connection if it is the one kept: it
/// ended, and what asks next is given a connection that works.
fn let_go_of_ended(bus: &Connection) {
    let mut held = SESSION.lock().unwrap_or_else(PoisonError::into_inner);
    if held.as_ref().is_some_and(|kept| {
        kept.server_guid() == bus.server_guid() && kept.unique_name() == bus.unique_name()
    }) {
        *held = None;
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
/// goes): `ready` once every rule is listened to, before anything is told,
/// so what is read then misses nothing said after.
pub fn listen(
    bus: &Connection,
    rules: Vec<zbus::MatchRule<'static>>,
    stop: &CancelToken,
    ready: impl FnOnce(),
    mut told: impl FnMut(&zbus::Message),
) {
    // Woken when stopped, as the connection is not closed: by a thread
    // waiting for the stop (a token says it on a channel), or for this
    // listening to end, the bus gone first.
    struct Ends(CancelToken);
    impl Drop for Ends {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }
    let listening = Ends(stop.child());
    let ended = Arc::new(event_listener::Event::new());
    {
        let (ended, signal) = (Arc::clone(&ended), listening.0.subscribe());
        let waiting = std::thread::Builder::new()
            .name("skwir-bus-end".into())
            .spawn(move || {
                let _ = signal.receiver().recv();
                ended.notify(usize::MAX);
            });
        if let Err(error) = waiting {
            tracing::debug!("not followed on the bus: {error}");
            return;
        }
    }
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
            let listener = ended.listen();
            if stop.is_cancelled() {
                return;
            }
            let next = futures_lite::future::or(async { Some(merged.next().await) }, async {
                listener.await;
                None
            })
            .await;
            match next {
                // Stopped.
                None => return,
                // The bus gone: whoever asks next connects again.
                Some(None) => {
                    let_go_of_ended(bus);
                    return;
                }
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
    use std::time::{Duration, Instant};

    /// Two listeners on one connection are each told their own, and one
    /// stopped leaves the connection to the other.
    #[test]
    fn listeners_share_a_connection_and_stop_alone() {
        let runtime = tempfile::Builder::new().prefix("b").tempdir().unwrap();
        let config = runtime.path().join("bus.conf");
        std::fs::write(
            &config,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={}</listen>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
"#,
                runtime.path().display()
            ),
        )
        .unwrap();
        let Ok(mut daemon) = std::process::Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        else {
            eprintln!("skipped: dbus-daemon is not installed");
            return;
        };
        let mut address = String::new();
        std::io::BufReader::new(daemon.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let bus = zbus::blocking::connection::Builder::address(address.trim())
            .unwrap()
            .build()
            .unwrap();
        let rule = |member: &'static str| {
            zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface("org.example.Test")
                .and_then(|r| r.member(member))
                .unwrap()
                .build()
        };
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
        let until = |what: &str, done: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !done() {
                assert!(Instant::now() < deadline, "{what}");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let emit = |member: &str| {
            bus.emit_signal(
                None::<&str>,
                "/org/example",
                "org.example.Test",
                member,
                &(),
            )
            .unwrap();
        };
        // Its rules listened to from the thread: told again until heard.
        until("both listen", &|| {
            emit("A");
            emit("C");
            !first.lock().unwrap().is_empty() && !second.lock().unwrap().is_empty()
        });
        first.lock().unwrap().clear();
        second.lock().unwrap().clear();
        emit("B");
        emit("C");
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
        emit("A");
        emit("C");
        until("the second still told", &|| {
            second.lock().unwrap().contains(&"C".to_owned())
        });
        assert!(first.lock().unwrap().is_empty(), "the first stopped");
        stop_second.cancel();
        let _ = daemon.kill();
        let _ = daemon.wait();
    }
}
