//! skwir plugin: edit-as-buffer. The oil.nvim idea: the directory listing
//! becomes a text buffer in your editor; when you save and quit, the
//! difference becomes a plan (renames, moves, new folders, trash) that is
//! proposed for review, never applied on its own.
//!
//! Buffer format: one entry per line, `<handle>  <name>` with a trailing
//! `/` for directories. Delete a line to trash the entry, change the name
//! to rename it, put a relative path with `/` in the name to move it into
//! that folder (created if needed), add a line without a handle ending in
//! `/` to create a folder. Line order is ignored. New files are not
//! supported. A folder named in a path is the folder as the edited buffer
//! leaves it: `b/x` after renaming `a/` to `b/` moves `x` into it.
//!
//! Names are escaped so every name is one line that reads back exactly:
//! `\\` for a backslash, and `\xHH` for a byte that is not text, a control
//! character (a newline in a name), or a space at either end of a name.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use skwir_core::CancelToken;
use skwir_core::app::{App, NotifyLevel, PaneArg};
use skwir_core::command::{Invocation, Outcome};
use skwir_core::entry::FileId;
use skwir_core::op::{
    EntryRef, Mkdir, Move, Operation, Plan, Rename, RenameOrderError, RenameRequest, Trash,
    order_renames, valid_file_name,
};
use skwir_core::plugin::Plugin;
use skwir_core::registry::{KeyBinding, MenuItem};
use skwir_core::scheduler::Priority;
use skwir_core::workspace::PaneId;

const ID: &str = "edit-as-buffer";

const HEADER: &str = "\
# skwir: edit this listing, save, and quit.
#   change a name            -> rename
#   delete a line            -> move to trash
#   name with a slash        -> move into that folder (created if needed)
#   new line ending with /   -> new folder
# Keep the number at the start of each line. Order does not matter.
# In names, \\\\ is a backslash and \\xHH a raw byte (\\x0a a newline,
# \\x20 a space at either end).
";

/// `name` as one line of text that [`unescape`] turns back into it.
pub fn escape(name: &OsStr) -> String {
    let bytes = name.as_encoded_bytes();
    let mut out = String::with_capacity(bytes.len());
    let hex = |out: &mut String, bytes: &[u8]| {
        for b in bytes {
            let _ = write!(out, "\\x{b:02x}");
        }
    };
    for chunk in bytes.utf8_chunks() {
        let text = chunk.valid();
        let chars = text.char_indices().count();
        for (n, (at, c)) in text.char_indices().enumerate() {
            let edge = (n == 0 && out.is_empty()) || (n + 1 == chars && chunk.invalid().is_empty());
            if c == '\\' {
                out.push_str("\\\\");
            } else if c.is_control() || (edge && c.is_whitespace()) {
                hex(&mut out, &text.as_bytes()[at..at + c.len_utf8()]);
            } else {
                out.push(c);
            }
        }
        hex(&mut out, chunk.invalid());
    }
    out
}

/// The bytes `text` stands for: `\\` and `\xHH` decoded, any other
/// backslash kept as it is.
pub fn unescape(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            if bytes.get(i + 1) == Some(&b'\\') {
                out.push(b'\\');
                i += 2;
                continue;
            }
            if bytes.get(i + 1) == Some(&b'x')
                && let Some(b) = bytes
                    .get(i + 2..i + 4)
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                out.push(b);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// The buffer text for `entries` (id, name, `is_dir`), one line each.
pub fn to_buffer(entries: &[(FileId, OsString, bool)]) -> String {
    let mut out = String::from(HEADER);
    for (i, (_, name, is_dir)) in entries.iter().enumerate() {
        let slash = if *is_dir { "/" } else { "" };
        let _ = writeln!(out, "{:>4}  {}{slash}", i + 1, escape(name));
    }
    out
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BufferError {
    #[error("line {line}: unknown handle {handle}")]
    UnknownHandle { line: usize, handle: usize },
    #[error("line {line}: handle {handle} appears twice")]
    DuplicateHandle { line: usize, handle: usize },
    #[error("line {line}: invalid name `{name}`: {reason}")]
    InvalidName {
        line: usize,
        name: String,
        reason: String,
    },
    #[error("`{name}` is the target of several lines")]
    DuplicateTarget { name: String },
    #[error("line {line}: `{name}` already exists")]
    Taken { line: usize, name: String },
    #[error(
        "line {line}: new files are not supported (`{name}`); end the line with / for a folder"
    )]
    NewFile { line: usize, name: String },
}

enum Line {
    Handled {
        line: usize,
        handle: usize,
        target: Target,
    },
    New {
        line: usize,
        target: Target,
    },
}

/// A parsed name: folders below the listed one, then the final name.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    sub: Vec<OsString>,
    name: OsString,
    /// Written with a trailing `/`.
    folder: bool,
}

impl Target {
    fn rel(&self) -> PathBuf {
        let mut p: PathBuf = self.sub.iter().collect();
        p.push(&self.name);
        p
    }
}

fn parse_target(text: &str, line: usize) -> Result<Target, BufferError> {
    let invalid = |reason: String| BufferError::InvalidName {
        line,
        name: text.to_owned(),
        reason,
    };
    // Escapes decode to bytes, so a `\x2f` is a slash in a name, which
    // `valid_file_name` refuses, not a separator.
    if text.starts_with('/') {
        return Err(invalid("a path must stay inside this folder".into()));
    }
    let folder = text.ends_with('/');
    let mut parts: Vec<OsString> = text
        .split('/')
        .filter(|p| !p.is_empty())
        .map(|p| skwir_core::os_path::from_bytes(unescape(p)))
        .collect();
    let Some(name) = parts.pop() else {
        return Err(invalid("a name cannot be empty".into()));
    };
    for part in parts.iter().chain([&name]) {
        valid_file_name(part).map_err(|e| invalid(e.to_string()))?;
    }
    Ok(Target {
        sub: parts,
        name,
        folder,
    })
}

fn parse(edited: &str) -> Result<Vec<Line>, BufferError> {
    let mut out = Vec::new();
    for (i, raw) in edited.lines().enumerate() {
        let line = i + 1;
        let t = raw.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let digits = t.bytes().take_while(u8::is_ascii_digit).count();
        let rest = &t[digits..];
        if digits > 0
            && (rest.starts_with("  ") || rest.starts_with('\t'))
            && let Ok(handle) = t[..digits].parse::<usize>()
        {
            out.push(Line::Handled {
                line,
                handle,
                target: parse_target(rest.trim(), line)?,
            });
        } else {
            out.push(Line::New {
                line,
                target: parse_target(t, line)?,
            });
        }
    }
    Ok(out)
}

/// A listed entry and what the buffer says becomes of it.
enum Fate {
    Trash,
    Keep,
    Rename(OsString),
    Move(Target),
}

/// Diff the edited buffer against `original` (ref, name, `is_dir`) for the
/// directory `dir` and build the plan. Stats the folder's contents where
/// the buffer alone cannot say whether a name is free: worker-only.
///
/// The steps run in an order where each one finds its entry and its target
/// free: trash; moves into folders that stay as they are; renames in `dir`,
/// ordered by [`order_renames`] (an entry whose name is wanted but that
/// moves into a folder that does not exist yet is parked under a free name
/// first); new folders; the remaining moves; and renames of moved entries.
pub fn diff_buffer(
    original: &[(EntryRef, OsString, bool)],
    edited: &str,
    dir: &Path,
) -> Result<Plan, BufferError> {
    let lines = parse(edited)?;
    let mut fates: Vec<Fate> = original.iter().map(|_| Fate::Trash).collect();
    let mut line_of: Vec<usize> = vec![0; original.len()];
    let mut new_dirs: Vec<(usize, PathBuf)> = Vec::new();
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
                let i = handle - 1;
                if line_of[i] != 0 {
                    return Err(BufferError::DuplicateHandle { line, handle });
                }
                line_of[i] = line;
                claim(target.rel())?;
                fates[i] = if !target.sub.is_empty() {
                    Fate::Move(target)
                } else if target.name == original[i].1 {
                    Fate::Keep
                } else {
                    Fate::Rename(target.name)
                };
            }
            Line::New { line, target } => {
                if !target.folder {
                    return Err(BufferError::NewFile {
                        line,
                        name: target.rel().to_string_lossy().into_owned(),
                    });
                }
                claim(target.rel())?;
                new_dirs.push((line, target.rel()));
            }
        }
    }

    // The folders of `dir` once the renames are done: final name -> where
    // that folder is now. A path through one of them can be checked on disk.
    let mut folders: HashMap<OsString, PathBuf> = HashMap::new();
    for (i, (entry, name, is_dir)) in original.iter().enumerate() {
        match &fates[i] {
            Fate::Keep if *is_dir => {
                folders.insert(name.clone(), entry.path.clone());
            }
            Fate::Rename(to) if *is_dir => {
                folders.insert(to.clone(), entry.path.clone());
            }
            _ => {}
        }
    }
    // Stable folders keep their name: a move into one can run before the
    // renames, which frees the mover's name early.
    let stable: HashSet<&OsStr> = original
        .iter()
        .zip(&fates)
        .filter(|((_, _, is_dir), fate)| *is_dir && matches!(fate, Fate::Keep))
        .map(|((_, name, _), _)| name.as_os_str())
        .collect();
    // Folders to create for `rel` (relative to `dir`), parents first.
    let missing = |rel: &Path, out: &mut Vec<PathBuf>| {
        let mut parts = rel.components();
        let Some(top) = parts.next() else { return };
        let top = top.as_os_str();
        let mut now = folders.get(top).cloned();
        let mut at = dir.join(top);
        if now.is_none() && !out.contains(&at) {
            out.push(at.clone());
        }
        for part in parts {
            at.push(part);
            now = now.map(|p| p.join(part)).filter(|p| p.is_dir());
            if now.is_none() && !out.contains(&at) {
                out.push(at.clone());
            }
        }
    };
    let mut early_dirs = Vec::new();
    let mut late_dirs = Vec::new();
    for (_, rel) in &new_dirs {
        missing(rel, &mut late_dirs);
    }
    let mut early = Vec::new();
    let mut late = Vec::new();
    for (i, fate) in fates.iter().enumerate() {
        let Fate::Move(target) = fate else { continue };
        let sub: PathBuf = target.sub.iter().collect();
        if stable.contains(target.sub[0].as_os_str()) {
            missing(&sub, &mut early_dirs);
            early.push(i);
        } else {
            missing(&sub, &mut late_dirs);
            late.push(i);
        }
    }
    // Names in `dir` that something wants once the renames run.
    let wanted: HashSet<&OsStr> = fates
        .iter()
        .filter_map(|f| match f {
            Fate::Rename(to) => Some(to.as_os_str()),
            _ => None,
        })
        .chain(
            late_dirs
                .iter()
                .filter(|p| p.parent() == Some(dir))
                .filter_map(|p| p.file_name()),
        )
        .collect();

    let mut steps: Vec<Box<dyn Operation>> = Vec::new();
    let mut current: Vec<PathBuf> = original.iter().map(|(e, _, _)| e.path.clone()).collect();
    let mut freed: HashSet<OsString> = HashSet::new();
    for (i, (entry, name, _)) in original.iter().enumerate() {
        if matches!(fates[i], Fate::Trash) {
            steps.push(Box::new(Trash {
                entry: entry.clone(),
            }));
            freed.insert(name.clone());
        }
    }
    for path in early_dirs {
        steps.push(Box::new(Mkdir { path }));
    }
    for &i in &early {
        let Fate::Move(target) = &fates[i] else {
            unreachable!()
        };
        move_into(&mut steps, &mut current, i, original[i].0.id, dir, target);
        freed.insert(original[i].1.clone());
    }

    // Renames, and the parking of late movers whose name is wanted. Once
    // the trash and the early moves ran, a name in `dir` is taken by what
    // stays (kept, or moving later), is free if it was freed, and is
    // otherwise asked of the disk (a hidden file, one created since).
    let occupied: HashSet<&OsStr> = original
        .iter()
        .zip(&fates)
        .filter(|(_, f)| matches!(f, Fate::Keep | Fate::Move(_)))
        .map(|((_, name, _), _)| name.as_os_str())
        .collect();
    let exists = |p: &Path| {
        let taken_now = || p.symlink_metadata().is_ok();
        match p.file_name() {
            Some(name) if p.parent() == Some(dir) => {
                !freed.contains(name) && (occupied.contains(name) || taken_now())
            }
            _ => taken_now(),
        }
    };
    let mut requests = Vec::new();
    let mut request_line = Vec::new();
    for (i, (entry, _, _)) in original.iter().enumerate() {
        if let Fate::Rename(to) = &fates[i] {
            requests.push(RenameRequest {
                entry: entry.clone(),
                new_name: to.clone(),
            });
            request_line.push(line_of[i]);
        }
    }
    let mut parked_serial = 0u32;
    for &i in &late {
        if !wanted.contains(original[i].1.as_os_str()) {
            continue;
        }
        let parked = loop {
            parked_serial += 1;
            let name = OsString::from(format!(".skwir-edit-{parked_serial}"));
            if !wanted.contains(name.as_os_str()) && !exists(&dir.join(&name)) {
                break name;
            }
        };
        requests.push(RenameRequest {
            entry: original[i].0.clone(),
            new_name: parked.clone(),
        });
        request_line.push(line_of[i]);
        current[i] = dir.join(parked);
    }
    let renames = order_renames(&requests, exists).map_err(|e| match e {
        RenameOrderError::InvalidName { item, reason } => BufferError::InvalidName {
            line: request_line[item],
            name: requests[item].new_name.to_string_lossy().into_owned(),
            reason,
        },
        RenameOrderError::Duplicate { target, .. } => BufferError::DuplicateTarget {
            name: target.to_string_lossy().into_owned(),
        },
        RenameOrderError::Conflict { item, target } => BufferError::Taken {
            line: request_line[item],
            name: target.to_string_lossy().into_owned(),
        },
    })?;
    steps.extend(
        renames
            .into_iter()
            .map(|r| Box::new(r) as Box<dyn Operation>),
    );

    for path in late_dirs {
        steps.push(Box::new(Mkdir { path }));
    }
    for &i in &late {
        let Fate::Move(target) = &fates[i] else {
            unreachable!()
        };
        move_into(&mut steps, &mut current, i, original[i].0.id, dir, target);
    }
    Ok(Plan::with_steps(format!("edit {}", dir.display()), steps))
}

/// Move entry `i` into its target folder, then give it its final name
/// there if that differs from the name it arrives with.
fn move_into(
    steps: &mut Vec<Box<dyn Operation>>,
    current: &mut [PathBuf],
    i: usize,
    id: FileId,
    dir: &Path,
    target: &Target,
) {
    let dest: PathBuf = dir.join(target.sub.iter().collect::<PathBuf>());
    let arrives = current[i].file_name().unwrap_or_default().to_owned();
    steps.push(Box::new(Move {
        entry: EntryRef::new(id, current[i].clone()),
        dest_dir: dest.clone(),
        new_name: None,
    }));
    current[i] = dest.join(&arrives);
    if arrives != target.name {
        steps.push(Box::new(Rename {
            entry: EntryRef::new(id, current[i].clone()),
            new_name: target.name.clone(),
        }));
        current[i] = dest.join(&target.name);
    }
}

// ------------------------------------------------------------------ plugin

/// The edit-as-buffer plugin. Finished editor sessions propose their plan
/// through a weak app handle, or report errors as notifications.
#[derive(Default)]
pub struct EditAsBufferPlugin {
    /// Cancelled on disable: a session whose editor closes afterwards
    /// proposes nothing.
    lifetime: Mutex<CancelToken>,
}

fn editor() -> String {
    ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "vi".into())
}

/// The argv that edits `file` with `editor`, read by the shell as git
/// reads `$EDITOR`: `code --wait` or a quoted path with spaces both work.
fn editor_argv(editor: &str, file: &Path) -> Vec<OsString> {
    vec![
        "/bin/sh".into(),
        "-c".into(),
        format!("{editor} \"$@\"").into(),
        editor.into(),
        file.as_os_str().to_owned(),
    ]
}

/// A private file for the buffer: created exclusively, readable by the
/// user alone, in the runtime directory when there is one.
fn buffer_file(text: &str) -> anyhow::Result<tempfile::TempPath> {
    use std::io::Write as _;
    let dir = skwir_platform::xdg::runtime_dir().unwrap_or_else(std::env::temp_dir);
    let mut file = tempfile::Builder::new()
        .prefix("skwir-edit-")
        .suffix(".txt")
        .tempfile_in(dir)?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    Ok(file.into_temp_path())
}

type Original = Vec<(EntryRef, OsString, bool)>;

fn start_session(
    pane: Option<PaneId>,
    app: &App,
    lifetime: CancelToken,
) -> anyhow::Result<Outcome> {
    let pane = app.pane_or_active(pane);
    let dir = app.pane_dir(pane)?;
    let original: Original = {
        let ws = app.workspace.read();
        let p = ws
            .pane(pane)
            .ok_or_else(|| anyhow::anyhow!("no such pane"))?;
        let open = p
            .open
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("pane has no open location"))?;
        p.listing
            .sorted(p.sort)
            .iter()
            .filter_map(|&id| {
                let e = p.listing.get(id)?;
                Some((
                    EntryRef::new(id.file, open.path_of(id)?).with_only_name(e.is_only_name()),
                    e.name.clone(),
                    e.is_dir(),
                ))
            })
            .collect()
    };
    let buffer = to_buffer(
        &original
            .iter()
            .map(|(r, n, d)| (r.id, n.clone(), *d))
            .collect::<Vec<_>>(),
    );
    let file = buffer_file(&buffer)?;
    let argv = editor_argv(&editor(), &file);
    let start_dir = dir.clone();
    let weak = app.weak();
    let cwd = dir.clone();
    let started = lifetime.clone();
    let start = move || {
        skwir_platform::open::open_terminal_watched(&cwd, &argv, move |status| {
            // The reaper thread only hands the result to a worker.
            let Some(app) = weak.upgrade() else { return };
            let weak = app.weak();
            // The diff stats names in the folder: in its lane.
            app.scheduler.spawn_in(
                skwir_core::fs::reach::lane(&dir),
                Priority::Normal,
                "edit-as-buffer.diff",
                lifetime.clone(),
                move || {
                    let result = match status {
                        Ok(status) if status.success() => match std::fs::read_to_string(&file) {
                            Ok(text) => {
                                diff_buffer(&original, &text, &dir).map_err(|e| e.to_string())
                            }
                            Err(e) => Err(format!("cannot read the edited buffer: {e}")),
                        },
                        Ok(status) => Err(format!("editor exited with {status}")),
                        Err(e) => Err(format!("editor failed: {e}")),
                    };
                    drop(file);
                    let Some(app) = weak.upgrade() else { return };
                    if lifetime.is_cancelled() {
                        return;
                    }
                    match result {
                        Ok(plan) if plan.is_empty() => {
                            app.notify(NotifyLevel::Info, "edit as text: no changes");
                        }
                        Ok(plan) => app.propose(plan),
                        Err(e) => app.notify(NotifyLevel::Error, format!("edit as text: {e}")),
                    }
                },
            );
        })
    };
    // The editor started in the pane's folder waits on its disk, a
    // server's for a remote folder: from the window, on a worker in the
    // folder's lane, a failure said.
    if skwir_core::scheduler::on_window_thread() {
        let weak = app.weak();
        app.scheduler.spawn_in(
            skwir_core::fs::reach::lane(&start_dir),
            Priority::Visible,
            "edit-as-buffer.start",
            started,
            move || {
                if let Err(e) = start()
                    && let Some(app) = weak.upgrade()
                {
                    app.notify(NotifyLevel::Error, format!("edit as text: {e:#}"));
                }
            },
        );
    } else {
        start()?;
    }
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

    fn category(&self) -> skwir_core::plugin::Category {
        skwir_core::plugin::Category::Files
    }

    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[
            Capability::Read,
            Capability::Write,
            Capability::Programs,
        ])
    }
    fn register(&self, app: &App) {
        let lifetime = app.lifetime().child();
        *self.lifetime.lock() = lifetime.clone();
        app.commands.register::<PaneArg, _>(
            ID,
            "edit_as_buffer",
            "Edit as text…",
            "Open the listing in $EDITOR; saving proposes a plan for review",
            move |a, app| start_session(a.pane, app, lifetime.clone()),
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
            MenuItem::new(
                "context",
                "Edit as Text…",
                "rename",
                205,
                Invocation::new("edit_as_buffer"),
            )
            .when("empty folder"),
        );
    }

    fn on_disable(&self, _app: &App) {
        self.lifetime.lock().cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[cfg(unix)]
    use std::time::Duration;

    /// Environment writes are unsafe in edition 2024; nextest runs each test
    /// in its own process and this runs before any thread is spawned.
    #[cfg(unix)]
    #[allow(unsafe_code)]
    fn set_env(key: &str, value: impl AsRef<OsStr>) {
        unsafe { std::env::set_var(key, value) };
    }

    fn scratch() -> tempfile::TempDir {
        // Trashed in some tests: on the home filesystem, like the user's files.
        let base = std::env::home_dir().unwrap().join(".cache/skwir-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::tempdir_in(base).unwrap()
    }

    fn refs(dir: &Path, names: &[(&str, bool)]) -> Original {
        names
            .iter()
            .map(|(n, d)| {
                let p = dir.join(n);
                if *d {
                    std::fs::create_dir_all(&p).unwrap();
                } else {
                    std::fs::write(&p, n.as_bytes()).unwrap();
                }
                (EntryRef::from_path(&p).unwrap(), OsString::from(n), *d)
            })
            .collect()
    }

    fn kinds(plan: &Plan) -> Vec<&'static str> {
        plan.steps.iter().map(|s| s.kind()).collect()
    }

    fn apply(plan: &Plan, dir: &Path) -> Arc<App> {
        let app = App::new(skwir_core::app::AppConfig::ephemeral(dir.join(".app"))).unwrap();
        let r = app.executor.execute_blocking(plan, app.lifetime());
        assert!(r.error.is_none(), "{:?}", r.error);
        app
    }

    fn read(p: PathBuf) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn buffer_format() {
        let b = to_buffer(&[
            (skwir_core::FileId::new(1, 1), "docs".into(), true),
            (skwir_core::FileId::new(1, 2), "a.txt".into(), false),
        ]);
        assert!(b.starts_with("# skwir"));
        assert!(b.ends_with("   1  docs/\n   2  a.txt\n"));
    }

    #[test]
    fn names_read_back_exactly() {
        let names: Vec<OsString> = [
            " notes",
            "notes ",
            "a\nb",
            "back\\slash",
            "tab\there",
            "été",
            "x\\x41",
            "#1",
        ]
        .into_iter()
        .map(OsString::from)
        // A name that is no UTF-8: Unix's alone.
        .chain(cfg!(unix).then(|| skwir_core::os_path::from_bytes(b"caf\xe9".to_vec())))
        .collect();
        for name in &names {
            let line = escape(name);
            assert!(!line.contains('\n') && line.trim() == line, "{line:?}");
            assert_eq!(
                &skwir_core::os_path::from_bytes(unescape(&line)),
                name,
                "{line:?}"
            );
        }
        assert_eq!(escape(OsStr::new("a b")), "a b", "inner spaces stay");
    }

    // Names Windows refuses (a trailing space, a backslash).
    #[cfg(unix)]
    #[test]
    fn an_unedited_buffer_changes_nothing_whatever_the_names() {
        let tmp = scratch();
        let dir = tmp.path();
        let orig = refs(dir, &[(" lead", false), ("trail ", false), ("a\\b", true)]);
        let buffer = to_buffer(
            &orig
                .iter()
                .map(|(r, n, d)| (r.id, n.clone(), *d))
                .collect::<Vec<_>>(),
        );
        assert!(diff_buffer(&orig, &buffer, dir).unwrap().is_empty());
    }

    #[test]
    fn diff_rename_move_delete_and_new_dir() {
        if skwir_core::system::under_wine("trashing through the shell") {
            return;
        }
        let tmp = scratch();
        let dir = tmp.path();
        let orig = refs(dir, &[("a.txt", false), ("b.txt", false), ("old", true)]);
        let edited = "# header\n   1  renamed.txt\n   2  archive/2024/b.txt\nphotos/\n";
        let plan = diff_buffer(&orig, edited, dir).unwrap();
        assert_eq!(
            kinds(&plan),
            ["trash", "rename", "mkdir", "mkdir", "mkdir", "move"]
        );
        apply(&plan, dir);
        assert!(
            dir.join("renamed.txt").exists()
                && dir.join("archive/2024/b.txt").exists()
                && dir.join("photos").is_dir()
                && !dir.join("old").exists()
        );
    }

    #[test]
    fn diff_swap_and_move_with_rename() {
        let tmp = scratch();
        let dir = tmp.path();
        let orig = refs(dir, &[("a", false), ("b", false), ("c", false)]);
        let plan = diff_buffer(&orig, "1  b\n2  a\n3  sub/renamed\n", dir).unwrap();
        assert_eq!(
            kinds(&plan),
            ["rename", "rename", "rename", "mkdir", "move", "rename"]
        );
        let app = apply(&plan, dir);
        assert_eq!(read(dir.join("a")), "b");
        assert_eq!(read(dir.join("b")), "a");
        assert_eq!(read(dir.join("sub/renamed")), "c");
        let u = app.executor.undo_blocking(app.lifetime()).unwrap();
        assert!(u.error.is_none(), "{:?}", u.error);
        assert_eq!(read(dir.join("a")), "a");
        assert!(dir.join("c").exists() && !dir.join("sub").exists());
    }

    #[test]
    fn a_name_freed_by_a_move_can_be_taken() {
        let tmp = scratch();
        let dir = tmp.path();
        let orig = refs(dir, &[("a", false), ("b", false)]);
        // a moves into a folder that does not exist yet, b takes its name.
        let plan = diff_buffer(&orig, "1  sub/a\n2  a\n", dir).unwrap();
        apply(&plan, dir);
        assert_eq!(read(dir.join("sub/a")), "a");
        assert_eq!(read(dir.join("a")), "b");
    }

    #[test]
    fn a_move_into_a_stable_folder_frees_its_name_first() {
        let tmp = scratch();
        let dir = tmp.path();
        let orig = refs(dir, &[("a", false), ("b", false), ("keep", true)]);
        let plan = diff_buffer(&orig, "1  keep/deep/a\n2  a\n3  keep/\n", dir).unwrap();
        assert_eq!(kinds(&plan), ["mkdir", "move", "rename"]);
        apply(&plan, dir);
        assert_eq!(read(dir.join("keep/deep/a")), "a");
        assert_eq!(read(dir.join("a")), "b");
    }

    #[test]
    fn a_move_into_a_renamed_folder_follows_the_new_name() {
        let tmp = scratch();
        let dir = tmp.path();
        let orig = refs(dir, &[("a", true), ("c", false)]);
        let plan = diff_buffer(&orig, "1  b/\n2  b/c\n", dir).unwrap();
        assert_eq!(kinds(&plan), ["rename", "move"], "no folder b is created");
        apply(&plan, dir);
        assert_eq!(read(dir.join("b/c")), "c");
    }

    #[test]
    fn a_folder_can_move_into_a_new_folder_of_its_own_name() {
        let tmp = scratch();
        let dir = tmp.path();
        let orig = refs(dir, &[("a", false)]);
        let plan = diff_buffer(&orig, "1  a/a\n", dir).unwrap();
        apply(&plan, dir);
        assert_eq!(read(dir.join("a/a")), "a");
    }

    #[test]
    fn a_hidden_file_in_the_way_is_reported() {
        let tmp = scratch();
        let dir = tmp.path();
        let orig = refs(dir, &[("a", false)]);
        std::fs::write(dir.join(".hidden"), b"").unwrap();
        assert_eq!(
            diff_buffer(&orig, "1  .hidden\n", dir).unwrap_err(),
            BufferError::Taken {
                line: 1,
                name: dir.join(".hidden").to_string_lossy().into_owned()
            }
        );
    }

    #[test]
    fn diff_errors() {
        let tmp = scratch();
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
        assert!(matches!(
            diff_buffer(&orig, "1  x\\x2fy\n2  b\n", dir).unwrap_err(),
            BufferError::InvalidName { .. }
        ));
        assert!(
            diff_buffer(&orig, "1  a\n2  b\n", dir).unwrap().is_empty(),
            "no change, empty plan"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_buffer_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let file = buffer_file("x").unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// A fake terminal that runs its `-e` arguments, and an editor that
    /// renames `one.txt` in the buffer once `gate` exists.
    // Shell scripts stand in for the programs (ADR-035).
    #[cfg(unix)]
    fn fake_tools(tmp: &Path, gate: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let bin = tmp.join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(
            bin.join("fake ed"),
            format!(
                "#!/bin/sh\n[ \"$1\" = --flag ] || exit 3\nwhile [ ! -e '{}' ]; do sleep 0.01; done\nsed -i 's/one.txt/two.txt/' \"$2\"\n",
                gate.display()
            ),
        )
        .unwrap();
        std::fs::write(bin.join("faketerm"), "#!/bin/sh\nshift\nexec \"$@\"\n").unwrap();
        for f in ["fake ed", "faketerm"] {
            std::fs::set_permissions(bin.join(f), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        set_env("TERMINAL", bin.join("faketerm"));
        // Quoted, with an argument: read by the shell like git does.
        set_env(
            "VISUAL",
            format!("'{}' --flag", bin.join("fake ed").display()),
        );
        bin
    }

    #[cfg(unix)]
    fn session(tmp: &Path) -> (Arc<App>, PathBuf) {
        let dir = tmp.join("work");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("one.txt"), b"1").unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.join("app"))).unwrap();
        app.plugins
            .add(Arc::new(EditAsBufferPlugin::default()), &app);
        let pane = app.workspace.read().active_id();
        app.navigate(pane, &skwir_core::Location::from_path(&dir))
            .unwrap();
        assert!(app.pump_until_complete(pane, Duration::from_secs(5)));
        (app, dir)
    }

    #[cfg(unix)]
    #[test]
    fn end_to_end_with_a_fake_editor_and_terminal() {
        let tmp = scratch();
        let gate = tmp.path().join("gate");
        std::fs::write(&gate, b"").unwrap();
        fake_tools(tmp.path(), &gate);
        let (app, dir) = session(tmp.path());
        app.dispatch(&Invocation::new("edit_as_buffer")).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let plan = loop {
            if let Some(p) = app.take_proposals().pop() {
                break p;
            }
            let notes = app.take_notifications();
            assert!(
                notes.is_empty(),
                "{:?}",
                notes.iter().map(|n| &n.text).collect::<Vec<_>>()
            );
            assert!(std::time::Instant::now() < deadline, "no plan proposed");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(kinds(&plan), ["rename"]);
        assert!(dir.join("one.txt").exists(), "proposing never applies");
    }

    #[cfg(unix)]
    #[test]
    fn a_session_ending_after_disable_proposes_nothing() {
        let tmp = scratch();
        let gate = tmp.path().join("gate");
        fake_tools(tmp.path(), &gate);
        let (app, _dir) = session(tmp.path());
        app.dispatch(&Invocation::new("edit_as_buffer")).unwrap();
        app.plugins.set_enabled(ID, false, &app);
        std::fs::write(&gate, b"").unwrap();
        // Wait for the editor to be done with the buffer, then some.
        std::thread::sleep(Duration::from_millis(500));
        assert!(app.take_proposals().is_empty());
        app.plugins.set_enabled(ID, true, &app);
        assert!(app.commands.get("edit_as_buffer").is_some());
    }

    proptest::proptest! {
        /// Whatever the user leaves in the buffer, it is read as a plan or
        /// refused in words: never a panic.
        #[test]
        fn any_buffer_the_user_leaves_is_read_without_a_panic(text in proptest::prop_oneof![
            skwir_core::testing::mutated_text(
                "# a comment\n1  café.txt\n2  sub/\n3  \"a b\\\"c\".md\n  new folder/\n4  ../x\n"
            ),
            proptest::prelude::any::<String>(),
        ]) {
            thread_local!(static HERE: (tempfile::TempDir, Original) = {
                let dir = scratch();
                let original = refs(dir.path(), &[("café.txt", false), ("sub", true), ("a b", false)]);
                (dir, original)
            });
            HERE.with(|(dir, original)| {
                let _ = diff_buffer(original, &text, dir.path());
            });
        }
    }
}
