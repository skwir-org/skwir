//! What a permanent deletion takes, listed in the question asked before it
//! (`Shell::apply_user_plan`): each item by the name and in the folder the
//! user knows it by (an item of a trash as it was named, where it came
//! from), what it is, what it weighs or holds, and the companion files
//! that go with it; under them, where they are and what they hold in all.
//!
//! The rows are the plan's, made from its paths when it asks; a worker
//! reads the rest (what each is, a trash's records, the companions), then
//! counts what each holds (`fs::count::count_each`), handing over what it
//! has every `count::REPORT_EVERY`. Only the rows on screen are drawn: an
//! emptied trash lists thousands. Closing the question stops the worker.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    AnyElement, Context, SharedString, UniformListScrollHandle, Window, div, prelude::*, px,
};
use skwir_core::fs::count::{self, Count};
use skwir_core::op::{Change, PlanId};
use skwir_core::{CancelToken, Plan};
use skwir_platform::calendar::Calendar;

use crate::controls;
use crate::elements::adaptive::AdaptiveRow;
use crate::elements::clip::clipped;
use crate::format;
use crate::modal::{ListMove, Rows};
use crate::prompts::Confirmed;
use crate::shell::Shell;
use crate::theme::Theme;

/// A row's height, and the rows shown before the list scrolls.
const ROWS: Rows = Rows {
    height: 24.,
    shown: 8,
};

/// The width of the folder each row is in, where they are not all in one.
const FOLDER: f32 = 200.;

/// What a row is, once read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// What is not a folder or a link (a pipe too), and its size.
    File(u64),
    Folder,
    /// The link goes, never what it leads to.
    Link,
    /// Not there any more: there is nothing to delete.
    Gone,
}

/// What the worker reads of a row before it counts.
struct Facts {
    kind: Kind,
    /// Where a trash says its item was put there from.
    from: Option<PathBuf>,
    /// The companion files deleted with it.
    companions: Vec<PathBuf>,
}

/// What the worker hands over.
enum Found {
    /// What rows are.
    Read(Vec<(usize, Facts)>),
    /// What the folders among the rows hold, counted so far, and what all
    /// of it holds; `done` once the count is over.
    Counted {
        folders: Vec<(usize, Count)>,
        total: Count,
        done: bool,
    },
}

struct Row {
    name: SharedString,
    /// The folder it is in, written as the address takes it (`~/…`); for
    /// an item of a trash, the one it came from once read.
    folder: Option<SharedString>,
    /// In a trash: its name and folder are what its record says.
    trashed: bool,
    /// What a step that deletes nothing says it does: shown as it is.
    other: bool,
    kind: Option<Kind>,
    /// The companion files deleted with it.
    companions: Vec<PathBuf>,
    /// What a folder holds, its companions with it, counted so far.
    count: Option<Count>,
}

/// The rows of a question before a permanent deletion, and what is known
/// of them so far.
pub struct Deleting {
    plan: PlanId,
    rows: Vec<Row>,
    /// The folder every row is in, known when asked: said once, under the
    /// list, rather than on each row.
    one_folder: Option<SharedString>,
    /// Each row says its folder: they are not all in one.
    folders: bool,
    /// The rows that are folders counted by themselves, once the count is
    /// over: a folder counts what it holds, not itself.
    root_folders: usize,
    total: Count,
    counted: bool,
    home: Option<PathBuf>,
    /// Each remote's mount and address: a folder there is said by its
    /// address, not the mount's path (ADR-077).
    remotes: Vec<(PathBuf, skwir_core::Location)>,
    scroll: UniformListScrollHandle,
    cancel: CancelToken,
}

impl Drop for Deleting {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// `dir` written as the address takes it: the home folder as `~`, a
/// remote's folder as its address (`sftp://me@host/docs`).
fn written(
    dir: &Path,
    home: Option<&Path>,
    remotes: &[(PathBuf, skwir_core::Location)],
) -> SharedString {
    if let Some((point, address)) = remotes.iter().find(|(point, _)| dir.starts_with(point))
        && let Ok(below) = dir.strip_prefix(point)
    {
        return skwir_core::remotes::address_below(address, below)
            .to_string()
            .into();
    }
    crate::complete::written(dir, "~", home).into()
}

fn name_of(path: &Path) -> SharedString {
    path.file_name()
        .map_or_else(|| path.to_string_lossy(), |name| name.to_string_lossy())
        .into_owned()
        .into()
}

impl Deleting {
    /// The rows of `plan`, from its paths alone, and the paths the worker
    /// reads, by row.
    fn of(
        plan: &Plan,
        remotes: Vec<(PathBuf, skwir_core::Location)>,
        cancel: CancelToken,
    ) -> (Self, Vec<(usize, PathBuf)>) {
        let home = skwir_core::os_path::home();
        let mut rows = Vec::with_capacity(plan.len());
        let mut paths = Vec::with_capacity(plan.len());
        // A folder is written once for the rows one after another in it.
        let mut last: Option<(PathBuf, SharedString)> = None;
        for (at, step) in plan.steps.iter().enumerate() {
            let path = match step.describe() {
                Change::Delete { path } => path,
                other => {
                    rows.push(Row {
                        name: other.to_string().into(),
                        folder: None,
                        trashed: false,
                        other: true,
                        kind: None,
                        companions: Vec::new(),
                        count: None,
                    });
                    continue;
                }
            };
            // Read from the path alone: what its record says comes later.
            let trashed = skwir_core::op::trash_info_of(&path).is_some();
            let folder = path.parent().filter(|_| !trashed).map(|dir| match &last {
                Some((was, said)) if was == dir => said.clone(),
                _ => {
                    let said = written(dir, home.as_deref(), &remotes);
                    last = Some((dir.to_path_buf(), said.clone()));
                    said
                }
            });
            rows.push(Row {
                name: name_of(&path),
                folder,
                trashed,
                other: false,
                kind: None,
                companions: Vec::new(),
                count: None,
            });
            paths.push((at, path));
        }
        let mut known = rows.iter().filter(|r| !r.trashed && !r.other);
        let first = known.next().and_then(|r| r.folder.clone());
        let same = known.all(|r| r.folder == first);
        let trashed = rows.iter().filter(|r| r.trashed).count();
        let (one_folder, folders) = match (trashed, same) {
            (0, true) => (first, false),
            // Where an item of a trash came from is read later: several
            // say theirs, one says it under the list.
            (1, true) if rows.len() == 1 => (None, false),
            _ => (None, rows.len() > 1),
        };
        let deleting = Deleting {
            plan: plan.id,
            rows,
            one_folder,
            folders,
            root_folders: 0,
            total: Count::default(),
            counted: paths.is_empty(),
            home,
            remotes,
            scroll: UniformListScrollHandle::new(),
            cancel,
        };
        (deleting, paths)
    }

    /// What is asked: the item deleted by its name when there is one (an
    /// item of a trash only once its record is read, so by a count), else
    /// how many, the plan's other steps left out (what a trash command
    /// moves to the trash beside what it deletes on a remote); then what
    /// the plan's maker could tell of what stays.
    fn question(&self, note: Option<&str>) -> String {
        let deleted: Vec<&Row> = self.rows.iter().filter(|r| !r.other).collect();
        let rows = if deleted.is_empty() {
            self.rows.iter().collect()
        } else {
            deleted
        };
        let trash = !rows.is_empty() && rows.iter().all(|r| r.trashed);
        let what = match rows.as_slice() {
            [one] if !one.trashed && !one.other => format!("“{}”", one.name),
            rows => format::count(rows.len(), "item"),
        };
        let from = if trash { " from the trash" } else { "" };
        let note = note.map_or(String::new(), |note| format!(" {note}."));
        format!("Delete {what}{from} permanently? This cannot be undone.{note}")
    }

    fn take(&mut self, found: Found) {
        match found {
            Found::Read(facts) => {
                for (at, facts) in facts {
                    let row = &mut self.rows[at];
                    if let Some(from) = &facts.from {
                        row.name = name_of(from);
                        row.folder = from
                            .parent()
                            .map(|dir| written(dir, self.home.as_deref(), &self.remotes));
                    }
                    row.kind = Some(facts.kind);
                    row.companions = facts.companions;
                }
            }
            Found::Counted {
                folders,
                total,
                done,
            } => {
                for (at, count) in folders {
                    self.rows[at].count = Some(count);
                }
                self.total = total;
                if done {
                    // Those counted by themselves: one inside another is
                    // among what that one holds.
                    self.root_folders = self
                        .rows
                        .iter()
                        .filter(|r| r.kind == Some(Kind::Folder) && r.count.is_some())
                        .count();
                    self.counted = true;
                }
            }
        }
    }

    /// What a row weighs or holds, as it is known so far.
    fn weight(&self, row: &Row) -> String {
        match row.kind {
            None => String::new(),
            Some(Kind::Gone) => "not there".into(),
            Some(Kind::Link) => "link".into(),
            Some(Kind::File(bytes)) => format::size(bytes),
            Some(Kind::Folder) => match row.count {
                // Inside another folder listed: counted there.
                None if self.counted => "folder".into(),
                None => "…".into(),
                Some(c) if !c.done => format!("{}…", format::count(c.files as usize, "file")),
                Some(c) if c.files == 0 && c.folders == 0 => "empty".into(),
                Some(c) => format!(
                    "{}{} · {}",
                    if at_least(&c) { "at least " } else { "" },
                    format::size(c.size),
                    format::count(c.files as usize, "file")
                ),
            },
        }
    }

    /// What the mark after a row's name says when pointed at: the
    /// companion files that go with it, each where it is, as the address
    /// writes it. None when none goes.
    fn deleted_with(&self, row: &Row) -> Option<SharedString> {
        if row.companions.is_empty() {
            return None;
        }
        let mut said = String::from("Deleted with it:");
        for path in &row.companions {
            said.push('\n');
            said.push_str(&written(path, self.home.as_deref(), &self.remotes));
        }
        Some(said.into())
    }

    /// Under the list: where the rows are, when one folder holds them all,
    /// and what they hold in all, or how far the count is.
    fn summary(&self) -> String {
        let all_trashed = self.rows.iter().all(|r| r.trashed);
        let place = match self.rows.as_slice() {
            [one] if one.trashed => Some(match &one.folder {
                Some(from) => format!("In the trash, from {from}"),
                None => "In the trash".to_owned(),
            }),
            _ if all_trashed => Some("In the trash".to_owned()),
            _ => self.one_folder.as_ref().map(|dir| format!("In {dir}")),
        };
        let held = self.held();
        match place {
            Some(place) if held.is_empty() => place,
            Some(place) => format!("{place} · {held}"),
            None => held,
        }
    }

    /// What the rows hold in all, once counted; how far the count is
    /// until then.
    fn held(&self) -> String {
        let total = &self.total;
        if !self.counted {
            return if total.size > 0 {
                format!("Counting… {} so far", format::size(total.size))
            } else {
                "Counting…".to_owned()
            };
        }
        let folders = total.folders as usize + self.root_folders;
        let said = match (total.files as usize, folders) {
            (0, 0) if self.rows.iter().all(|r| r.kind == Some(Kind::Gone)) => {
                return "Already gone".to_owned();
            }
            (0, 0) => return String::new(),
            (0, folders) => format::count(folders, "empty folder"),
            (files, 0) => format!(
                "{} in {}",
                format::size(total.size),
                format::count(files, "file")
            ),
            (files, folders) => format!(
                "{} in {} and {}",
                format::size(total.size),
                format::count(files, "file"),
                format::count(folders, "folder")
            ),
        };
        if at_least(total) {
            format!("At least {said}")
        } else {
            said
        }
    }
}

/// What a count says less than there is of: not over, something it could
/// not read, or a drive mounted inside, which the walk leaves out.
fn at_least(count: &Count) -> bool {
    count.lower_bound() || count.other_drives > 0
}

/// Read what each of `paths` is, then count what they hold, telling
/// `send` as it goes, until `cancel` stops it.
fn read(
    paths: &[(usize, PathBuf)],
    companions: &skwir_core::companions::Companions,
    cancel: &CancelToken,
    send: impl Fn(Found),
) {
    let calendar = Calendar::system();
    let mut facts = Vec::new();
    let mut last = Instant::now();
    // What is counted: each path, then its companions; for each root the
    // path whose deletion takes it, and for each path its roots and, for a
    // folder, its row, whose count is shown.
    let mut roots: Vec<PathBuf> = Vec::with_capacity(paths.len());
    let mut owner: Vec<usize> = Vec::with_capacity(paths.len());
    let mut taken: Vec<(Option<usize>, Range<usize>)> = Vec::with_capacity(paths.len());
    for (row, path) in paths {
        if cancel.is_cancelled() {
            return;
        }
        let kind = match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => Kind::Link,
            Ok(meta) if meta.is_dir() => Kind::Folder,
            Ok(meta) => Kind::File(meta.len()),
            Err(_) => Kind::Gone,
        };
        let found = if kind == Kind::Gone {
            Vec::new()
        } else {
            companions
                .existing(path)
                .into_iter()
                .map(|c| c.path)
                .collect()
        };
        let start = roots.len();
        if kind != Kind::Gone {
            roots.push(path.clone());
        }
        roots.extend(found.iter().cloned());
        owner.resize(roots.len(), taken.len());
        taken.push(((kind == Kind::Folder).then_some(*row), start..roots.len()));
        facts.push((
            *row,
            Facts {
                kind,
                from: skwir_platform::trash::item(path, &calendar).map(|t| t.original),
                companions: found,
            },
        ));
        if last.elapsed() >= count::REPORT_EVERY {
            send(Found::Read(std::mem::take(&mut facts)));
            last = Instant::now();
        }
    }
    send(Found::Read(facts));

    // What each root holds so far, and the folders among the rows whose
    // count changed since it was last said. A folder none of whose roots
    // has a count is not said: not reached yet, or (once the count is
    // over) counted with a folder holding it.
    let mut each: Vec<Option<Count>> = vec![None; roots.len()];
    let mut changed: Vec<usize> = Vec::new();
    let said = |each: &[Option<Count>], changed: &mut Vec<usize>, done: bool| {
        let folders = changed
            .drain(..)
            .filter_map(|at| {
                let (row, range) = &taken[at];
                let (row, roots) = ((*row)?, &each[range.clone()]);
                roots
                    .iter()
                    .any(Option::is_some)
                    .then(|| (row, held(roots)))
            })
            .collect();
        Found::Counted {
            folders,
            total: held(each),
            done,
        }
    };
    let mut last = Instant::now();
    let counted = count::count_each(&roots, cancel, count::REPORT_EVERY, |at, now| {
        each[at] = Some(*now);
        let row = owner[at];
        if changed.last() != Some(&row) {
            changed.push(row);
        }
        if last.elapsed() >= count::REPORT_EVERY {
            send(said(&each, &mut changed, false));
            last = Instant::now();
        }
    });
    if cancel.is_cancelled() {
        return;
    }
    // Every folder once more: one counted with another says so now.
    changed = (0..taken.len())
        .filter(|&at| taken[at].0.is_some())
        .collect();
    send(said(&counted, &mut changed, true));
}

/// What `roots` hold together: done once each counted by itself is.
fn held(roots: &[Option<Count>]) -> Count {
    let mut all = Count {
        done: true,
        ..Count::default()
    };
    for count in roots.iter().flatten() {
        all.add(count);
        all.done &= count.done;
    }
    all
}

impl Shell {
    /// Ask before applying `plan`, which Undo cannot take back, listing
    /// what it deletes.
    pub fn confirm_deleting(&mut self, plan: Plan, window: &mut Window, cx: &mut Context<Self>) {
        let cancel = self.app.lifetime().child();
        let remotes = self
            .app
            .registry
            .remotes
            .all()
            .into_iter()
            .filter_map(|known| Some((known.point, known.address?)))
            .collect();
        let (listed, paths) = Deleting::of(&plan, remotes, cancel.clone());
        let question = listed.question(plan.note.as_deref());
        let id = plan.id;
        self.open_confirm(question, Confirmed::Apply(plan), window, cx);
        if let Some(confirm) = self.confirm.as_mut() {
            confirm.listed = Some(listed);
        }
        if paths.is_empty() {
            return;
        }
        let companions = Arc::clone(&self.app.companions);
        let (tx, rx) = flume::unbounded();
        let token = cancel.clone();
        self.app.scheduler.spawn(
            skwir_core::scheduler::Priority::Visible,
            "deleting",
            cancel,
            move || {
                read(&paths, &companions, &token, |found| {
                    let _ = tx.send(found);
                });
            },
        );
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(found) = rx.recv_async().await {
                let asking = this.update_in(cx, |shell, _, cx| {
                    let Some(listed) = shell.deleting_mut().filter(|l| l.plan == id) else {
                        return false;
                    };
                    listed.take(found);
                    cx.notify();
                    true
                });
                if !matches!(asking, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    fn deleting(&self) -> Option<&Deleting> {
        self.confirm.as_ref()?.listed.as_ref()
    }

    fn deleting_mut(&mut self) -> Option<&mut Deleting> {
        self.confirm.as_mut()?.listed.as_mut()
    }

    /// Up, Down and the page keys while a question has the keyboard
    /// (`ui.list_move`): they scroll what it lists, and reach nothing
    /// behind it. False when none has it (a palette opened over it).
    pub(crate) fn confirm_move(
        &mut self,
        mv: ListMove,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(confirm) = self
            .confirm
            .as_ref()
            .filter(|c| c.focus.contains_focused(window, cx))
        else {
            return false;
        };
        if let Some(listed) = &confirm.listed {
            ROWS.scroll(&listed.scroll, mv);
            cx.notify();
        }
        true
    }
}

/// The list of what is deleted, and the line under it.
pub fn render(
    listed: &Deleting,
    shell: &Shell,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> AnyElement {
    let count = listed.rows.len();
    let list = ROWS.list(
        "deleting",
        count,
        &listed.scroll,
        theme,
        |shell, range, _, cx| rows(shell, range, cx),
        cx,
    );
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(list.role(gpui::Role::List).when(shell.a11y, |d| {
            d.aria_label("What is deleted").aria_size_of_set(count)
        }))
        .child(
            div()
                .min_h(px(16.))
                .text_xs()
                .text_color(theme.text_muted)
                .truncate()
                .child(listed.summary()),
        )
        .into_any_element()
}

/// The rows in `range`, the only ones drawn.
fn rows(shell: &Shell, range: Range<usize>, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    let Some(listed) = shell.deleting() else {
        return Vec::new();
    };
    let theme = cx.global::<Theme>();
    range
        .filter_map(|at| {
            let row = listed.rows.get(at)?;
            let weight = listed.weight(row);
            let glyph = match row.kind {
                Some(Kind::Folder) => Some("folder"),
                Some(Kind::Link) => Some("link"),
                _ => None,
            };
            let folder = row.folder.clone().filter(|_| listed.folders);
            let said = shell.a11y.then(|| {
                let with = (!row.companions.is_empty()).then(|| {
                    let names: Vec<_> = row.companions.iter().map(|c| name_of(c)).collect();
                    format!("deleted with it: {}", names.join(", "))
                });
                [
                    Some(row.name.to_string()),
                    folder.as_ref().map(ToString::to_string),
                    with,
                    Some(weight.clone()),
                ]
                .into_iter()
                .flatten()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ")
            });
            Some(
                div()
                    .id(("deleting-row", at))
                    .role(gpui::Role::ListItem)
                    .when_some(said, |d, said| d.aria_label(said).aria_position_in_set(at))
                    .w_full()
                    .h(px(ROWS.height))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_sm()
                    .text_color(if row.kind == Some(Kind::Gone) || row.other {
                        theme.text_muted
                    } else {
                        theme.text
                    })
                    .child(
                        div()
                            .w(px(14.))
                            .flex_none()
                            .children(glyph.map(|g| controls::glyph(g, theme.text_muted, 14.))),
                    )
                    .child(name(at, &row.name, listed.deleted_with(row), theme))
                    .children(folder.map(|folder| {
                        clipped(
                            div()
                                .id(("deleting-folder", at))
                                .w(px(FOLDER))
                                .flex_none()
                                .text_xs()
                                .text_color(theme.text_muted)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis_middle(),
                            folder,
                        )
                    }))
                    .child(
                        div()
                            .w(px(120.))
                            .flex_none()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .text_right()
                            .truncate()
                            .child(weight),
                    )
                    .into_any_element(),
            )
        })
        .collect()
}

/// A row's name and, when companion files go with it, a mark after it
/// that says where they are when pointed at (`with`): "+ metadata" while
/// the whole name fits beside it, else "+", the name taking the room and
/// cut only once the mark is that small, in its middle as the views cut
/// names (the extension tells files apart).
fn name(at: usize, name: &SharedString, with: Option<SharedString>, theme: &Theme) -> AnyElement {
    let named = || {
        clipped(
            div()
                .id(("deleting-name", at))
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle(),
            name.clone(),
        )
    };
    let Some(with) = with else {
        return div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .child(named())
            .into_any_element();
    };
    let marked = |mark: &'static str| {
        let with = with.clone();
        div()
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .gap_1()
            .child(named())
            .child(
                controls::badge(mark, theme.text_muted, theme)
                    .id(("deleting-with", at))
                    .tooltip(move |_, cx| crate::toolbar::tooltip(with.clone(), cx)),
            )
            .into_any_element()
    };
    div()
        .flex_1()
        .min_w_0()
        .child(AdaptiveRow {
            variants: vec![marked("+ metadata"), marked("+")],
            height: ROWS.height,
            chosen: None,
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::path::PathBuf;

    use skwir_core::companions::{Companion, Companions};
    use skwir_core::op::{Delete, EntryRef, Operation, Trash};
    use skwir_core::{CancelToken, FileId, Plan};

    /// A remote's folder is said by its address, not its mount's path
    /// (ADR-077).
    #[test]
    fn a_remote_folder_is_said_by_its_address() {
        let point = PathBuf::from("/run/user/1000/skwir/remote/sshfs x");
        let remotes = [(
            point.clone(),
            skwir_core::Location::new("sftp", "//me@example.com/"),
        )];
        assert_eq!(
            written(&point.join("home/Two Words"), None, &remotes).as_ref(),
            "sftp://me@example.com/home/Two Words"
        );
        assert_eq!(
            written(std::path::Path::new("/data"), None, &remotes).as_ref(),
            "/data"
        );
    }

    use super::{Deleting, Kind, read, written};

    /// `x.xmp` beside `x`.
    struct Sidecar;

    impl Companion for Sidecar {
        fn id(&self) -> &'static str {
            "test"
        }
        fn of(&self, name: &OsStr) -> Vec<PathBuf> {
            let mut xmp = name.to_os_string();
            xmp.push(".xmp");
            vec![xmp.into()]
        }
        fn owner(&self, name: &OsStr) -> Option<OsString> {
            name.to_str()?.strip_suffix(".xmp").map(Into::into)
        }
    }

    fn delete(path: PathBuf) -> Box<dyn Operation> {
        Box::new(Delete {
            entry: EntryRef::new(FileId::new(1, 1), path),
        })
    }

    fn plan(paths: Vec<PathBuf>) -> Plan {
        Plan::with_steps(
            "delete permanently",
            paths.into_iter().map(delete).collect(),
        )
    }

    /// The question names one item, counts several, and says which come
    /// from the trash; the folder they share is said once.
    #[test]
    fn the_question_names_what_it_deletes() {
        let dir = PathBuf::from("/data/it's a café");
        let one = plan(vec![dir.join("notes  draft.txt")]);
        let (listed, paths) = Deleting::of(&one, Vec::new(), CancelToken::new());
        assert_eq!(
            listed.question(None),
            "Delete “notes  draft.txt” permanently? This cannot be undone."
        );
        assert_eq!(paths.len(), 1);
        assert_eq!(listed.summary(), "In /data/it's a café · Counting…");
        assert!(!listed.folders);

        let several = plan(vec![dir.join("a"), dir.join("b")]);
        let (listed, _) = Deleting::of(&several, Vec::new(), CancelToken::new());
        assert_eq!(
            listed.question(Some("1 of these has other names: its file stays")),
            "Delete 2 items permanently? This cannot be undone. \
             1 of these has other names: its file stays."
        );
        assert!(!listed.folders, "one folder holds them");

        let apart = plan(vec![dir.join("a"), PathBuf::from("/elsewhere/b")]);
        let (listed, _) = Deleting::of(&apart, Vec::new(), CancelToken::new());
        assert!(listed.folders && listed.one_folder.is_none());

        // Items of a trash: by count, their folders read from their records.
        let trash = PathBuf::from("/data/.Trash-1000/files");
        let trashed = plan(vec![trash.join("a.txt"), trash.join("b.txt")]);
        let (listed, _) = Deleting::of(&trashed, Vec::new(), CancelToken::new());
        assert_eq!(
            listed.question(None),
            "Delete 2 items from the trash permanently? This cannot be undone."
        );
        assert!(listed.folders && listed.rows.iter().all(|r| r.folder.is_none()));

        // A trash command whose selection is partly on a remote, which
        // keeps no trash: what it deletes is asked about, the rest said.
        let mut mixed = plan(vec![dir.join("on the server.txt")]);
        mixed.steps.push(Box::new(Trash {
            entry: EntryRef::new(FileId::new(1, 2), dir.join("here.txt")),
        }));
        let (listed, _) = Deleting::of(&mixed, Vec::new(), CancelToken::new());
        assert_eq!(
            listed.question(Some(
                "It is in a network folder, which keeps no trash; the other item goes to the trash"
            )),
            "Delete “on the server.txt” permanently? This cannot be undone. \
             It is in a network folder, which keeps no trash; the other item goes to the trash."
        );
    }

    /// What the worker reads: what each is, its companions, what the
    /// folders hold; the line under the list adds it up.
    #[test]
    fn the_rows_say_what_each_is_and_holds() {
        let tmp = tempfile::tempdir().unwrap();
        let folder = tmp.path().join("old  project");
        std::fs::create_dir_all(folder.join("src")).unwrap();
        std::fs::write(folder.join("src/main.rs"), vec![b'x'; 2000]).unwrap();
        std::fs::write(folder.join("README"), vec![b'x'; 48]).unwrap();
        let photo = tmp.path().join("it's a photo.jpg");
        std::fs::write(&photo, vec![b'p'; 500]).unwrap();
        std::fs::write(tmp.path().join("it's a photo.jpg.xmp"), vec![b'm'; 20]).unwrap();
        let empty = tmp.path().join("empty");
        std::fs::create_dir(&empty).unwrap();
        let gone = tmp.path().join("gone");
        // Inside the folder listed first: counted there.
        let inner = tmp.path().join("old  project/src");
        let all = plan(vec![folder, photo, empty, gone, inner]);
        let (mut listed, paths) = Deleting::of(&all, Vec::new(), CancelToken::new());
        // Not all in one folder: each row says its own.
        assert!(listed.folders);
        assert_eq!(listed.summary(), "Counting…");
        let companions = Companions::default();
        companions.register("test", std::sync::Arc::new(Sidecar));
        let found = std::sync::Mutex::new(Vec::new());
        read(&paths, &companions, &CancelToken::new(), |f| {
            found.lock().unwrap().push(f);
        });
        for f in found.into_inner().unwrap() {
            listed.take(f);
        }
        assert!(listed.counted);
        let kinds: Vec<_> = listed.rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [
                Some(Kind::Folder),
                Some(Kind::File(500)),
                Some(Kind::Folder),
                Some(Kind::Gone),
                Some(Kind::Folder),
            ]
        );
        let weights: Vec<_> = listed.rows.iter().map(|r| listed.weight(r)).collect();
        assert_eq!(
            weights,
            ["2.0 KB · 2 files", "500 B", "empty", "not there", "folder"]
        );
        let home = skwir_core::os_path::home();
        let xmp = tmp.path().join("it's a photo.jpg.xmp");
        assert_eq!(listed.rows[1].companions, std::slice::from_ref(&xmp));
        assert_eq!(
            listed.deleted_with(&listed.rows[1]).as_deref(),
            Some(format!("Deleted with it:\n{}", written(&xmp, home.as_deref(), &[])).as_str()),
            "the mark after its name says where its companions are"
        );
        assert_eq!(listed.deleted_with(&listed.rows[0]), None);
        // The companion goes too; the gone one is not there to count, so
        // the sum is no lower bound.
        assert_eq!(listed.summary(), "2.5 KB in 4 files and 3 folders");
        let place = written(tmp.path(), home.as_deref(), &[]);
        assert_eq!(listed.rows[1].folder.as_ref(), Some(&place));
        assert_eq!(listed.total.size, 2000 + 48 + 500 + 20);
    }

    /// An item of a trash is listed as it was named, from where it was
    /// put there, as its record says: the trash renamed it beside another
    /// of its name.
    #[test]
    fn an_item_of_a_trash_is_named_as_it_came() {
        let tmp = tempfile::tempdir().unwrap();
        let trash = tmp.path().join(".Trash-1000");
        std::fs::create_dir_all(trash.join("files")).unwrap();
        std::fs::create_dir_all(trash.join("info")).unwrap();
        let item = trash.join("files/it's a note~1.txt");
        std::fs::write(&item, b"note").unwrap();
        std::fs::write(
            trash.join("info/it's a note~1.txt.trashinfo"),
            "[Trash Info]\nPath=docs/caf%C3%A9/it%27s%20a%20note.txt\nDeletionDate=2026-10-19T10:00:00\n",
        )
        .unwrap();
        let (mut listed, paths) = Deleting::of(&plan(vec![item]), Vec::new(), CancelToken::new());
        assert_eq!(
            listed.question(None),
            "Delete 1 item from the trash permanently? This cannot be undone."
        );
        assert_eq!(listed.summary(), "In the trash · Counting…");
        let found = std::sync::Mutex::new(Vec::new());
        read(&paths, &Companions::default(), &CancelToken::new(), |f| {
            found.lock().unwrap().push(f);
        });
        for f in found.into_inner().unwrap() {
            listed.take(f);
        }
        assert_eq!(listed.rows[0].name.as_ref(), "it's a note.txt");
        let from = written(
            &tmp.path().join("docs/café"),
            skwir_core::os_path::home().as_deref(),
            &[],
        );
        assert_eq!(
            listed.summary(),
            format!("In the trash, from {from} · 4 B in 1 file")
        );
    }
}
