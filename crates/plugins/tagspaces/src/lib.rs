//! bibli plugin: `TagSpaces`' sidecars (ADR-027). `TagSpaces` keeps a file's
//! tags and description in a hidden `.ts` folder beside it, `.ts/x.json`,
//! and a folder's own in the folder, `.ts/tsm.json`: plain JSON, a tag an
//! object with its `title` and its `color` (hex). They are a notes store,
//! read with the others and updated where present (created only when the
//! user asks, on the Notes page), and companions of their files
//! (ADR-028); the `.ts` folder is hidden like them. Everything else in a
//! sidecar (its id, a thumbnail's, what a perspective keeps) stays as it
//! was. `TagSpaces`' index of a location, `.ts/tsi.json`, is a search hint
//! (ADR-026).

use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use bibli_core::app::App;
use bibli_core::cancel::CancelToken;
use bibli_core::companions::Companion;
use bibli_core::entry::keys;
use bibli_core::hints::HintProvider;
use bibli_core::metadata::{Listed, same_tag};
use bibli_core::notes::raw::{self, RawKey};
use bibli_core::notes::{Holds, Kept, Notes, NotesStore, Probe, Tag, Write, Wrote};
use bibli_core::plugin::{Category, Plugin};
use parking_lot::Mutex;
use serde_json::{Map, Value};

pub(crate) const ID: &str = "tagspaces";
/// The folder `TagSpaces` keeps its sidecars in.
pub const FOLDER: &str = ".ts";
/// A folder's own sidecar, inside it.
const FOLDER_META: &str = "tsm.json";
/// A location's index, at its root.
const INDEX: &str = "tsi.json";

/// The palette's colors as `TagSpaces` would draw them, for a tag given
/// one here; and their hues, to read a color of its as the nearest.
const PALETTE: [(&str, &str, f32); 8] = [
    ("red", "#e53935", 0.0),
    ("orange", "#fb8c00", 30.0),
    ("yellow", "#fdd835", 55.0),
    ("green", "#43a047", 120.0),
    ("teal", "#00897b", 175.0),
    ("blue", "#1e88e5", 215.0),
    ("purple", "#8e24aa", 285.0),
    ("pink", "#d81b60", 335.0),
];

/// The palette color nearest a `TagSpaces` color (`#rrggbb`, `#rrggbbaa`);
/// none for a gray.
pub fn nearest(hex: &str) -> Option<&'static str> {
    let hex = hex.trim().strip_prefix('#')?;
    if hex.len() < 6 {
        return None;
    }
    let channel = |i: usize| -> Option<f32> {
        Some(f32::from(u8::from_str_radix(hex.get(i..i + 2)?, 16).ok()?) / 255.0)
    };
    let (r, g, b) = (channel(0)?, channel(2)?, channel(4)?);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let chroma = max - min;
    if chroma < 0.15 {
        return None;
    }
    let hue = if (max - r).abs() < f32::EPSILON {
        60.0 * ((g - b) / chroma).rem_euclid(6.0)
    } else if (max - g).abs() < f32::EPSILON {
        60.0 * ((b - r) / chroma + 2.0)
    } else {
        60.0 * ((r - g) / chroma + 4.0)
    };
    PALETTE
        .iter()
        .min_by(|a, b| {
            let d = |h: f32| (hue - h).abs().min(360.0 - (hue - h).abs());
            d(a.2).total_cmp(&d(b.2))
        })
        .map(|(name, _, _)| *name)
}

fn hex_of(color: &str) -> Option<&'static str> {
    PALETTE
        .iter()
        .find(|(n, _, _)| *n == color)
        .map(|(_, h, _)| *h)
}

/// What a sidecar says.
pub fn notes_of(json: &Value) -> Option<Notes> {
    let object = json.as_object()?;
    let mut notes = Notes::default();
    for tag in object
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(title) = tag.get("title").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if title.is_empty() || notes.tag(title).is_some() {
            continue;
        }
        notes.tags.push(Tag {
            name: title.to_owned(),
            color: tag
                .get("color")
                .and_then(Value::as_str)
                .and_then(nearest)
                .map(str::to_owned),
        });
    }
    notes.comment = object
        .get("description")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(str::to_owned);
    Some(notes)
}

/// The sidecar with `notes` written in: a tag it had keeps its object
/// (its exact color), a new one is a new object; everything else stays.
pub fn edit(json: &Value, notes: &Notes) -> Option<Value> {
    let mut object: Map<String, Value> = json.as_object()?.clone();
    let before: Vec<Value> = object
        .get("tags")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tags: Vec<Value> = notes
        .tags
        .iter()
        .map(|t| {
            before
                .iter()
                .find(|b| {
                    b.get("title")
                        .and_then(Value::as_str)
                        .is_some_and(|title| same_tag(title, &t.name))
                })
                .cloned()
                .unwrap_or_else(|| {
                    let mut tag = serde_json::json!({ "title": t.name, "type": "sidecar" });
                    if let Some(hex) = t.color.as_deref().and_then(hex_of) {
                        tag["color"] = hex.into();
                    }
                    tag
                })
        })
        .collect();
    object.insert("tags".into(), Value::Array(tags));
    match &notes.comment {
        Some(text) => {
            object.insert("description".into(), Value::String(text.clone()));
        }
        None => {
            object.remove("description");
        }
    }
    Some(Value::Object(object))
}

/// The sidecars in the `.ts` folders of the folders batches were in: a
/// `.ts` folder is read once for its folder's listing, and again when it
/// changes.
#[derive(Default)]
struct SidecarNames {
    folders: Mutex<Vec<(PathBuf, Option<SystemTime>, Instant, Arc<HashSet<OsString>>)>>,
}

const FOLDERS: usize = 16;
const FRESH: Duration = Duration::from_secs(5);

impl SidecarNames {
    fn of(&self, dir: &Path) -> Arc<HashSet<OsString>> {
        let ts = dir.join(FOLDER);
        let modified = std::fs::metadata(&ts).ok().and_then(|m| m.modified().ok());
        if modified.is_none() {
            return Arc::default();
        }
        let known = self
            .folders
            .lock()
            .iter()
            .find(|(d, m, at, _)| *d == ts && *m == modified && at.elapsed() < FRESH)
            .map(|(_, _, _, names)| Arc::clone(names));
        if let Some(names) = known {
            return names;
        }
        let names: Arc<HashSet<OsString>> = Arc::new(
            std::fs::read_dir(&ts)
                .map(|entries| entries.flatten().map(|e| e.file_name()).collect())
                .unwrap_or_default(),
        );
        let mut folders = self.folders.lock();
        folders.retain(|(d, _, _, _)| *d != ts);
        if folders.len() >= FOLDERS {
            folders.remove(0);
        }
        folders.push((ts, modified, Instant::now(), Arc::clone(&names)));
        names
    }
}

/// A file's sidecar, relative to its folder: `.ts/x.json`, or a folder's
/// own inside it, `x/.ts/tsm.json`.
fn sidecar(name: &OsStr, folder: bool) -> PathBuf {
    if folder {
        Path::new(name).join(FOLDER).join(FOLDER_META)
    } else {
        let mut json = name.to_os_string();
        json.push(".json");
        Path::new(FOLDER).join(json)
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// `TagSpaces`' sidecars as a notes store.
#[derive(Default)]
pub struct TagSpacesStore {
    names: SidecarNames,
}

impl NotesStore for TagSpacesStore {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "TagSpaces sidecars (.ts folders)"
    }
    fn kept(&self) -> Kept {
        Kept::Beside
    }
    fn holds(&self) -> Holds {
        Holds {
            tags: true,
            tag_colors: true,
            rating: false,
            comment: true,
            origin: false,
            limits: "no rating",
        }
    }
    fn default_write(&self) -> Write {
        Write::Update
    }
    fn in_use(&self, dir: &Path) -> bool {
        dir.join(FOLDER).is_dir()
    }
    fn read(
        &self,
        dir: &Path,
        files: &[Probe<'_>],
        listed: Option<&Listed<'_>>,
    ) -> Vec<Option<Notes>> {
        let has_ts = listed.map_or_else(|| dir.join(FOLDER).is_dir(), |l| l(OsStr::new(FOLDER)));
        let names = if has_ts {
            self.names.of(dir)
        } else {
            Arc::default()
        };
        files
            .iter()
            .map(|f| {
                if f.name == FOLDER {
                    return None;
                }
                let relative = sidecar(f.name, f.folder);
                let there = if f.folder {
                    dir.join(&relative).is_file()
                } else {
                    relative.file_name().is_some_and(|n| names.contains(n))
                };
                if !there {
                    return None;
                }
                notes_of(&read_json(&dir.join(relative))?).filter(|n| !n.is_empty())
            })
            .collect()
    }
    fn keys(&self, file: &Probe<'_>, create: bool) -> Vec<RawKey> {
        let Some(dir) = file.path.parent() else {
            return Vec::new();
        };
        let relative = sidecar(file.name, file.folder);
        if create || dir.join(&relative).is_file() {
            vec![RawKey::Beside { path: relative }]
        } else {
            Vec::new()
        }
    }
    fn write(&self, file: &Probe<'_>, notes: &Notes, create: bool) -> std::io::Result<Wrote> {
        let Some(dir) = file.path.parent() else {
            return Ok(Wrote::NotHere);
        };
        let path = dir.join(sidecar(file.name, file.folder));
        let has_notes = !notes.tags.is_empty() || notes.comment.is_some();
        let before = match read_json(&path) {
            Some(json) => json,
            None if path.exists() => {
                tracing::debug!(path = %path.display(), "tagspaces: a sidecar left as it is");
                return Ok(Wrote::NotHere);
            }
            None if create && has_notes => Value::Object(Map::new()),
            None => return Ok(Wrote::NotHere),
        };
        let Some(after) = edit(&before, notes) else {
            return Ok(Wrote::NotHere);
        };
        if after == before {
            return Ok(Wrote::Unchanged);
        }
        let text = serde_json::to_vec_pretty(&after).map_err(std::io::Error::other)?;
        raw::replace(&path, &text)?;
        Ok(Wrote::Written)
    }
}

/// A file's sidecar and thumbnail in `.ts` as its companions; the `.ts`
/// folder itself a folder of companions.
pub struct TagSpacesCompanion;

impl Companion for TagSpacesCompanion {
    fn id(&self) -> &'static str {
        ID
    }
    fn of(&self, name: &OsStr) -> Vec<PathBuf> {
        let mut thumbnail = name.to_os_string();
        thumbnail.push(".jpg");
        vec![sidecar(name, false), Path::new(FOLDER).join(thumbnail)]
    }
    fn owner(&self, _name: &OsStr) -> Option<OsString> {
        None
    }
    fn is_folder(&self, name: &OsStr) -> bool {
        name == FOLDER
    }
}

/// An index read, by its file and when it changed.
type Cached = (
    PathBuf,
    Option<SystemTime>,
    Arc<Vec<(PathBuf, Vec<String>)>>,
);

/// `TagSpaces`' index of a location (`.ts/tsi.json` at its root) as a
/// search hint: the files it lists with a tag.
#[derive(Default)]
pub struct IndexHint {
    /// The indexes read, until they change.
    read: Mutex<Vec<Cached>>,
}

impl IndexHint {
    /// The files a location's index lists, with their tags; its paths are
    /// absolute, or relative to the location.
    fn index(&self, path: &Path) -> Arc<Vec<(PathBuf, Vec<String>)>> {
        let modified = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
        if let Some((_, _, entries)) = self
            .read
            .lock()
            .iter()
            .find(|(p, m, _)| p == path && *m == modified)
        {
            return Arc::clone(entries);
        }
        let root = path
            .parent()
            .and_then(Path::parent)
            .unwrap_or(Path::new("/"));
        let entries: Vec<(PathBuf, Vec<String>)> = read_json(path)
            .and_then(|v| match v {
                Value::Array(items) => Some(items),
                Value::Object(mut o) => match o.remove("entries") {
                    Some(Value::Array(items)) => Some(items),
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or_default()
            .into_iter()
            .filter_map(|e| {
                let at = e.get("path").and_then(Value::as_str)?;
                let tags: Vec<String> = e
                    .get("tags")?
                    .as_array()?
                    .iter()
                    .filter_map(|t| t.get("title")?.as_str().map(str::to_owned))
                    .collect();
                let at = Path::new(at);
                let at = if at.is_absolute() {
                    at.to_path_buf()
                } else {
                    root.join(at)
                };
                (!tags.is_empty()).then_some((at, tags))
            })
            .collect();
        let entries = Arc::new(entries);
        let mut cached = self.read.lock();
        cached.retain(|(p, _, _)| p != path);
        if cached.len() >= FOLDERS {
            cached.remove(0);
        }
        cached.push((path.to_path_buf(), modified, Arc::clone(&entries)));
        entries
    }
}

impl HintProvider for IndexHint {
    fn id(&self) -> &'static str {
        "tagspaces.index"
    }
    fn candidates(
        &self,
        key: &str,
        value: &str,
        root: &Path,
        cancel: &CancelToken,
    ) -> Option<Vec<PathBuf>> {
        if key != keys::TAGS {
            return None;
        }
        // The locations the search is in, or under it: the root's own
        // index and those above it.
        let mut found = Vec::new();
        let mut said = false;
        for dir in root.ancestors() {
            if cancel.is_cancelled() {
                return None;
            }
            let path = dir.join(FOLDER).join(INDEX);
            if !path.is_file() {
                continue;
            }
            said = true;
            found.extend(
                self.index(&path)
                    .iter()
                    .filter(|(at, tags)| {
                        at.starts_with(root) && tags.iter().any(|t| same_tag(t, value))
                    })
                    .map(|(at, _)| at.clone()),
            );
        }
        said.then_some(found)
    }
}

/// The `TagSpaces` plugin.
#[derive(Debug, Default)]
pub struct TagSpacesPlugin;

impl Plugin for TagSpacesPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "TagSpaces"
    }
    fn description(&self) -> &'static str {
        "Tags and descriptions in TagSpaces' .ts sidecars, read and kept in step; its location index as a search hint"
    }
    fn category(&self) -> Category {
        Category::Files
    }
    fn register(&self, app: &App) {
        app.register_notes_store(ID, Arc::new(TagSpacesStore::default()));
        app.companions.register(ID, Arc::new(TagSpacesCompanion));
        app.hints.register(ID, Arc::new(IndexHint::default()));
    }
}

#[cfg(test)]
mod tests;
