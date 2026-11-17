//! KDE told its network folders changed, on a bus of the test's own, as
//! its own Add Network Folder (`knetattach`) tells it: `KDirNotify`'s
//! `FilesAdded` of `remote:/`, which an open Dolphin showing Network
//! lists again on. Linux only, as KIO's bus is. Skipped, saying why,
//! without `dbus-daemon`.
#![cfg(target_os = "linux")]
// Tests run from a console: no console window to keep away.
#![allow(clippy::disallowed_methods)]

use std::io::BufRead;
use std::process::{Command, Stdio};
use std::time::Duration;

/// The signal says `remote:/` was added to, from `/`, on KIO's interface:
/// what KIO's `KDirNotify::emitFilesAdded` sends.
#[test]
fn kde_is_told_as_its_own_add_network_folder_tells_it() {
    let runtime = tempfile::Builder::new().prefix("k").tempdir().unwrap();
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
    let Ok(mut daemon) = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .args(["--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        eprintln!("skipped: dbus-daemon is not installed");
        return;
    };
    let mut address = String::new();
    std::io::BufReader::new(daemon.stdout.take().unwrap())
        .read_line(&mut address)
        .unwrap();
    let connect = || {
        zbus::blocking::connection::Builder::address(address.trim())
            .unwrap()
            .build()
            .unwrap()
    };
    let (listener, teller) = (connect(), connect());
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.kde.KDirNotify")
        .and_then(|rule| rule.member("FilesAdded"))
        .unwrap()
        .build();
    let messages = zbus::blocking::MessageIterator::for_match_rule(rule, &listener, None).unwrap();
    let (tx, rx) = crossbeam_channel::bounded(1);
    std::thread::spawn(move || {
        if let Some(Ok(message)) = messages.into_iter().next() {
            let _ = tx.send(message);
        }
    });

    skwir_plugin_remote::servers::tell_kde_on(&teller).unwrap();

    let told = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("KDE was told");
    let header = told.header();
    assert_eq!(
        header.path().map(zbus::zvariant::ObjectPath::as_str),
        Some("/")
    );
    assert_eq!(header.destination(), None, "said to every program");
    let (url,): (String,) = told.body().deserialize().unwrap();
    assert_eq!(url, "remote:/");
    let _ = daemon.kill();
    let _ = daemon.wait();
}
