//! bibli plugin: rename-bulk. Turn a selection plus a rename spec into a
//! plan of renames that is safe to apply in order, even when new names
//! collide with old ones (swaps and chains).
//!
//! Per item, in this order: the regex replacement on the full name, then
//! the template, then the case rule. Template placeholders:
//!
//! - `{name}` stem, `{ext}` extension without the dot, `{full}` the whole name
//! - `{n}` counter from `start` by `step`; `{n:03}` zero-padded to 3
//! - `{date}` the entry's modification time as `%Y-%m-%d`; `{date:%Y-%m-%d_%H%M}`
//!   with any `strftime` format (local time zone)
//! - `{parent}` the parent directory's name
//! - `{{` and `}}` for literal braces
//!
//! Case rules (`lower`, `upper`, `title`, `sentence`) apply to the stem and
//! leave the extension alone.

#![allow(clippy::implicit_hasher)]

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bibli_core::app::App;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::{Value, keys};
use bibli_core::op::{Change, EntryRef, Operation, Plan, Rename};
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem};
use bibli_core::workspace::PaneId;

const ID: &str = "rename-bulk";

/// Case rule applied to the stem.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Case {
    Lower,
    Upper,
    Title,
    Sentence,
}

/// What to do to each name.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
pub struct RenameSpec {
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub regex: Option<String>,
    #[serde(default)]
    pub replacement: Option<String>,
    #[serde(default)]
    pub case: Option<Case>,
    #[serde(default = "one")]
    pub start: u64,
    #[serde(default = "one")]
    pub step: u64,
}

fn one() -> u64 {
    1
}

impl Default for RenameSpec {
    fn default() -> Self {
        RenameSpec {
            template: None,
            regex: None,
            replacement: None,
            case: None,
            start: 1,
            step: 1,
        }
    }
}

/// The result: a plan safe to apply in order, and the logical old → new list.
#[derive(Debug)]
pub struct BulkRename {
    pub plan: Plan,
    pub preview: Vec<Change>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BulkRenameError {
    #[error("invalid regex: {0}")]
    Regex(String),
    #[error("invalid template: {0}")]
    Template(String),
    #[error("`{item}` would become an invalid name `{name}`")]
    InvalidName { item: String, name: String },
    #[error("several items would become `{name}`: {}", items.join(", "))]
    Duplicate { name: String, items: Vec<String> },
    #[error("`{item}` would become `{name}`, which already exists")]
    Conflict { item: String, name: String },
    #[error("`{item}` has no modification time yet (needed for {{date}})")]
    MissingDate { item: String },
    #[error("nothing to rename")]
    Empty,
}

/// One input item: where it is, what it is called, and when it changed.
pub type Item = (EntryRef, String, Option<SystemTime>);

fn split_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, ext),
        _ => (name, ""),
    }
}

fn apply_case(name: &str, case: Case) -> String {
    let (stem, ext) = split_ext(name);
    let new_stem = match case {
        Case::Lower => stem.to_lowercase(),
        Case::Upper => stem.to_uppercase(),
        Case::Title => {
            let mut out = String::with_capacity(stem.len());
            let mut start = true;
            for ch in stem.chars() {
                if ch.is_whitespace() || ch == '_' || ch == '-' || ch == '.' {
                    out.push(ch);
                    start = true;
                } else if start {
                    out.extend(ch.to_uppercase());
                    start = false;
                } else {
                    out.extend(ch.to_lowercase());
                }
            }
            out
        }
        Case::Sentence => {
            let mut chars = stem.chars();
            match chars.next() {
                Some(first) => first
                    .to_uppercase()
                    .chain(chars.flat_map(char::to_lowercase))
                    .collect(),
                None => String::new(),
            }
        }
    };
    if ext.is_empty() {
        new_stem
    } else {
        format!("{new_stem}.{ext}")
    }
}

/// Expand the template for one item. `name` is the (regex-transformed) name.
fn render(
    template: &str,
    name: &str,
    parent: &str,
    n: u64,
    mtime: Option<SystemTime>,
    item: &str,
) -> Result<String, BulkRenameError> {
    let (stem, ext) = split_ext(name);
    let mut out = String::with_capacity(template.len() + 16);
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '{' => {
                let mut token = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => token.push(ch),
                        None => {
                            return Err(BulkRenameError::Template(format!(
                                "unclosed `{{` in `{template}`"
                            )));
                        }
                    }
                }
                let (key, arg) = token
                    .split_once(':')
                    .map_or((token.as_str(), None), |(k, a)| (k, Some(a)));
                match key {
                    "name" => out.push_str(stem),
                    "ext" => out.push_str(ext),
                    "full" => out.push_str(name),
                    "parent" => out.push_str(parent),
                    "n" => {
                        let width: usize = match arg {
                            None => 0,
                            Some(a) => a
                                .trim_start_matches('0')
                                .parse()
                                .or_else(|_| {
                                    if a.chars().all(|c| c == '0') {
                                        Ok(a.len())
                                    } else {
                                        Err(())
                                    }
                                })
                                .map_err(|()| {
                                    BulkRenameError::Template(format!("bad counter width `{a}`"))
                                })?,
                        };
                        let _ = write!(out, "{n:0width$}");
                    }
                    "date" => {
                        let t = mtime.ok_or_else(|| BulkRenameError::MissingDate {
                            item: item.to_owned(),
                        })?;
                        let fmt = arg.unwrap_or("%Y-%m-%d");
                        let ts = jiff::Timestamp::try_from(t)
                            .map_err(|e| BulkRenameError::Template(e.to_string()))?;
                        let zoned = ts.to_zoned(jiff::tz::TimeZone::system());
                        let s = zoned.strftime(fmt).to_string();
                        if s.is_empty() && !fmt.is_empty() {
                            return Err(BulkRenameError::Template(format!(
                                "bad date format `{fmt}`"
                            )));
                        }
                        out.push_str(&s);
                    }
                    other => {
                        return Err(BulkRenameError::Template(format!(
                            "unknown placeholder `{{{other}}}`"
                        )));
                    }
                }
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

/// Compute the new name of every item (unchanged names included).
pub fn new_names(items: &[Item], spec: &RenameSpec) -> Result<Vec<String>, BulkRenameError> {
    let re = match &spec.regex {
        Some(r) => Some(regex::Regex::new(r).map_err(|e| BulkRenameError::Regex(e.to_string()))?),
        None => None,
    };
    let replacement = spec.replacement.clone().unwrap_or_default();
    let mut out = Vec::with_capacity(items.len());
    for (i, (entry, name, mtime)) in items.iter().enumerate() {
        let mut n = name.clone();
        if let Some(re) = &re {
            n = re.replace_all(&n, replacement.as_str()).into_owned();
        }
        if let Some(t) = &spec.template {
            let parent = entry
                .path
                .parent()
                .and_then(Path::file_name)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let counter = spec.start + i as u64 * spec.step;
            n = render(t, &n, &parent, counter, *mtime, name)?;
        }
        if let Some(case) = spec.case {
            n = apply_case(&n, case);
        }
        out.push(n);
    }
    Ok(out)
}

/// Order renames so every target is free when its step runs, breaking
/// cycles with a temporary name. `renames` maps item index → new name.
/// Returns the steps as (item index, new name) pairs.
pub fn order_renames(
    old_names: &[String],
    renames: &HashMap<usize, String>,
) -> Vec<(usize, String)> {
    let index_of_old: HashMap<&str, usize> = old_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    // i depends on j when i's target is j's current name and j also moves.
    let dep: HashMap<usize, usize> = renames
        .iter()
        .filter_map(|(&i, target)| {
            index_of_old
                .get(target.as_str())
                .filter(|&&j| j != i && renames.contains_key(&j))
                .map(|&j| (i, j))
        })
        .collect();
    let mut done: HashSet<usize> = HashSet::new();
    let mut steps = Vec::new();
    let mut pending: Vec<usize> = renames.keys().copied().collect();
    pending.sort_unstable();
    let mut temp_counter = 0;
    let mut parked: Vec<(usize, String)> = Vec::new(); // items moved to a temp name, awaiting their final name
    while !pending.is_empty() {
        let mut progressed = false;
        let mut i = 0;
        while i < pending.len() {
            let item = pending[i];
            let free = dep.get(&item).is_none_or(|j| done.contains(j));
            if free {
                steps.push((item, renames[&item].clone()));
                done.insert(item);
                pending.remove(i);
                progressed = true;
            } else {
                i += 1;
            }
        }
        if !progressed {
            // A cycle: park the first pending item under a temp name, which
            // frees its current name for whoever waits on it.
            let item = pending.remove(0);
            temp_counter += 1;
            let tmp = format!(".bibli-rename-tmp-{temp_counter}");
            steps.push((item, tmp.clone()));
            done.insert(item);
            parked.push((item, renames[&item].clone()));
        }
    }
    for (item, final_name) in parked {
        steps.push((item, final_name));
    }
    steps
}

/// Build the plan. `occupied` are names present in the directory that are
/// not part of `items`; a target among them is a conflict.
pub fn plan_bulk_rename_in(
    items: &[Item],
    occupied: &HashSet<String>,
    spec: &RenameSpec,
) -> Result<BulkRename, BulkRenameError> {
    if items.is_empty() {
        return Err(BulkRenameError::Empty);
    }
    let new = new_names(items, spec)?;
    let old: Vec<String> = items.iter().map(|(_, n, _)| n.clone()).collect();
    let mut renames: HashMap<usize, String> = HashMap::new();
    let mut seen: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, n) in new.iter().enumerate() {
        if !valid_name(n) {
            return Err(BulkRenameError::InvalidName {
                item: old[i].clone(),
                name: n.clone(),
            });
        }
        seen.entry(n.as_str()).or_default().push(i);
        if *n != old[i] {
            renames.insert(i, n.clone());
        }
    }
    for (name, idx) in &seen {
        if idx.len() > 1 {
            return Err(BulkRenameError::Duplicate {
                name: (*name).to_owned(),
                items: idx.iter().map(|&i| old[i].clone()).collect(),
            });
        }
    }
    for (&i, target) in &renames {
        if occupied.contains(target) {
            return Err(BulkRenameError::Conflict {
                item: old[i].clone(),
                name: target.clone(),
            });
        }
    }
    let steps = order_renames(&old, &renames);
    let mut current_path: Vec<PathBuf> = items.iter().map(|(e, _, _)| e.path.clone()).collect();
    let mut ops: Vec<Box<dyn Operation>> = Vec::with_capacity(steps.len());
    for (i, name) in steps {
        let entry = EntryRef::new(items[i].0.id, current_path[i].clone());
        let parent = current_path[i]
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        current_path[i] = parent.join(&name);
        ops.push(Box::new(Rename {
            entry,
            new_name: name,
        }));
    }
    let mut idx: Vec<usize> = renames.keys().copied().collect();
    idx.sort_unstable();
    let preview = idx
        .iter()
        .map(|&i| {
            let parent = items[i]
                .0
                .path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            Change::Rename {
                from: items[i].0.path.clone(),
                to: parent.join(&renames[&i]),
            }
        })
        .collect();
    Ok(BulkRename {
        plan: Plan::with_steps(format!("rename {} items", idx.len()), ops),
        preview,
    })
}

/// [`plan_bulk_rename_in`] with no other names in the directory.
pub fn plan_bulk_rename(items: &[Item], spec: &RenameSpec) -> Result<BulkRename, BulkRenameError> {
    plan_bulk_rename_in(items, &HashSet::new(), spec)
}

// ------------------------------------------------------------------ plugin

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct Args {
    #[serde(default)]
    pane: Option<PaneId>,
    #[serde(flatten)]
    spec: RenameSpec,
}

/// Selection in sorted order with names and mtimes, plus the other names in the directory.
fn gather(app: &App, pane: Option<PaneId>) -> anyhow::Result<(Vec<Item>, HashSet<String>)> {
    let pane = pane.unwrap_or_else(|| app.workspace.read().active_id());
    let ws = app.workspace.read();
    let p = ws
        .pane(pane)
        .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
    let open = p
        .open
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("pane has no open location"))?;
    let order = p.listing.sorted(p.sort);
    let mut items = Vec::new();
    for id in order.iter().copied().filter(|id| p.selection.contains(*id)) {
        let Some(e) = p.listing.get(id) else { continue };
        let Some(path) = open.path_of(id) else {
            continue;
        };
        let mtime = e.metadata.value(keys::MODIFIED).and_then(Value::as_time);
        items.push((EntryRef::new(id, path), e.name_str().into_owned(), mtime));
    }
    if items.is_empty() {
        anyhow::bail!("nothing selected");
    }
    let occupied: HashSet<String> = p
        .listing
        .iter()
        .filter(|e| !p.selection.contains(e.id))
        .map(|e| e.name_str().into_owned())
        .collect();
    Ok((items, occupied))
}

/// The rename-bulk plugin.
#[derive(Debug, Default)]
pub struct RenameBulkPlugin;

impl Plugin for RenameBulkPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Bulk rename"
    }

    fn description(&self) -> &'static str {
        "Rename the selection from a template, a regex, and case rules"
    }

    fn register(&self, app: &App) {
        app.commands.register::<Args, _>(
            ID,
            "rename_bulk",
            "Rename several…",
            "Rename the selection from a template, regex, and case rule (plan)",
            |a, app| {
                let (items, occupied) = gather(app, a.pane)?;
                let r = plan_bulk_rename_in(&items, &occupied, &a.spec)?;
                Ok(Outcome::Plan(r.plan))
            },
        );
        app.commands.register::<Args, _>(ID, "rename_bulk.preview", "Preview bulk rename", "Return the old → new names a bulk rename would produce", |a, app| {
            let (items, occupied) = gather(app, a.pane)?;
            let r = plan_bulk_rename_in(&items, &occupied, &a.spec)?;
            let list: Vec<serde_json::Value> = r
                .preview
                .iter()
                .filter_map(|c| match c {
                    Change::Rename { from, to } => Some(serde_json::json!({ "from": from.file_name()?.to_string_lossy(), "to": to.file_name()?.to_string_lossy() })),
                    _ => None,
                })
                .collect();
            Ok(Outcome::Value(serde_json::Value::Array(list)))
        });
        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "ctrl-f2".into(),
                command: Invocation::new("ui.rename_bulk_prompt"),
                when: Some("pane selection".into()),
            },
        );
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: "Rename Several…".into(),
                command: Invocation::new("ui.rename_bulk_prompt"),
                when: Some("selection".into()),
                order: 105,
                icon: Some("rename".into()),
                submenu: Some("More".into()),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::entry::EntryId;
    use std::sync::Arc;

    fn item(name: &str) -> Item {
        (
            EntryRef::new(
                EntryId {
                    dev: 1,
                    ino: name.len() as u64 * 7919 + name.bytes().map(u64::from).sum::<u64>(),
                },
                format!("/d/{name}"),
            ),
            name.into(),
            None,
        )
    }

    fn names(items: &[Item], spec: &RenameSpec) -> Vec<String> {
        new_names(items, spec).unwrap()
    }

    #[test]
    fn templates_counters_and_placeholders() {
        let items = [item("photo.JPG"), item("notes")];
        let spec = RenameSpec {
            template: Some("{parent}-{name}-{n:03}.{ext}".into()),
            start: 5,
            step: 5,
            ..Default::default()
        };
        assert_eq!(names(&items, &spec), ["d-photo-005.JPG", "d-notes-010."]);
        let spec = RenameSpec {
            template: Some("{{{full}}}".into()),
            ..Default::default()
        };
        assert_eq!(names(&items, &spec)[0], "{photo.JPG}");
        assert!(matches!(
            new_names(
                &items,
                &RenameSpec {
                    template: Some("{nope}".into()),
                    ..Default::default()
                }
            ),
            Err(BulkRenameError::Template(_))
        ));
        assert!(matches!(
            new_names(
                &items,
                &RenameSpec {
                    template: Some("{date}".into()),
                    ..Default::default()
                }
            ),
            Err(BulkRenameError::MissingDate { .. })
        ));
    }

    #[test]
    fn regex_groups_then_case() {
        let items = [item("IMG_1234.jpg"), item("img_77.PNG")];
        let spec = RenameSpec {
            regex: Some(r"(?i)img_(\d+)".into()),
            replacement: Some("shot-$1".into()),
            case: Some(Case::Upper),
            ..Default::default()
        };
        assert_eq!(names(&items, &spec), ["SHOT-1234.jpg", "SHOT-77.PNG"]);
        let spec = RenameSpec {
            case: Some(Case::Title),
            ..Default::default()
        };
        assert_eq!(
            names(&[item("my_holiday photos.txt")], &spec),
            ["My_Holiday Photos.txt"]
        );
        let spec = RenameSpec {
            case: Some(Case::Sentence),
            ..Default::default()
        };
        assert_eq!(names(&[item("HELLO wORLD.md")], &spec), ["Hello world.md"]);
        assert!(matches!(
            new_names(
                &items,
                &RenameSpec {
                    regex: Some("(".into()),
                    ..Default::default()
                }
            ),
            Err(BulkRenameError::Regex(_))
        ));
    }

    #[test]
    fn date_uses_mtime() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let items = [(
            EntryRef::new(EntryId { dev: 1, ino: 1 }, "/d/a.txt"),
            "a.txt".to_string(),
            Some(t),
        )];
        let spec = RenameSpec {
            template: Some("{date:%Y}-{name}.{ext}".into()),
            ..Default::default()
        };
        assert_eq!(names(&items, &spec), ["2023-a.txt"]);
    }

    #[test]
    fn validation_and_conflicts() {
        let items = [item("a"), item("b")];
        let dup = plan_bulk_rename(
            &items,
            &RenameSpec {
                template: Some("same".into()),
                ..Default::default()
            },
        );
        assert!(matches!(dup, Err(BulkRenameError::Duplicate { .. })));
        let bad = plan_bulk_rename(
            &items,
            &RenameSpec {
                template: Some("x/{name}".into()),
                ..Default::default()
            },
        );
        assert!(matches!(bad, Err(BulkRenameError::InvalidName { .. })));
        let occupied: HashSet<String> = ["a-1".to_string()].into();
        let conflict = plan_bulk_rename_in(
            &items,
            &occupied,
            &RenameSpec {
                template: Some("{name}-{n}".into()),
                ..Default::default()
            },
        );
        assert!(matches!(conflict, Err(BulkRenameError::Conflict { .. })));
        let unchanged = plan_bulk_rename(&items, &RenameSpec::default()).unwrap();
        assert!(unchanged.plan.is_empty() && unchanged.preview.is_empty());
    }

    #[test]
    fn ordering_handles_chains_and_cycles() {
        // chain: a -> b, b -> c (c free): b must go first.
        let old = ["a".to_string(), "b".to_string()];
        let renames: HashMap<usize, String> = [(0, "b".to_string()), (1, "c".to_string())].into();
        let steps = order_renames(&old, &renames);
        assert_eq!(steps, [(1, "c".to_string()), (0, "b".to_string())]);
        // cycle: a <-> b needs a temp.
        let renames: HashMap<usize, String> = [(0, "b".to_string()), (1, "a".to_string())].into();
        let steps = order_renames(&old, &renames);
        assert_eq!(steps.len(), 3);
        assert!(steps[0].1.starts_with(".bibli-rename-tmp-"));
        assert_eq!(steps[2], (0, "b".to_string()));
    }

    fn setup(names: &[&str]) -> (tempfile::TempDir, Vec<Item>, Arc<App>) {
        let tmp = tempfile::tempdir().unwrap();
        let mut items = Vec::new();
        for n in names {
            let p = tmp.path().join(n);
            std::fs::write(&p, n.as_bytes()).unwrap();
            items.push((EntryRef::from_path(&p).unwrap(), (*n).to_string(), None));
        }
        let app = App::new(bibli_core::app::AppConfig::ephemeral(
            tmp.path().join("app"),
        ))
        .unwrap();
        (tmp, items, app)
    }

    fn contents(dir: &Path) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read_to_string(e.path()).unwrap(),
                )
            })
            .collect();
        v.sort();
        v
    }

    #[test]
    fn swap_and_chain_apply_for_real_and_undo() {
        let (tmp, items, app) = setup(&["a", "b", "c"]);
        // a -> b, b -> a (swap), c -> d (free)
        let spec = RenameSpec {
            regex: Some("^(a|b|c)$".into()),
            replacement: Some("$1".into()),
            ..Default::default()
        };
        assert_eq!(
            new_names(&items, &spec).unwrap(),
            ["a", "b", "c"],
            "identity regex"
        );
        let renames: Vec<String> = vec!["b".into(), "a".into(), "d".into()];
        let map: HashMap<usize, String> = renames
            .iter()
            .enumerate()
            .map(|(i, n)| (i, n.clone()))
            .collect();
        let old: Vec<String> = items.iter().map(|i| i.1.clone()).collect();
        let steps = order_renames(&old, &map);
        let mut current: Vec<PathBuf> = items.iter().map(|i| i.0.path.clone()).collect();
        let mut ops: Vec<Box<dyn Operation>> = Vec::new();
        for (i, name) in steps {
            ops.push(Box::new(Rename {
                entry: EntryRef::new(items[i].0.id, current[i].clone()),
                new_name: name.clone(),
            }));
            current[i] = tmp.path().join(name);
        }
        let plan = Plan::with_steps("t", ops);
        let r = app.executor.execute_blocking(&plan, app.lifetime());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(
            contents(tmp.path()),
            [
                ("a".into(), "b".into()),
                ("b".into(), "a".into()),
                ("d".into(), "c".into())
            ]
        );
        let u = app.executor.undo_blocking(app.lifetime()).unwrap();
        assert!(u.error.is_none(), "{:?}", u.error);
        assert_eq!(
            contents(tmp.path()),
            [
                ("a".into(), "a".into()),
                ("b".into(), "b".into()),
                ("c".into(), "c".into())
            ]
        );
    }

    #[test]
    fn plugin_command_end_to_end() {
        let (tmp, _items, app) = setup(&["x1.txt", "x2.txt"]);
        app.plugins.add(Arc::new(RenameBulkPlugin), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &bibli_core::Location::from_path(tmp.path()))
            .unwrap();
        assert!(app.pump_until_complete(pane, std::time::Duration::from_secs(5)));
        app.dispatch(&Invocation::new("select_all")).unwrap();
        let preview = app
            .dispatch(&Invocation::with_args(
                "rename_bulk.preview",
                &serde_json::json!({ "template": "file-{n:02}.{ext}" }),
            ))
            .unwrap();
        let Outcome::Value(v) = preview else { panic!() };
        assert_eq!(v[0]["to"], "file-01.txt");
        let Outcome::Plan(plan) = app
            .dispatch(&Invocation::with_args(
                "rename_bulk",
                &serde_json::json!({ "template": "file-{n:02}.{ext}" }),
            ))
            .unwrap()
        else {
            panic!()
        };
        let r = app.executor.execute_blocking(&plan, app.lifetime());
        assert!(r.error.is_none());
        assert!(tmp.path().join("file-01.txt").exists() && tmp.path().join("file-02.txt").exists());
        app.plugins.set_enabled("rename-bulk", false, &app);
        assert!(app.commands.get("rename_bulk").is_none());
    }
}
