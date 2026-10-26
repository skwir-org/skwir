//! The media controls on the program's one session connection
//! (`skwir_platform::bus`): turned off, what they served there is taken
//! back (the name, the objects) and the connection stays the program's;
//! turned on again, served anew. Run again on a session bus of its own.
//! Skipped, saying why, without `dbus-daemon`.
#![cfg(target_os = "linux")]

use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::media::PlaybackStatus;
use skwir_core::playing::NowPlaying;
use skwir_plugin_mpris::MprisPlugin;

const MARK: &str = "SKWIR_TEST_MPRIS_OWN_BUS";
const PATH: &str = "/org/mpris/MediaPlayer2";

fn until(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn track() -> NowPlaying {
    NowPlaying {
        path: "/music/it's a café/01 – song.flac".into(),
        title: "A Song".into(),
        artist: "Someone".into(),
        album: String::new(),
        status: PlaybackStatus::Playing,
        position: 1.,
        at: Instant::now(),
        rate: 1.,
        duration: Some(200.),
        volume: 1.,
        can_next: false,
        can_previous: false,
        can_seek: true,
        cover: None,
        pinned: false,
    }
}

#[test]
fn turned_off_and_on_again_on_the_program_s_connection() {
    if std::env::var_os(MARK).is_none() {
        let runtime = tempfile::Builder::new().prefix("m").tempdir().unwrap();
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
        let ran = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "turned_off_and_on_again_on_the_program_s_connection",
            ])
            .env("DBUS_SESSION_BUS_ADDRESS", address.trim())
            .env(MARK, "1")
            .output()
            .unwrap();
        let _ = daemon.kill();
        let _ = daemon.wait();
        let said = String::from_utf8_lossy(&ran.stdout);
        assert!(
            ran.status.success() && said.contains("1 passed"),
            "{said}\n{}",
            String::from_utf8_lossy(&ran.stderr)
        );
        return;
    }

    let base = tempfile::tempdir().unwrap();
    let app = App::new(AppConfig::ephemeral(base.path().join("app"))).unwrap();
    app.plugins.add(Arc::new(MprisPlugin::default()), &app);
    let bus = skwir_platform::bus::session().unwrap();
    let name = format!(
        "org.mpris.MediaPlayer2.skwir.instance{}",
        std::process::id()
    );
    // Asked from another connection, as the desktop asks.
    let desktop = zbus::blocking::Connection::session().unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&desktop).unwrap();
    let named = || {
        dbus.name_has_owner(zbus::names::BusName::try_from(name.as_str()).unwrap())
            .unwrap()
    };
    let status = || -> Option<String> {
        zbus::blocking::Proxy::new(
            &desktop,
            bus.unique_name()?.to_owned(),
            PATH,
            "org.mpris.MediaPlayer2.Player",
        )
        .ok()?
        .get_property("PlaybackStatus")
        .ok()
    };

    app.player.publish(Some(track()));
    until("named while playing", named);
    assert_eq!(status().as_deref(), Some("Playing"));

    // Turned off: the name and the objects go, the connection stays.
    assert!(app.plugins.set_enabled("mpris", false, &app));
    until("the name let go of", || !named());
    until("the objects taken back", || status().is_none());
    assert!(
        zbus::blocking::fdo::DBusProxy::new(&bus).is_ok_and(|own| own.get_id().is_ok()),
        "the program's connection still answers"
    );

    // Turned on again while it plays: served anew on the same connection,
    // named at once.
    assert!(app.plugins.set_enabled("mpris", true, &app));
    until("named again", named);
    assert_eq!(status().as_deref(), Some("Playing"));
    app.player.publish(None);
    until("let go of once nothing plays", || !named());
}
