//! The remote plugin's `kmtp` connector with this helper, against a
//! stand-in kmtpd on a bus of the test's own (ADR-097): a phone kmtpd holds
//! is offered among the devices by its name, opened at the address GVfs
//! would give it into the helper's mount (the same place, with Eject,
//! thumbnails of small files, copies written in place), let go of by
//! Eject; kmtpd never loaded off KDE's desktop, loaded on it. Here because
//! the helper's build is known here alone. Skipped, saying why, without
//! FUSE or `dbus-daemon`.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{Bus, Phone, Unloaded, fixture, fuse, until};
use skwir_core::app::{App, AppConfig};
use skwir_core::cancel::CancelToken;
use skwir_core::command::Invocation;
use skwir_core::registry::Place;
use skwir_plugin_remote::address::Address;
use skwir_plugin_remote::connector::{Ask, Connector};
use skwir_plugin_remote::kmtp::Kmtp;
use skwir_plugin_remote::{RemotePlugin, Remotes};

const WORKER: &str = env!("CARGO_BIN_EXE_skwir-kmtp-worker");
const UDI: &str = "/org/kde/solid/udev/sys/devices/pci0000:00/usb9/9-1";

/// Answers nothing: what is tested asks nothing.
struct NoOne;

impl Ask for NoOne {
    fn secret(&self, _: &str, _: &str, _: &CancelToken) -> Option<String> {
        None
    }
    fn confirm(&self, _: &str, _: &str, _: &CancelToken) -> bool {
        false
    }
}

/// A system of the test's own where the phone `UDI` names is, its serial
/// the one GVfs names it by.
fn system(base: &Path) -> PathBuf {
    let system = base.join("system");
    let folder = system.join("sys/devices/pci0000:00/usb9/9-1");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("dev"), "189:1037\n").unwrap();
    std::fs::write(folder.join("busnum"), "9\n").unwrap();
    std::fs::write(folder.join("devnum"), "14\n").unwrap();
    let udev = system.join("run/udev/data");
    std::fs::create_dir_all(&udev).unwrap();
    std::fs::write(
        udev.join("c189:1037"),
        "E:ID_MTP_DEVICE=1\nE:ID_SERIAL=Example_Phone_0123\n",
    )
    .unwrap();
    system
}

fn kmtp(bus: &Bus, system: PathBuf, on_kde: bool) -> Kmtp {
    Kmtp {
        env: vec![(
            "DBUS_SESSION_BUS_ADDRESS".into(),
            bus.address.clone().into(),
        )],
        helper: Some(PathBuf::from(WORKER)),
        on_kde: Some(on_kde),
        system: Some(system),
    }
}

fn phone(app: &App) -> Option<Place> {
    app.registry
        .places
        .all()
        .into_iter()
        .find(|p| p.group == "devices" && p.title == "Example Phone")
}

fn wait(app: &App, what: &str, mut done: impl FnMut() -> bool) {
    until(what, || {
        app.pump();
        done()
    });
}

#[test]
fn a_phone_kmtpd_holds_opens_through_it_and_is_let_go_of() {
    if !fuse() {
        eprintln!("skipped: no FUSE here");
        return;
    }
    let Some(bus) = Bus::start() else {
        eprintln!("skipped: dbus-daemon is not installed");
        return;
    };
    let base = fixture("connector");
    let held = Phone::serve(
        &bus.address,
        UDI,
        "Example Phone",
        &[("/DCIM/", b""), ("/DCIM/It's 1.jpg", b"a picture")],
    );
    let connector: Arc<dyn Connector> = Arc::new(kmtp(&bus, system(&base), false));
    let remotes = Remotes::with(vec![connector]);
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.plugins
        .add(Arc::new(RemotePlugin::with(remotes.clone())), &app);

    // Offered by kmtpd, at GVfs's address for it, by its name.
    wait(&app, "the phone offered", || {
        phone(&app).is_some_and(|p| &*p.location.scheme == "mtp")
    });
    let offered = phone(&app).unwrap();
    assert_eq!(offered.location.to_string(), "mtp://Example_Phone_0123/");
    assert_eq!(offered.icon.as_deref(), Some("phone"));
    assert!(offered.unavailable.is_some());

    // Opened through the helper: the same place, mounted.
    let address = Address::parse("mtp://Example_Phone_0123/").unwrap();
    let ask: Arc<dyn Ask> = Arc::new(NoOne);
    let top = remotes
        .reach(&app, &address, &ask, &CancelToken::new())
        .expect("the phone opens");
    let picture = top.join("Internal storage/DCIM/It's 1.jpg");
    assert_eq!(std::fs::read(&picture).unwrap(), b"a picture");
    wait(&app, "the phone shown mounted", || {
        phone(&app).is_some_and(|p| p.location.path().as_deref() == Some(top.as_path()))
    });
    let mounted = phone(&app).unwrap();
    assert_eq!(mounted.key, offered.key, "the same place mounted or not");
    assert!(mounted.eject.is_some() && mounted.unavailable.is_none());
    let remote = skwir_core::fs::reach::of(&top).expect("known as remote");
    assert_eq!(remote.fstype, "fuse.skwir-kmtp");
    assert!(
        skwir_core::fs::reach::writes_in_place(&top),
        "copies written in place (ADR-083)"
    );
    let known = app
        .registry
        .remotes
        .all()
        .into_iter()
        .find(|k| k.point == top)
        .expect("what is known of it");
    assert_eq!(known.label, "Example Phone");
    assert_eq!(
        known.thumbnails,
        skwir_core::fs::reach::Thumbnails::Small,
        "near, but read whole: small files' alone"
    );

    // Eject: Skwir's mount let go of, the phone still KDE's, offered again.
    app.dispatch(&Invocation::with_args(
        "remote.disconnect",
        &serde_json::json!({ "path": top }),
    ))
    .unwrap();
    wait(&app, "let go of, offered again", || {
        !common::mounted(&top) && phone(&app).is_some_and(|p| &*p.location.scheme == "mtp")
    });
    assert!(
        held.file("/DCIM/It's 1.jpg").is_some(),
        "the phone untouched"
    );
    drop(app);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn kmtpd_is_never_loaded_off_kde_s_desktop_and_loaded_on_it() {
    let Some(bus) = Bus::start() else {
        eprintln!("skipped: dbus-daemon is not installed");
        return;
    };
    let base = fixture("unloaded");
    let unloaded = Unloaded::serve(&bus.address);
    let address = Address::parse("mtp://Example_Phone_0123/").unwrap();

    // Off KDE's desktop: asked what `/modules` holds, nothing addressed to
    // kmtpd, nothing offered, the address left to the next tool.
    let elsewhere = kmtp(&bus, system(&base), false);
    assert!(!elsewhere.opens(&address));
    let remotes = Remotes::with(vec![Arc::new(kmtp(&bus, system(&base), false))]);
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.plugins
        .add(Arc::new(RemotePlugin::with(remotes.clone())), &app);
    let ask: Arc<dyn Ask> = Arc::new(NoOne);
    let error = remotes
        .reach(&app, &address, &ask, &CancelToken::new())
        .unwrap_err()
        .to_string();
    assert_eq!(error, "no tool Skwir knows opens MTP addresses");
    assert!(phone(&app).is_none());
    assert!(unloaded.asked().is_empty(), "{:?}", unloaded.asked());
    drop(app);

    // On KDE's desktop: asked as Dolphin would, which loads it.
    let on_kde = kmtp(&bus, system(&base), true);
    assert!(!on_kde.opens(&address), "it held nothing once asked");
    assert!(
        !unloaded.asked().is_empty(),
        "kmtpd addressed, as `kiod6` loads it then"
    );
    std::fs::remove_dir_all(&base).unwrap();
}
