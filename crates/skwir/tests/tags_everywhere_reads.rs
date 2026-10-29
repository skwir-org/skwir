//! A tag renamed everywhere takes each file's tags from the search that
//! found it: the tags plugin reads a file again only where the search did
//! not say. Counted by a notes store of the test's own, which every read
//! of a file's notes asks.
// Tags in extended attributes: Unix's.
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use skwir_core::app::{App, AppConfig};
use skwir_core::command::Invocation;
use skwir_core::metadata::Listed;
use skwir_core::notes::raw::RawKey;
use skwir_core::notes::{Holds, Kept, Notes, NotesStore, Probe, Write, Wrote};
use skwir_core::op::Plan;
use skwir_core::os_path::PathArg;
use skwir_plugin_tags::{read_tags, write_tags};

/// A folder where extended attributes work: not `/tmp`, often a tmpfs.
fn home_tempdir() -> tempfile::TempDir {
    let base = dirs::home_dir().unwrap().join(".cache").join("skwir-tests");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::Builder::new()
        .prefix("tags-everywhere-reads-")
        .tempdir_in(base)
        .unwrap()
}

/// A store holding nothing, which counts how often each file's notes are
/// read.
#[derive(Default)]
struct Counting(Mutex<HashMap<PathBuf, usize>>);

impl NotesStore for Counting {
    fn id(&self) -> &'static str {
        "test.counting"
    }
    fn title(&self) -> &'static str {
        "Counting"
    }
    fn kept(&self) -> Kept {
        Kept::Beside
    }
    fn holds(&self) -> Holds {
        Holds {
            tags: true,
            tag_colors: false,
            rating: false,
            comment: false,
            origin: false,
            limits: "",
        }
    }
    fn default_write(&self) -> Write {
        Write::Off
    }
    fn read(&self, dir: &Path, files: &[Probe<'_>], _: Option<&Listed<'_>>) -> Vec<Option<Notes>> {
        let mut read = self.0.lock();
        for file in files {
            *read.entry(dir.join(file.name)).or_default() += 1;
        }
        vec![None; files.len()]
    }
    fn keys(&self, _: &Probe<'_>, _: bool) -> Vec<RawKey> {
        Vec::new()
    }
    fn write(&self, _: &Probe<'_>, _: &Notes, _: bool) -> std::io::Result<Wrote> {
        Ok(Wrote::NotHere)
    }
}

fn proposal(app: &App) -> Plan {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.pump();
        if let Some(plan) = app.take_proposals().pop() {
            return plan;
        }
        assert!(Instant::now() < deadline, "waited for a proposal");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[test]
fn a_tag_renamed_everywhere_reads_each_file_once_with_its_search() {
    const FILES: usize = 300;
    let tmp = home_tempdir();
    let root = tmp.path().join("it's tagged");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let name = |i: usize| {
        if i % 3 == 0 {
            format!("sub/l'été {i}.txt")
        } else {
            format!("file {i}.txt")
        }
    };
    for i in 0..FILES {
        let path = root.join(name(i));
        std::fs::write(&path, b"x").unwrap();
        let tags: &[&str] = if i % 2 == 0 {
            &["work", "x"]
        } else {
            &["Work"]
        };
        let tags: Vec<String> = tags.iter().map(|&t| t.to_owned()).collect();
        write_tags(&path, &tags).unwrap();
    }
    let app = App::new(AppConfig::ephemeral(tmp.path().join("state"))).unwrap();
    app.plugins
        .add(Arc::new(skwir_plugin_query::QueryPlugin::default()), &app);
    app.plugins
        .add(Arc::new(skwir_plugin_tags::TagsPlugin::default()), &app);
    let counting = Arc::new(Counting::default());
    app.register_notes_store("test", counting.clone());

    let args = serde_json::json!({
        "tag": "work",
        "to": "Job",
        "root": PathArg::from(root.as_path()),
    });
    app.dispatch(&Invocation::with_args("tags.rename", &args))
        .unwrap();
    let plan = proposal(&app);
    assert_eq!(plan.steps.len(), FILES, "{:?}", plan.note);
    {
        let read = counting.0.lock();
        for i in 0..FILES {
            let path = root.join(name(i));
            assert_eq!(
                read.get(&path).copied(),
                Some(1),
                "{}: read by the search alone",
                path.display()
            );
        }
    }

    let done = app.executor.execute_blocking(&plan, app.lifetime());
    assert!(done.error.is_none(), "{:?}", done.error);
    for i in 0..FILES {
        let expected: &[&str] = if i % 2 == 0 { &["Job", "x"] } else { &["Job"] };
        assert_eq!(read_tags(&root.join(name(i))), expected, "{}", name(i));
    }
}
