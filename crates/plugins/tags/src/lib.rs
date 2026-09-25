//! bibli plugin: tags. Tags live in the `user.xdg.tags` extended attribute
//! (comma-separated, the freedesktop convention), so they travel with the
//! file and need no database. A metadata provider surfaces them as the
//! `tags` field; a `set_tags` operation writes them with a proper inverse.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use bibli_core::app::App;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::{Entry, Metadata, Provenance, Value, keys};
use bibli_core::metadata::{MetadataProvider, same_tag};
use bibli_core::op::{Change, EntryRef, Operation, Plan, ResolveError, StepContext, StepStatus};
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem};
use bibli_core::workspace::PaneId;

const ID: &str = "tags";
/// The metadata field key.
pub const FIELD: &str = keys::TAGS;
/// Read-only companion field from `user.xdg.comment`.
pub const COMMENT_FIELD: &str = "comment";
use bibli_core::metadata::TAGS_XATTR as XATTR_TAGS;
const XATTR_COMMENT: &str = "user.xdg.comment";

/// At most this many files are read by `tags.list { read }`: enough for
/// the tags a search found, bounded for one that found a disk.
const READ_AT_MOST: usize = 20_000;

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct ListArgs {
    #[serde(default)]
    pane: Option<PaneId>,
    /// Also read the tags of entries whose tags were not read yet.
    #[serde(default)]
    read: bool,
}

/// Tags of a path, from its xattr. Missing, unsupported, or unreadable → empty.
pub fn read_tags(path: &Path) -> Vec<String> {
    xattr::get(path, XATTR_TAGS)
        .ok()
        .flatten()
        .map(|b| parse_tags(&String::from_utf8_lossy(&b)))
        .unwrap_or_default()
}

/// Split a comma-separated tag list, trimming and dropping empties and
/// duplicates (tags ignore case: [`same_tag`]; the first spelling stays).
pub fn parse_tags(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in s.split(',') {
        let t = t.trim();
        if !t.is_empty() && !out.iter().any(|x| same_tag(x, t)) {
            out.push(t.to_owned());
        }
    }
    out
}

/// Write tags; an empty list removes the attribute.
pub fn write_tags(path: &Path, tags: &[String]) -> std::io::Result<()> {
    if tags.is_empty() {
        match xattr::remove(path, XATTR_TAGS) {
            // ENODATA: the attribute was not there, which is what we wanted.
            Err(e)
                if rustix::io::Errno::from_io_error(&e) == Some(rustix::io::Errno::NODATA)
                    || e.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(())
            }
            r => r,
        }
    } else {
        xattr::set(path, XATTR_TAGS, tags.join(",").as_bytes())
    }
}

fn same_set(a: &[String], b: &[String]) -> bool {
    let mut a: Vec<&String> = a.iter().collect();
    let mut b: Vec<&String> = b.iter().collect();
    a.sort();
    b.sort();
    a == b
}

fn tags_field(tags: &[String]) -> Value {
    Value::List(
        tags.iter()
            .map(|t| Value::Text(Arc::from(t.as_str())))
            .collect(),
    )
}

/// Tags recorded in an entry's metadata, if the provider filled them.
pub fn tags_of(metadata: &Metadata) -> Option<Vec<String>> {
    match metadata.value(FIELD)? {
        Value::List(items) => Some(
            items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
        ),
        _ => None,
    }
}

// ---------------------------------------------------------------- provider

/// Fills `tags` (and `comment`) from xattrs during hydration.
#[derive(Debug, Default)]
pub struct TagsProvider;

impl MetadataProvider for TagsProvider {
    fn id(&self) -> &'static str {
        ID
    }
    fn fill(&self, dir: &Path, entries: &mut [Entry]) {
        for e in entries {
            let path = dir.join(&e.name);
            let tags = read_tags(&path);
            if !tags.is_empty() {
                e.metadata.set(FIELD, tags_field(&tags), Provenance::User);
            }
            if let Ok(Some(c)) = xattr::get(&path, XATTR_COMMENT) {
                let c = String::from_utf8_lossy(&c).trim().to_owned();
                if !c.is_empty() {
                    e.metadata
                        .set(COMMENT_FIELD, Value::Text(c.into()), Provenance::User);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- operation

/// Set the full tag list of an entry.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SetTags {
    pub entry: EntryRef,
    pub tags: Vec<String>,
}

impl Operation for SetTags {
    fn kind(&self) -> &'static str {
        "set_tags"
    }
    fn clone_box(&self) -> Box<dyn Operation> {
        Box::new(self.clone())
    }
    fn args(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("serializable")
    }
    fn describe(&self) -> Change {
        Change::Metadata {
            path: self.entry.path.clone(),
            field: FIELD.into(),
        }
    }
    fn check(&self, _ctx: &StepContext) -> StepStatus {
        match self.entry.resolve() {
            Ok(p) if same_set(&read_tags(&p), &self.tags) => StepStatus::AlreadyDone,
            Ok(_) => StepStatus::Pending { reversible: true },
            Err(ResolveError::Gone(p)) => StepStatus::Conflict {
                reason: format!("{} no longer exists", p.display()),
            },
            Err(e) => StepStatus::Conflict {
                reason: e.to_string(),
            },
        }
    }
    fn apply(&self, _ctx: &StepContext) -> anyhow::Result<Option<Box<dyn Operation>>> {
        let path = self.entry.resolve()?;
        let previous = read_tags(&path);
        write_tags(&path, &self.tags)
            .map_err(|e| anyhow::anyhow!("set tags on {}: {e}", path.display()))?;
        Ok(Some(Box::new(SetTags {
            entry: EntryRef::new(self.entry.id, path),
            tags: previous,
        })))
    }
}

// ---------------------------------------------------------------- commands

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct TagsArgs {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    pane: Option<PaneId>,
}

/// Current tags of a selected entry, read from the xattr: the listing's
/// field can be stale (a watcher re-stat replaces metadata without running
/// providers) and one getxattr per selected entry is cheap.
fn current_tags(r: &EntryRef) -> Vec<String> {
    read_tags(&r.path)
}

#[derive(Clone, Copy)]
enum Mode {
    Set,
    Add,
    Remove,
}

fn tag_plan(app: &App, a: &TagsArgs, mode: Mode) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(a.pane);
    let refs = app.selected_refs(pane);
    if refs.is_empty() {
        anyhow::bail!("nothing selected");
    }
    let wanted = parse_tags(&a.tags.join(","));
    let verb = match mode {
        Mode::Set => "set tags",
        Mode::Add => "add tags",
        Mode::Remove => "remove tags",
    };
    let mut plan = Plan::new(format!("{verb} {}", wanted.join(", ")));
    for r in refs {
        let current = current_tags(&r);
        let tags = match mode {
            Mode::Set => wanted.clone(),
            // A tag already there under another case is kept as spelled.
            Mode::Add => {
                let mut t = current.clone();
                for w in &wanted {
                    if !t.iter().any(|x| same_tag(x, w)) {
                        t.push(w.clone());
                    }
                }
                t
            }
            Mode::Remove => current
                .iter()
                .filter(|t| !wanted.iter().any(|w| same_tag(w, t)))
                .cloned()
                .collect(),
        };
        plan.push(SetTags { entry: r, tags });
    }
    Ok(Outcome::Plan(plan))
}

/// The tags plugin.
#[derive(Debug, Default)]
pub struct TagsPlugin;

impl Plugin for TagsPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Tags"
    }
    fn description(&self) -> &'static str {
        "Tags stored as extended attributes, no database"
    }
    fn category(&self) -> bibli_core::plugin::Category {
        bibli_core::plugin::Category::Files
    }
    fn register(&self, app: &App) {
        app.metadata.register(ID, Arc::new(TagsProvider));
        app.ops.register::<SetTags>(ID, "set_tags");
        app.commands.register::<TagsArgs, _>(
            ID,
            "tags.set",
            "Set tags",
            "Replace the selection's tags (plan)",
            |a, app| tag_plan(app, &a, Mode::Set),
        );
        app.commands.register::<TagsArgs, _>(
            ID,
            "tags.add",
            "Add tags",
            "Add tags to the selection (plan)",
            |a, app| tag_plan(app, &a, Mode::Add),
        );
        app.commands.register::<TagsArgs, _>(
            ID,
            "tags.remove",
            "Remove tags",
            "Remove tags from the selection (plan)",
            |a, app| tag_plan(app, &a, Mode::Remove),
        );
        app.commands.register::<ListArgs, _>(
            ID,
            "tags.list",
            "List tags",
            "Tags used in the pane's listing, with counts (tags that differ only in case count as one); with `read`, read from the files whose tags the listing has not read yet (blocks: dispatch from a worker)",
            |a, app| {
                let pane = app.pane_or_active(a.pane);
                // Keyed by the lowercase form, shown as first spelled.
                let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
                let mut count = |tags: Vec<String>| {
                    for t in tags {
                        counts.entry(t.to_lowercase()).or_insert((t, 0)).1 += 1;
                    }
                };
                // What the listing read, and the files it has not, taken
                // under the lock and read after it.
                let mut unread = Vec::new();
                let open = {
                    let ws = app.workspace.read();
                    let Some(p) = ws.pane(pane) else {
                        anyhow::bail!("no such pane");
                    };
                    for e in p.listing.iter() {
                        match tags_of(&e.metadata) {
                            Some(tags) => count(tags),
                            None if a.read && unread.len() < READ_AT_MOST => unread.push(e.id),
                            None => {}
                        }
                    }
                    p.open.clone()
                };
                if let Some(open) = open {
                    for id in unread {
                        if let Some(path) = open.path_of(id) {
                            count(read_tags(&path));
                        }
                    }
                }
                let counts: BTreeMap<String, usize> = counts.into_values().collect();
                Ok(Outcome::Value(serde_json::to_value(counts)?))
            },
        );
        app.registry.keymap.add(
            ID,
            KeyBinding {
                // Ctrl+Shift+T reopens a closed tab, as in file managers.
                keys: "ctrl-shift-g".into(),
                command: Invocation::new("ui.tags_prompt"),
                when: Some("pane selection".into()),
            },
        );
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: "Tags…".into(),
                command: Invocation::new("ui.tags_prompt"),
                when: Some("selection".into()),
                order: 150,
                icon: Some("tag".into()),
                submenu: None,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bibli_core::app::AppConfig;
    use bibli_core::location::Location;
    use std::time::Duration;

    fn home_tempdir() -> tempfile::TempDir {
        let base = dirs::home_dir().unwrap().join(".cache").join("bibli-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new()
            .prefix("tags-")
            .tempdir_in(base)
            .unwrap()
    }

    fn hydrate_all(app: &App, pane: PaneId) {
        let ids: Vec<_> = app
            .workspace
            .read()
            .pane(pane)
            .unwrap()
            .listing
            .ids()
            .collect();
        app.dispatch(&Invocation::with_args(
            "hydrate",
            &serde_json::json!({ "ids": ids, "pane": pane }),
        ))
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            app.pump();
            if app
                .workspace
                .read()
                .pane(pane)
                .unwrap()
                .listing
                .iter()
                .all(|e| e.metadata.is_hydrated())
            {
                return;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn parse_and_roundtrip() {
        assert_eq!(parse_tags(" a, b ,a,,c "), ["a", "b", "c"]);
        assert_eq!(parse_tags("Work, work,WORK, Été, été"), ["Work", "Été"]);
        let tmp = home_tempdir();
        let f = tmp.path().join("f");
        std::fs::write(&f, b"x").unwrap();
        assert!(read_tags(&f).is_empty());
        write_tags(&f, &["work".into(), "urgent".into()]).unwrap();
        assert_eq!(read_tags(&f), ["work", "urgent"]);
        write_tags(&f, &[]).unwrap();
        assert!(read_tags(&f).is_empty());
        write_tags(&f, &[]).unwrap();
    }

    #[test]
    fn provider_fills_tags_and_plans_apply_and_undo() {
        let tmp = home_tempdir();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        let f = dir.join("doc.txt");
        std::fs::write(&f, b"x").unwrap();
        write_tags(&f, &["old".into()]).unwrap();
        let app = App::new(AppConfig::ephemeral(tmp.path())).unwrap();
        app.plugins.add(Arc::new(TagsPlugin), &app);
        assert!(app.metadata.ids().contains(&"tags"));
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &Location::from_path(&dir)).unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        hydrate_all(&app, pane);
        {
            let ws = app.workspace.read();
            let e = ws
                .pane(pane)
                .unwrap()
                .listing
                .find_by_name("doc.txt".as_ref())
                .unwrap();
            assert_eq!(tags_of(&e.metadata).unwrap(), ["old"]);
            assert_eq!(e.metadata.get(FIELD).unwrap().provenance, Provenance::User);
        }
        app.dispatch(&Invocation::with_args(
            "select",
            &serde_json::json!({ "names": ["doc.txt"] }),
        ))
        .unwrap();

        let Outcome::Plan(plan) = app
            .dispatch(&Invocation::with_args(
                "tags.add",
                &serde_json::json!({ "tags": ["new", "old"] }),
            ))
            .unwrap()
        else {
            panic!("plan")
        };
        let r = app.executor.execute_blocking(&plan, app.lifetime());
        assert!(r.error.is_none());
        assert_eq!(read_tags(&f), ["old", "new"]);
        // Re-apply is a no-op.
        let again = Plan::from_data(&plan.to_data(), &app.ops).unwrap();
        let r2 = app.executor.execute_blocking(&again, app.lifetime());
        assert_eq!((r2.applied, r2.skipped), (0, 1));

        let Outcome::Plan(plan) = app
            .dispatch(&Invocation::with_args(
                "tags.remove",
                &serde_json::json!({ "tags": ["old"] }),
            ))
            .unwrap()
        else {
            panic!("plan")
        };
        app.executor.execute_blocking(&plan, app.lifetime());
        assert_eq!(read_tags(&f), ["new"]);
        // Tags ignore case: adding "NEW" keeps "new", removing "NEW" removes it.
        let Outcome::Plan(plan) = app
            .dispatch(&Invocation::with_args(
                "tags.add",
                &serde_json::json!({ "tags": ["NEW"] }),
            ))
            .unwrap()
        else {
            panic!("plan")
        };
        let r = app.executor.execute_blocking(&plan, app.lifetime());
        assert_eq!((r.applied, r.skipped), (0, 1));
        assert_eq!(read_tags(&f), ["new"]);
        let Outcome::Plan(plan) = app
            .dispatch(&Invocation::with_args(
                "tags.remove",
                &serde_json::json!({ "tags": ["NEW"] }),
            ))
            .unwrap()
        else {
            panic!("plan")
        };
        app.executor.execute_blocking(&plan, app.lifetime());
        assert!(read_tags(&f).is_empty());
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_tags(&f), ["new"]);
        let Outcome::Plan(plan) = app
            .dispatch(&Invocation::with_args(
                "tags.set",
                &serde_json::json!({ "tags": [] }),
            ))
            .unwrap()
        else {
            panic!("plan")
        };
        app.executor.execute_blocking(&plan, app.lifetime());
        assert!(read_tags(&f).is_empty());

        // Undo walks back: set([]) -> ["new"], remove -> ["old","new"], add -> ["old"].
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_tags(&f), ["new"]);
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_tags(&f), ["old", "new"]);
        app.executor.undo_blocking(app.lifetime()).unwrap();
        assert_eq!(read_tags(&f), ["old"]);

        // tags.list counts what the listing knows (hydrate again to refresh).
        hydrate_all(&app, pane);
        let Outcome::Value(v) = app.dispatch(&Invocation::new("tags.list")).unwrap() else {
            panic!()
        };
        assert_eq!(v["old"], 1);

        // Disabling withdraws the provider, the op kind, and the contributions.
        app.plugins.set_enabled("tags", false, &app);
        assert!(!app.metadata.ids().contains(&"tags"));
        assert!(
            app.ops
                .build(&bibli_core::op::OpData {
                    kind: "set_tags".into(),
                    args: serde_json::json!({})
                })
                .is_err()
        );
        assert!(app.commands.get("tags.set").is_none());
        assert!(app.registry.menus.all().iter().all(|m| m.title != "Tags…"));
    }
}
