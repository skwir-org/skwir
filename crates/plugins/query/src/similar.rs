//! Find Similar: a search built from one file's own properties (its kind,
//! the day it was modified, its size, its tags), opened with the chips so
//! it can be adjusted. It searches where the pane is searching, or the
//! folder it shows. What it needs of the file is read on a worker: a stat
//! can block on a sleeping disk.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use skwir_core::app::{App, NotifyLevel};
use skwir_core::cancel::CancelToken;
use skwir_core::entry::{Entry, Value, keys};
use skwir_core::location::Location;
use skwir_core::scheduler::Priority;
use skwir_core::workspace::PaneId;
use skwir_platform::calendar::{Calendar, date_string};
use skwir_platform::mime;

use crate::{SCHEME, lang, location_for};

/// What the files found share with the one selected.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum By {
    Kind,
    Day,
    Size,
    Tags,
}

/// What a file is, for [`terms`].
#[derive(Clone, Debug, Default)]
pub struct Sample<'a> {
    pub name: &'a OsStr,
    /// The name that says its type, when it is not its own: a link's
    /// target's (`Entry::type_name`).
    pub type_name: Option<&'a OsStr>,
    /// The type it has whatever its name (`Entry::inode_type`): a pipe,
    /// a device.
    pub inode: Option<&'static str>,
    pub dir: bool,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
    pub tags: Vec<String>,
}

/// `n` bytes the way a size term writes them, near enough for a range.
fn bytes(n: u64) -> String {
    let n = n as f64;
    for (unit, scale) in [("G", 1u64 << 30), ("M", 1 << 20), ("k", 1 << 10)] {
        let scaled = n / scale as f64;
        if scaled >= 1.0 {
            let text = format!("{scaled:.1}");
            let text = text.strip_suffix(".0").unwrap_or(&text);
            return format!("{text}{unit}");
        }
    }
    format!("{n}")
}

/// The terms that find files like `s` by `by`, or why there are none.
pub fn terms(s: &Sample<'_>, by: By, cal: &Calendar) -> Result<String, String> {
    let name = s.name.to_string_lossy();
    match by {
        By::Kind if s.dir => Ok("kind:folder".into()),
        By::Kind if let Some(inode) = s.inode => Ok(format!("mime:{}", lang::quote(inode))),
        By::Kind => {
            let typed = s.type_name.unwrap_or(s.name);
            let mime = mime::by_name(typed);
            // The first kind is the one its generic icon gives, before text.
            if let Some(kind) = mime.as_ref().and_then(|m| mime::kinds(m).first().copied()) {
                return Ok(format!("kind:{}", kind.name()));
            }
            // A type the desktop does not group: its extension.
            let lowered = typed.to_string_lossy().to_lowercase();
            match lowered.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => {
                    Ok(format!("ext:{}", lang::quote(ext)))
                }
                _ => Err(format!("“{name}” is of no kind the desktop knows")),
            }
        }
        By::Day => s
            .modified
            .map(|t| format!("modified:{}", date_string(cal.day_of(t))))
            .ok_or_else(|| format!("“{name}” has no modification date")),
        By::Size => match s.size {
            Some(0) => Ok("size:0".into()),
            // Half to twice as big: what "about this size" reads as.
            Some(n) => Ok(format!(
                "size:{}..{}",
                bytes(n / 2),
                bytes(n.saturating_mul(2))
            )),
            None => Err(format!("“{name}” has no size")),
        },
        By::Tags => match s.tags.as_slice() {
            [] => Err(format!("“{name}” has no tags")),
            [one] => Ok(format!("tag:{}", lang::quote(one))),
            many => {
                let any: Vec<String> = many
                    .iter()
                    .map(|t| format!("tag:{}", lang::quote(t)))
                    .collect();
                Ok(format!("({})", any.join(" OR ")))
            }
        },
    }
}

fn tags_of(entry: &Entry) -> Vec<String> {
    match entry.metadata.value(keys::TAGS) {
        Some(Value::List(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// Where a similar search looks: where the pane searches, or its folder.
fn root_of(app: &App, pane: PaneId) -> anyhow::Result<PathBuf> {
    if let Some(location) = app.pane_location(pane).filter(|l| &*l.scheme == SCHEME) {
        return Ok(lang::parse(&location.rest)
            .map_err(|e| anyhow::anyhow!("query: {e}"))?
            .root);
    }
    app.pane_dir(pane)
}

/// Build the search for the pane's selected file, or the one given, on a
/// worker in the file's lane, and show it in the pane; say why when the
/// file gives nothing to search by. The pane's: going elsewhere
/// meanwhile, Back or closing it lets the search go.
pub fn start(
    app: &App,
    pane: Option<PaneId>,
    entries: Vec<skwir_core::op::EntryArg>,
    by: By,
    lifetime: &CancelToken,
) -> anyhow::Result<()> {
    let [selected] = app
        .targets(pane, entries)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("select one file to find others like it"))?;
    let pane = app.pane_or_active(pane);
    let root = root_of(app, pane)?;
    let (known, visit) = {
        let ws = app.workspace.read();
        let p = ws
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        (
            p.entry_at(selected.id, &selected.path).cloned(),
            p.location_lifetime(),
        )
    };
    let cancel = lifetime.linked(&visit);
    let weak = app.weak();
    app.scheduler.spawn_in(
        skwir_core::fs::reach::lane(&selected.path),
        Priority::Normal,
        "query.similar",
        cancel.clone(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let path = selected.path;
            // What the listing read, else the file itself.
            let mut entry = match known {
                Some(entry) if entry.metadata.value(keys::SIZE).is_some() => entry,
                _ => {
                    let Some(entry) = skwir_core::fs::hydrate_path(&path, &app.metadata) else {
                        app.notify(
                            NotifyLevel::Error,
                            format!("“{}” cannot be read", name(&path)),
                        );
                        return;
                    };
                    entry
                }
            };
            if by == By::Tags
                && tags_of(&entry).is_empty()
                && let Some(dir) = path.parent()
            {
                app.metadata.fill(dir, std::slice::from_mut(&mut entry));
            }
            let sample = Sample {
                name: &entry.name,
                type_name: Some(entry.type_name()),
                inode: entry.inode_type(),
                dir: entry.is_dir(),
                size: entry.metadata.value(keys::SIZE).and_then(Value::as_u64),
                modified: entry
                    .metadata
                    .value(keys::MODIFIED)
                    .and_then(Value::as_time),
                tags: tags_of(&entry),
            };
            if cancel.is_cancelled() {
                return;
            }
            match terms(&sample, by, &Calendar::system()) {
                Ok(terms) => {
                    let location: Location = location_for(&terms, Some(&root));
                    if let Err(e) = app.navigate(pane, &location) {
                        app.notify(NotifyLevel::Error, format!("{e:#}"));
                    }
                }
                Err(why) => app.notify(NotifyLevel::Info, why),
            }
            app.wake();
        },
    );
    Ok(())
}

fn name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

// Types are read from the free desktops' database (ADR-035).
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sample(name: &str) -> Sample<'_> {
        Sample {
            name: OsStr::new(name),
            ..Sample::default()
        }
    }

    #[test]
    #[allow(
        clippy::duration_suboptimal_units,
        reason = "a time since 1970 is written as the seconds or days it is"
    )]
    fn a_file_says_what_to_search_by() {
        let cal = Calendar::utc();
        let t = |s: &Sample<'_>, by| terms(s, by, &cal);
        assert_eq!(t(&sample("beach.JPG"), By::Kind).unwrap(), "kind:image");
        assert_eq!(t(&sample("notes.md"), By::Kind).unwrap(), "kind:document");
        assert_eq!(t(&sample("scene.blend1"), By::Kind).unwrap(), "ext:blend1");
        assert!(t(&sample("README"), By::Kind).is_err());
        let folder = Sample {
            dir: true,
            ..sample("Photos")
        };
        assert_eq!(t(&folder, By::Kind).unwrap(), "kind:folder");
        // A pipe is one whatever its name says.
        let pipe = Sample {
            inode: Some("inode/fifo"),
            ..sample("stream.png")
        };
        assert_eq!(t(&pipe, By::Kind).unwrap(), "mime:inode/fifo");

        let day = Sample {
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(20_721 * 86_400 + 3_600)),
            ..sample("a")
        };
        assert_eq!(t(&day, By::Day).unwrap(), "modified:2026-09-25");
        assert!(t(&sample("a"), By::Day).is_err());

        let size = |n| Sample {
            size: Some(n),
            ..sample("a")
        };
        assert_eq!(t(&size(3 << 20), By::Size).unwrap(), "size:1.5M..6M");
        assert_eq!(t(&size(3000), By::Size).unwrap(), "size:1.5k..5.9k");
        assert_eq!(t(&size(100), By::Size).unwrap(), "size:50..200");
        assert_eq!(t(&size(0), By::Size).unwrap(), "size:0");

        let tagged = |tags: &[&str]| Sample {
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            ..sample("a")
        };
        assert_eq!(t(&tagged(&["work"]), By::Tags).unwrap(), "tag:work");
        assert_eq!(
            t(&tagged(&["work", "to do"]), By::Tags).unwrap(),
            r#"(tag:work OR tag:"to do")"#
        );
        assert!(t(&tagged(&[]), By::Tags).is_err());
        // Each reads back as a query.
        for by in [By::Kind, By::Day, By::Size, By::Tags] {
            let s = Sample {
                size: Some(1 << 20),
                modified: day.modified,
                tags: vec!["x y".into()],
                ..sample("a.png")
            };
            lang::parse(&t(&s, by).unwrap()).unwrap();
        }
    }

    /// The pane's: shown elsewhere before the search is built, the pane
    /// stays where it went.
    #[test]
    fn a_pane_gone_elsewhere_is_left_where_it_went() {
        use skwir_core::command::Invocation;
        use std::sync::{Arc, Condvar, Mutex};
        let tmp = tempfile::tempdir().unwrap();
        let (dir, other) = (tmp.path().join("it's here"), tmp.path().join("élsewhere"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(
            tmp.path().join("state"),
        ))
        .unwrap();
        app.plugins
            .add(Arc::new(crate::QueryPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&dir)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(10)));
        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["a.txt"] }),
        ))
        .unwrap();
        // Every worker held, its stand-ins too, until the pane went.
        assert!(app.scheduler.wait_idle(Duration::from_secs(10)));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (started, running) = std::sync::mpsc::channel();
        for _ in 0..2 * app.scheduler.workers() {
            let (gate, started) = (Arc::clone(&gate), started.clone());
            app.scheduler.spawn(
                Priority::Visible,
                "test.hold",
                CancelToken::new(),
                move || {
                    let _ = started.send(());
                    let mut open = gate.0.lock().unwrap();
                    while !*open {
                        open = gate.1.wait(open).unwrap();
                    }
                },
            );
            running.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        app.dispatch(&Invocation::with_args(
            "query.similar",
            &serde_json::json!({ "by": "kind" }),
        ))
        .unwrap();
        app.navigate(pane, &Location::from_path(&other)).unwrap();
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert!(app.scheduler.wait_idle(Duration::from_secs(10)));
        assert_eq!(app.pane_location(pane), Some(Location::from_path(&other)));
    }
}
