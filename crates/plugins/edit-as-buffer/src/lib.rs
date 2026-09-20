//! bibli plugin: edit-as-buffer. The oil.nvim idea: the directory listing
//! becomes a text buffer in your editor; when you save and quit, the
//! difference becomes a plan (renames, moves, new folders, trash) that is
//! proposed for review, never applied on its own.
//!
//! Buffer format: one entry per line, `<handle>  <name>` with a trailing
//! `/` for directories. Delete a line to trash the entry, change the name
//! to rename it, put a relative path with `/` in the name to move it into
//! that folder (created if needed), add a line without a handle ending in
//! `/` to create a folder. Line order is ignored. New files are not
//! supported.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use bibli_core::app::{App, NotifyLevel};
use bibli_core::command::{Invocation, Outcome};
use bibli_core::entry::EntryId;
use bibli_core::op::{EntryRef, Mkdir, Move, Operation, Plan, Rename, Trash};
use bibli_core::plugin::Plugin;
use bibli_core::registry::{KeyBinding, MenuItem};
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;

const ID: &str = "edit-as-buffer";

const HEADER: &str = "\
# bibli: edit this listing, save, and quit.
#   change a name            -> rename
#   delete a line            -> move to trash
#   name with a slash        -> move into that folder (created if needed)
#   new line ending with /   -> new folder
# Keep the number at the start of each line. Order does not matter.
";

/// The buffer text for `entries` (id, name, `is_dir`), one line each.
pub fn to_buffer(entries: &[(EntryId, String, bool)]) -> String {
    let mut out = String::from(HEADER);
    for (i, (_, name, is_dir)) in entries.iter().enumerate() {
        let slash = if *is_dir { "/" } else { "" };
        let _ = writeln!(out, "{:>4}  {name}{slash}", i + 1);
    }
    out
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BufferError {
    #[error("line {line}: unknown handle {handle}")]
    UnknownHandle { line: usize, handle: usize },
    #[error("line {line}: handle {handle} appears twice")]
    DuplicateHandle { line: usize, handle: usize },
    #[error("line {line}: invalid name `{name}`")]
    InvalidName { line: usize, name: String },
    #[error("`{name}` is the target of several lines")]
    DuplicateTarget { name: String },
    #[error(
        "line {line}: new files are not supported (`{name}`); end the line with / for a folder"
    )]
    NewFile { line: usize, name: String },
    #[error("line {line}: cannot parse `{text}`")]
    Unparsable { line: usize, text: String },
}

enum Line {
    Handled {
        line: usize,
        handle: usize,
        target: String,
    },
    New {
        line: usize,
        name: String,
    },
}

fn parse(edited: &str) -> Result<Vec<Line>, BufferError> {
    let mut out = Vec::new();
    for (i, raw) in edited.lines().enumerate() {
        let line = i + 1;
        let text = raw.trim_end();
        if text.trim().is_empty() || text.trim_start().starts_with('#') {
            continue;
        }
        let t = text.trim_start();
        let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            let rest = &t[digits.len()..];
            if rest.starts_with("  ") || rest.starts_with('\t') {
                let handle: usize = digits.parse().map_err(|_| BufferError::Unparsable {
                    line,
                    text: text.to_owned(),
                })?;
                out.push(Line::Handled {
                    line,
                    handle,
                    target: rest.trim().to_owned(),
                });
                continue;
            }
        }
        out.push(Line::New {
            line,
            name: t.trim().to_owned(),
        });
    }
    Ok(out)
}

fn valid_component(c: &str) -> bool {
    !c.is_empty() && c != "." && c != ".." && !c.contains('\0')
}

/// Split `target` into (relative subdirectory, final name), validating.
fn split_target(target: &str, line: usize) -> Result<(PathBuf, String), BufferError> {
    let t = target.trim_end_matches('/');
    let mut parts: Vec<&str> = t.split('/').filter(|p| !p.is_empty()).collect();
    let Some(name) = parts.pop() else {
        return Err(BufferError::InvalidName {
            line,
            name: target.to_owned(),
        });
    };
    if target.starts_with('/')
        || !valid_component(name)
        || !parts.iter().all(|p| valid_component(p))
    {
        return Err(BufferError::InvalidName {
            line,
            name: target.to_owned(),
        });
    }
    Ok((parts.iter().collect(), name.to_owned()))
}

/// Order renames within one directory so targets are free, parking cycles
/// under temp names. Returns (index, new name) steps.
fn order_renames(old: &[String], renames: &HashMap<usize, String>) -> Vec<(usize, String)> {
    let index_of_old: HashMap<&str, usize> = old
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    let dep: HashMap<usize, usize> = renames
        .iter()
        .filter_map(|(&i, t)| {
            index_of_old
                .get(t.as_str())
                .filter(|&&j| j != i && renames.contains_key(&j))
                .map(|&j| (i, j))
        })
        .collect();
    let mut done = HashSet::new();
    let mut steps = Vec::new();
    let mut pending: Vec<usize> = renames.keys().copied().collect();
    pending.sort_unstable();
    let mut parked = Vec::new();
    let mut k = 0;
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|&i| {
            if dep.get(&i).is_none_or(|j| done.contains(j)) {
                steps.push((i, renames[&i].clone()));
                done.insert(i);
                false
            } else {
                true
            }
        });
        if pending.len() == before {
            let i = pending.remove(0);
            k += 1;
            steps.push((i, format!(".bibli-edit-tmp-{k}")));
            done.insert(i);
            parked.push((i, renames[&i].clone()));
        }
    }
    steps.extend(parked);
    steps
}

/// Diff the edited buffer against `original` (ref, name, `is_dir`) for the
/// directory `dir` and build the plan.
pub fn diff_buffer(
    original: &[(EntryRef, String, bool)],
    edited: &str,
    dir: &Path,
) -> Result<Plan, BufferError> {
    let lines = parse(edited)?;
    let mut seen_handles: HashSet<usize> = HashSet::new();
    let mut renames: HashMap<usize, String> = HashMap::new(); // same-dir renames
    let mut moves: Vec<(usize, PathBuf, String)> = Vec::new(); // (index, subdir, final name)
    let mut new_dirs: Vec<PathBuf> = Vec::new();
    let mut targets: HashSet<PathBuf> = HashSet::new();
    let mut claim = |p: PathBuf| -> Result<(), BufferError> {
        if !targets.insert(p.clone()) {
            return Err(BufferError::DuplicateTarget {
                name: p.to_string_lossy().into_owned(),
            });
        }
        Ok(())
    };
    for l in lines {
        match l {
            Line::Handled {
                line,
                handle,
                target,
            } => {
                if handle == 0 || handle > original.len() {
                    return Err(BufferError::UnknownHandle { line, handle });
                }
                if !seen_handles.insert(handle) {
                    return Err(BufferError::DuplicateHandle { line, handle });
                }
                let i = handle - 1;
                let (sub, name) = split_target(&target, line)?;
                claim(sub.join(&name))?;
                if sub.as_os_str().is_empty() {
                    if name != original[i].1 {
                        renames.insert(i, name);
                    }
                } else {
                    moves.push((i, sub, name));
                }
            }
            Line::New { line, name } => {
                if !name.ends_with('/') {
                    return Err(BufferError::NewFile { line, name });
                }
                let (sub, last) = split_target(&name, line)?;
                let rel = sub.join(last);
                claim(rel.clone())?;
                new_dirs.push(rel);
            }
        }
    }
    let old: Vec<String> = original.iter().map(|(_, n, _)| n.clone()).collect();
    let mut steps: Vec<Box<dyn Operation>> = Vec::new();
    // 1. Trash entries whose line disappeared.
    for (i, (entry, _, _)) in original.iter().enumerate() {
        if !seen_handles.contains(&(i + 1)) {
            steps.push(Box::new(Trash {
                entry: entry.clone(),
            }));
        }
    }
    // 2. Folders to create, deepest last, deduplicated, only when missing.
    let mut mkdirs: Vec<PathBuf> = Vec::new();
    let want_dir = |rel: &Path, mkdirs: &mut Vec<PathBuf>| {
        let mut acc = PathBuf::new();
        for c in rel.components() {
            acc.push(c);
            let abs = dir.join(&acc);
            if !abs.is_dir() && !mkdirs.contains(&abs) {
                mkdirs.push(abs);
            }
        }
    };
    for d in &new_dirs {
        want_dir(d, &mut mkdirs);
    }
    for (_, sub, _) in &moves {
        want_dir(sub, &mut mkdirs);
    }
    for path in mkdirs {
        steps.push(Box::new(Mkdir { path }));
    }
    // 3. Same-directory renames, cycle-safe.
    let mut current: Vec<PathBuf> = original.iter().map(|(e, _, _)| e.path.clone()).collect();
    for (i, name) in order_renames(&old, &renames) {
        steps.push(Box::new(Rename {
            entry: EntryRef::new(original[i].0.id, current[i].clone()),
            new_name: name.clone(),
        }));
        current[i] = dir.join(name);
    }
    // 4. Moves into subfolders (targets there were validated unique).
    for (i, sub, name) in moves {
        let dest = dir.join(&sub);
        steps.push(Box::new(Move {
            entry: EntryRef::new(original[i].0.id, current[i].clone()),
            dest_dir: dest.clone(),
        }));
        let after_move = dest.join(&original[i].1);
        if name == original[i].1 {
            current[i] = after_move;
        } else {
            steps.push(Box::new(Rename {
                entry: EntryRef::new(original[i].0.id, after_move.clone()),
                new_name: name.clone(),
            }));
            current[i] = dest.join(name);
        }
    }
    Ok(Plan::with_steps(format!("edit {}", dir.display()), steps))
}

// ------------------------------------------------------------------ plugin

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
struct PaneArgs {
    #[serde(default)]
    pane: Option<PaneId>,
}

/// The edit-as-buffer plugin. Finished editor sessions propose their plan
/// through a weak app handle, or report errors as notifications.
#[derive(Default)]
pub struct EditAsBufferPlugin;

fn editor() -> String {
    ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .unwrap_or_else(|| "vi".into())
}

static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn start_session(pane: Option<PaneId>, app: &App) -> anyhow::Result<Outcome> {
    let pane = pane.unwrap_or_else(|| app.workspace.read().active_id());
    let (dir, original) = {
        let ws = app.workspace.read();
        let p = ws
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        let dir = p
            .location()
            .and_then(bibli_core::Location::path)
            .ok_or_else(|| anyhow::anyhow!("pane is not showing a directory"))?;
        let open = p
            .open
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("pane has no open location"))?;
        let order = p.listing.sorted(p.sort);
        let original: Vec<(EntryRef, String, bool)> = order
            .iter()
            .filter_map(|&id| {
                let e = p.listing.get(id)?;
                Some((
                    EntryRef::new(id, open.path_of(id)?),
                    e.name_str().into_owned(),
                    e.is_dir(),
                ))
            })
            .collect();
        (dir, original)
    };
    let buffer = to_buffer(
        &original
            .iter()
            .map(|(r, n, d)| (r.id, n.clone(), *d))
            .collect::<Vec<_>>(),
    );
    let file = std::env::temp_dir().join(format!(
        "bibli-edit-{}-{}.txt",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::write(&file, &buffer)?;
    let mut child = bibli_platform::open::open_terminal_with(
        &dir,
        &[editor(), file.to_string_lossy().into_owned()],
    )?;
    let weak = app.weak();
    app.scheduler.spawn(
        Priority::Background,
        "edit-as-buffer.wait",
        app.lifetime().child(),
        move || {
            let result = match child.wait() {
                Ok(status) if status.success() => match std::fs::read_to_string(&file) {
                    Ok(text) => diff_buffer(&original, &text, &dir).map_err(|e| e.to_string()),
                    Err(e) => Err(format!("cannot read the edited buffer: {e}")),
                },
                Ok(status) => Err(format!("editor exited with {status}")),
                Err(e) => Err(format!("editor failed: {e}")),
            };
            let _ = std::fs::remove_file(&file);
            let Some(app) = weak.upgrade() else { return };
            match result {
                Ok(plan) if plan.is_empty() => {
                    app.notify(NotifyLevel::Info, "edit as text: no changes");
                }
                Ok(plan) => app.propose(plan),
                Err(e) => app.notify(NotifyLevel::Error, format!("edit as text: {e}")),
            }
        },
    );
    Ok(Outcome::Done)
}

impl Plugin for EditAsBufferPlugin {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self) -> &'static str {
        "Edit as text"
    }

    fn description(&self) -> &'static str {
        "Edit the listing in your editor; the difference becomes a plan to review"
    }

    fn register(&self, app: &App) {
        app.commands.register::<PaneArgs, _>(
            ID,
            "edit_as_buffer",
            "Edit as text…",
            "Open the listing in $EDITOR; saving proposes a plan for review",
            |a, app| start_session(a.pane, app),
        );
        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "ctrl-e".into(),
                command: Invocation::new("edit_as_buffer"),
                when: Some("pane".into()),
            },
        );
        app.registry.menus.add(
            ID,
            MenuItem {
                menu: "context".into(),
                title: "Edit as Text…".into(),
                command: Invocation::new("edit_as_buffer"),
                when: Some("empty".into()),
                order: 108,
                icon: Some("rename".into()),
                submenu: None,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    /// Environment writes are unsafe in edition 2024; nextest runs each test
    /// in its own process and this runs before any thread is spawned.
    #[allow(unsafe_code)]
    fn set_env(key: &str, value: &Path) {
        unsafe { std::env::set_var(key, value) };
    }

    fn refs(dir: &Path, names: &[(&str, bool)]) -> Vec<(EntryRef, String, bool)> {
        names
            .iter()
            .map(|(n, d)| {
                let p = dir.join(n);
                if *d {
                    std::fs::create_dir_all(&p).unwrap();
                } else {
                    std::fs::write(&p, n.as_bytes()).unwrap();
                }
                (EntryRef::from_path(&p).unwrap(), (*n).to_string(), *d)
            })
            .collect()
    }

    fn kinds(plan: &Plan) -> Vec<&'static str> {
        plan.steps.iter().map(|s| s.kind()).collect()
    }

    #[test]
    fn buffer_format() {
        let b = to_buffer(&[
            (EntryId { dev: 1, ino: 1 }, "docs".into(), true),
            (EntryId { dev: 1, ino: 2 }, "a.txt".into(), false),
        ]);
        assert!(b.starts_with("# bibli"));
        assert!(b.ends_with("   1  docs/\n   2  a.txt\n"));
    }

    #[test]
    fn diff_rename_move_delete_and_new_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let orig = refs(dir, &[("a.txt", false), ("b.txt", false), ("old", true)]);
        let edited = "# header\n   1  renamed.txt\n   2  archive/2024/b.txt\nphotos/\n";
        let plan = diff_buffer(&orig, edited, dir).unwrap();
        // old/ (line 3) deleted -> trash; mkdir archive, archive/2024, photos; rename a; move b
        assert_eq!(
            kinds(&plan),
            ["trash", "mkdir", "mkdir", "mkdir", "rename", "move"]
        );
        let r = App::new(bibli_core::app::AppConfig::ephemeral(dir.join("app")))
            .unwrap()
            .executor
            .execute_blocking(&plan, &bibli_core::CancelToken::new());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(
            dir.join("renamed.txt").exists()
                && dir.join("archive/2024/b.txt").exists()
                && dir.join("photos").is_dir()
                && !dir.join("old").exists()
        );
    }

    #[test]
    fn diff_swap_and_move_with_rename() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let orig = refs(dir, &[("a", false), ("b", false), ("c", false)]);
        let plan = diff_buffer(&orig, "1  b\n2  a\n3  sub/renamed\n", dir).unwrap();
        assert_eq!(
            kinds(&plan),
            ["mkdir", "rename", "rename", "rename", "move", "rename"]
        );
        let app = App::new(bibli_core::app::AppConfig::ephemeral(dir.join("app"))).unwrap();
        let r = app.executor.execute_blocking(&plan, app.lifetime());
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(std::fs::read_to_string(dir.join("a")).unwrap(), "b");
        assert_eq!(std::fs::read_to_string(dir.join("b")).unwrap(), "a");
        assert_eq!(
            std::fs::read_to_string(dir.join("sub/renamed")).unwrap(),
            "c"
        );
        let u = app.executor.undo_blocking(app.lifetime()).unwrap();
        assert!(u.error.is_none(), "{:?}", u.error);
        assert_eq!(std::fs::read_to_string(dir.join("a")).unwrap(), "a");
        assert!(dir.join("c").exists() && !dir.join("sub").exists());
    }

    #[test]
    fn diff_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let orig = refs(dir, &[("a", false), ("b", false)]);
        assert_eq!(
            diff_buffer(&orig, "1  a\n7  b\n", dir).unwrap_err(),
            BufferError::UnknownHandle { line: 2, handle: 7 }
        );
        assert_eq!(
            diff_buffer(&orig, "1  a\n1  b\n", dir).unwrap_err(),
            BufferError::DuplicateHandle { line: 2, handle: 1 }
        );
        assert_eq!(
            diff_buffer(&orig, "1  same\n2  same\n", dir).unwrap_err(),
            BufferError::DuplicateTarget {
                name: "same".into()
            }
        );
        assert_eq!(
            diff_buffer(&orig, "1  a\n2  b\nnewfile\n", dir).unwrap_err(),
            BufferError::NewFile {
                line: 3,
                name: "newfile".into()
            }
        );
        assert!(matches!(
            diff_buffer(&orig, "1  ..\n2  b\n", dir).unwrap_err(),
            BufferError::InvalidName { .. }
        ));
        assert!(
            diff_buffer(&orig, "1  a\n2  b\n", dir).unwrap().is_empty(),
            "no change, empty plan"
        );
    }

    #[test]
    fn end_to_end_with_a_fake_editor_and_terminal() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("one.txt"), b"1").unwrap();
        let bin = tmp.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        // The fake terminal runs its `-e` arguments; the fake editor rewrites the buffer.
        std::fs::write(
            bin.join("fakeed"),
            "#!/bin/sh\nf=\"$1\"\nsed -i 's/one.txt/two.txt/' \"$f\"\n",
        )
        .unwrap();
        std::fs::write(bin.join("faketerm"), "#!/bin/sh\nshift\nexec \"$@\"\n").unwrap();
        for f in ["fakeed", "faketerm"] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(bin.join(f), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        set_env("TERMINAL", &bin.join("faketerm"));
        set_env("VISUAL", &bin.join("fakeed"));
        let app = App::new(bibli_core::app::AppConfig::ephemeral(
            tmp.path().join("app"),
        ))
        .unwrap();
        app.plugins.add(Arc::new(EditAsBufferPlugin), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &bibli_core::Location::from_path(&dir))
            .unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        app.dispatch(&Invocation::new("edit_as_buffer")).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let plan = loop {
            if let Some(p) = app.take_proposals().pop() {
                break p;
            }
            assert!(std::time::Instant::now() < deadline, "no plan proposed");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(kinds(&plan), ["rename"]);
        assert!(dir.join("one.txt").exists(), "proposing never applies");
    }
}
