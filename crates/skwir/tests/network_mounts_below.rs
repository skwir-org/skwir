//! Walks never enter a network mount below their root (ADR-077, remote.md
//! "What differs on a network"): a tag search's index hints, a tag
//! location's count and listing, and the timeline's subtree walk leave
//! what is mounted below the folder they walk alone: they ask nothing
//! there and list nothing of it, though a pane shown there taught the
//! tags index its files. Jump keeps a recent file there unread; the
//! recent files' listing reads it in its server's lane. Against an rclone
//! mount of a folder of the test's (`fuse.rclone`: a network's, to
//! `fs::reach`), whose program logs each request it is asked. Skipped
//! where rclone or FUSE is missing.
// Linux's mount table, and tags in extended attributes.
#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use skwir_core::app::{App, AppConfig};
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::fs::reach;
use skwir_core::hints::HintProvider;
use skwir_core::location::{ListingEvent, Location};
use skwir_core::os_path::PathArg;

/// A folder of the test's on the home's file system, never `/tmp`.
fn fixture(name: &str) -> PathBuf {
    let base = dirs::home_dir()
        .unwrap()
        .join(".cache/skwir-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

/// `served` mounted at `point` by rclone, which logs every request: let go
/// of when dropped, before its program stops.
struct Mount {
    child: Child,
    point: PathBuf,
    log: PathBuf,
}

impl Mount {
    fn start(served: &Path, point: &Path, base: &Path) -> Option<Mount> {
        if skwir_core::os_path::find_program("rclone").is_none()
            || skwir_core::os_path::find_program("fusermount3").is_none()
        {
            return None;
        }
        let log = base.join("rclone.log");
        let child = Command::new("rclone")
            .arg("mount")
            .arg(served)
            .arg(point)
            .arg("--config")
            .arg("/dev/null")
            .arg("--cache-dir")
            .arg(base.join("rclone-cache"))
            // Every stat reaches it, never the kernel's memory of one.
            .args(["--attr-timeout", "0", "-vv", "--log-file"])
            .arg(&log)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mount = Mount {
            child,
            point: point.to_path_buf(),
            log,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        while !reach::is_point(point) {
            if Instant::now() > deadline {
                eprintln!("skipped: rclone did not mount");
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(mount)
    }

    /// How much it logged so far.
    fn logged(&self) -> usize {
        std::fs::read(&self.log).map_or(0, |l| l.len())
    }

    /// What it logged since `from`.
    fn asked_since(&self, from: usize) -> String {
        let log = std::fs::read(&self.log).unwrap_or_default();
        String::from_utf8_lossy(log.get(from..).unwrap_or_default()).into_owned()
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        let _ = Command::new("fusermount3")
            .arg("-uz")
            .arg(&self.point)
            .status();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Names files under the mount, as an index that learned them would.
struct Planted(Vec<PathBuf>);

impl HintProvider for Planted {
    fn id(&self) -> &'static str {
        "planted"
    }
    fn candidates(&self, _: &str, _: &str, _: &Path, _: &CancelToken) -> Option<Vec<PathBuf>> {
        Some(self.0.clone())
    }
}

/// Until `ready` holds, pumping the app, for at most `seconds`.
fn until(app: &App, seconds: u64, what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        app.pump();
        if ready() {
            return;
        }
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The files a search of `root` found, by their paths under it.
fn search(app: &App, expr: &str, root: &Path) -> BTreeSet<String> {
    let location = skwir_plugin_query::location_for(expr, Some(root));
    let open = app.open_location(&location).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut found = BTreeSet::new();
    loop {
        match open
            .events()
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the search completes")
        {
            ListingEvent::Added(batch) => {
                for e in batch {
                    let path = open.path_of(e.id).unwrap();
                    found.insert(path.strip_prefix(root).unwrap().display().to_string());
                }
            }
            ListingEvent::Complete => break,
            ListingEvent::Error(e) => panic!("{e}"),
            _ => {}
        }
    }
    open.close();
    found
}

/// An XMP sidecar giving its file `tag`.
fn sidecar(tag: &str) -> String {
    format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:subject><rdf:Bag><rdf:li>{tag}</rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#
    )
}

#[test]
fn walks_below_a_folder_leave_the_network_mounts_in_it_alone() {
    let base = fixture("mounts-below");
    let root = base.join("it's home");
    let served = base.join("served");
    let point = root.join("nas ü");
    for dir in [&root, &point, &served.join("sub")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(root.join("a.txt"), "a").unwrap();
    if skwir_plugin_tags::write_tags(&root.join("a.txt"), &["Work".to_owned()]).is_err() {
        eprintln!("skipped: no user extended attributes here");
        return;
    }
    std::fs::write(served.join("far b.txt"), "b").unwrap();
    std::fs::write(served.join("far b.txt.xmp"), sidecar("Work")).unwrap();
    std::fs::write(served.join("sub/far c.txt"), "c").unwrap();
    let Some(mount) = Mount::start(&served, &point, &base) else {
        eprintln!("skipped: rclone or FUSE is missing");
        return;
    };
    let app = App::new(AppConfig::ephemeral(base.join("app"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
    app.plugins.add(Arc::new(skwir_plugin_xmp::XmpPlugin), &app);
    app.plugins.add(
        Arc::new(skwir_plugin_timeline::TimelinePlugin::default()),
        &app,
    );
    app.plugins
        .add(Arc::new(skwir_plugin_jump::JumpPlugin::default()), &app);
    let pane = app.workspace.read().active_id();
    // The desktop's recent files name one there.
    let recent = format!(
        r#"<?xml version="1.0"?><xbel version="1.0"><bookmark href="{}" visited="2026-01-01T00:00:00Z"/><bookmark href="{}" visited="2026-01-02T00:00:00Z"/></xbel>"#,
        skwir_platform::uri::file_uri(&root.join("a.txt")),
        skwir_platform::uri::file_uri(&point.join("far b.txt")),
    );
    let data = skwir_platform::xdg::data_home().unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("recently-used.xbel"), recent).unwrap();

    // A pane shown there teaches the tags index the file tagged there.
    app.navigate(pane, &Location::from_path(&point)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
    {
        // Its rows read, as a view asks for those it shows.
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        let ids: Vec<_> = p.listing.ids().collect();
        p.open
            .as_ref()
            .unwrap()
            .hydrate(&ids, skwir_core::scheduler::Priority::Visible);
    }
    let far = point.join("far b.txt");
    until(&app, 20, "the index learned the file there", || {
        app.hints
            .candidates(
                skwir_core::entry::keys::TAGS,
                "work",
                &point,
                &CancelToken::new(),
            )
            .is_some_and(|known| known.contains(&far))
    });
    // And another index names them too.
    app.hints.register(
        "test",
        Arc::new(Planted(vec![far.clone(), point.join("sub/far c.txt")])),
    );
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
    let from = mount.logged();

    // A search of the folder above.
    assert_eq!(
        search(&app, "tag:work", &root),
        BTreeSet::from(["a.txt".to_owned()])
    );

    // Its tag location: counted, then listed.
    let tags = skwir_plugin_tags::location::Address {
        tag: Some("Work".to_owned()),
        scope: Some(root.clone()),
        ..skwir_plugin_tags::location::Address::default()
    }
    .location();
    app.navigate(pane, &tags).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
    let counted = || {
        let Outcome::Value(v) = app
            .dispatch(&Invocation::with_args(
                "tags.counted",
                &serde_json::json!({ "root": PathArg::from(root.as_path()) }),
            ))
            .unwrap()
        else {
            panic!("tags.counted answers a value");
        };
        v["counting"] == false && v["counted"].is_u64()
    };
    until(&app, 20, "the tags of the folder counted", counted);
    let files: BTreeSet<String> = {
        let ws = app.workspace.read();
        let p = ws.pane(pane).unwrap();
        p.listing
            .iter()
            .filter(|e| !e.is_place())
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(files, BTreeSet::from(["a.txt".to_owned()]));

    // The timeline of everything below the folder.
    app.navigate(pane, &Location::from_path(&root)).unwrap();
    assert!(app.pump_until_complete(pane, Duration::from_secs(20)));
    app.dispatch(&Invocation::with_args(
        "timeline.histogram",
        &serde_json::json!({ "pane": pane, "subtree": true }),
    ))
    .unwrap();
    until(&app, 20, "the subtree walked", || {
        let Ok(Outcome::Value(v)) = app.dispatch(&Invocation::with_args(
            "timeline.status",
            &serde_json::json!({ "pane": pane }),
        )) else {
            return false;
        };
        !v["histogram"].is_null()
    });

    // Jump ranks the recent file there, kept unread.
    let Outcome::Value(ranked) = app
        .dispatch(&Invocation::with_args(
            "jump.candidates",
            &serde_json::json!({ "query": "far" }),
        ))
        .unwrap()
    else {
        panic!("jump.candidates answers a value");
    };
    assert!(
        ranked
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["path"] == serde_json::json!(PathArg::from(far.as_path()))),
        "{ranked}"
    );

    // Nothing of what is mounted there was asked for: no name in it looked
    // up, no folder of it listed.
    let asked = mount.asked_since(from);
    for what in [
        "far b.txt",
        "far c.txt",
        "name=\"sub\"",
        "sub: ",
        "ReadDirAll",
    ] {
        assert!(
            !asked.contains(what),
            "“{what}” asked of the mount:\n{asked}"
        );
    }
    // The recent files are listed, that one in its server's lane: and the
    // log does tell a read there.
    let from = mount.logged();
    app.navigate(pane, &Location::parse("recent:")).unwrap();
    until(&app, 20, "the recent files listed", || {
        let ws = app.workspace.read();
        let names: BTreeSet<String> = ws
            .pane(pane)
            .unwrap()
            .listing
            .iter()
            .map(|e| e.name.to_string_lossy().into_owned())
            .collect();
        names == BTreeSet::from(["a.txt".to_owned(), "far b.txt".to_owned()])
    });
    assert!(mount.asked_since(from).contains("far b.txt"));
    drop(app);
    drop(mount);
    std::fs::remove_dir_all(&base).unwrap();
}
