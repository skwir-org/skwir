//! Find Similar: a search built from one file's own properties (its kind,
//! the day it was modified, its size, its tags), opened with the chips so
//! it can be adjusted. It searches where the pane is searching, or the
//! folder it shows. What it needs of the file is read on a worker: a stat
//! can block on a sleeping disk.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bibli_core::app::{App, NotifyLevel};
use bibli_core::cancel::CancelToken;
use bibli_core::entry::{Entry, Value, keys};
use bibli_core::location::Location;
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use bibli_platform::calendar::{Calendar, date_string};
use bibli_platform::mime;

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
        By::Kind => {
            let mime = mime::by_name(s.name);
            // The first kind is the one its generic icon gives, before text.
            if let Some(kind) = mime.as_ref().and_then(|m| mime::kinds(m).first().copied()) {
                return Ok(format!("kind:{}", kind.name()));
            }
            // A type the desktop does not group: its extension.
            let lowered = name.to_lowercase();
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

/// Build the search for the pane's selected file on a worker and show it
/// in the pane; say why when the file gives nothing to search by.
pub fn start(
    app: &App,
    pane: Option<PaneId>,
    by: By,
    lifetime: &CancelToken,
) -> anyhow::Result<()> {
    let pane = app.pane_or_active(pane);
    let [selected] = app
        .selected_refs(pane)
        .try_into()
        .map_err(|_| anyhow::anyhow!("select one file to find others like it"))?;
    let root = root_of(app, pane)?;
    let known = {
        let ws = app.workspace.read();
        ws.pane(pane)
            .and_then(|p| p.listing.get(selected.id).cloned())
    };
    let weak = app.weak();
    app.scheduler.spawn(
        Priority::Normal,
        "query.similar",
        lifetime.child(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let path = selected.path;
            // What the listing read, else the file itself.
            let mut entry = match known {
                Some(entry) if entry.metadata.value(keys::SIZE).is_some() => entry,
                _ => {
                    let Some(entry) = bibli_core::fs::stat::stat_path(&path) else {
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
                dir: entry.is_dir(),
                size: entry.metadata.value(keys::SIZE).and_then(Value::as_u64),
                modified: entry
                    .metadata
                    .value(keys::MODIFIED)
                    .and_then(Value::as_time),
                tags: tags_of(&entry),
            };
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

#[cfg(test)]
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
    #[cfg(target_os = "linux")]
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
}
