//! Links followed from what skwir shows (ADR-029): a document's own, an
//! address found in raw text, a link of the terminal.
//!
//! A link the text was written with answers a click, as on a page. One
//! found in raw text (source, a log, the terminal's output), where a click
//! selects, answers Ctrl+click (Cmd+click) and is drawn as a link only
//! while Ctrl is held over it (`Run::found`). Shift added shows a file's
//! link in a tab of its own. An address goes to the browser or the mail
//! client through one gate (`ui.open_url`, `open::open_address`, on a
//! worker); a file or a folder is shown in skwir (`ui.reveal`), never
//! opened, so a link a file wrote cannot run anything, and a line number
//! shows that line in the preview. The keyboard reaches the same links:
//! Ctrl+Shift+O labels those in view (`labels`).

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Context, ElementId, ModifiersChangedEvent, Window};
use skwir_core::os_path::PathArg;
use skwir_core::terminal::Target;

use crate::shell::Shell;

/// How long a line asked for waits for its file's preview.
const GOTO_WAIT: Duration = Duration::from_secs(10);

/// Links as the pointer and the modifiers left them.
#[derive(Default)]
pub struct LinkState {
    /// The found link the pointer rests on: the text it is drawn in, and
    /// its range there (`inspector_text::part`).
    pub(crate) hover: Option<(ElementId, Range<usize>)>,
    /// Ctrl (Cmd) is held, as the last event said.
    pub(crate) held: bool,
    /// A place to show once the preview of its file arrives.
    goto: Option<Goto>,
}

struct Goto {
    /// The pane the file was revealed in: its preview is the one to wait
    /// for, not another pane's of the same file.
    pane: skwir_core::workspace::PaneId,
    path: PathBuf,
    /// The member of the container at `path` the place is in, if any
    /// (`Nested::target`).
    member: Option<String>,
    mark: Mark,
    until: Instant,
}

/// Where in a file its preview shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A line, from 1.
    Line(u32),
    /// What a link's fragment names (`#setup`): a mark the text holds (an
    /// `id`, a heading's), or a line as code hosts write one (`#L12`).
    Anchor(String),
}

/// A link of a preview labelled: the block it is in, where it starts
/// there, its label, and where it leads.
pub(crate) struct Labelled {
    pub block: usize,
    pub at: usize,
    pub label: String,
    pub target: String,
    pub note: bool,
}

/// A preview's links in view, labelled (Ctrl+Shift+O), and what was typed
/// of a label so far.
pub(crate) struct Labels {
    pub links: Vec<Labelled>,
    pub typed: String,
}

impl Labels {
    /// What a frame draws: by block, the labels what was typed still
    /// matches, each at the start of its link.
    pub fn shown(&self) -> std::collections::HashMap<usize, Vec<(usize, gpui::SharedString)>> {
        let mut shown: std::collections::HashMap<usize, Vec<_>> = std::collections::HashMap::new();
        for link in self
            .links
            .iter()
            .filter(|link| link.label.starts_with(self.typed.as_str()))
        {
            shown
                .entry(link.block)
                .or_default()
                .push((link.at, link.label.clone().into()));
        }
        shown
    }
}

/// Where `ui.reveal` shows a file: in a tab of its own or in the pane, and
/// where its preview shows it.
#[derive(Clone, Debug, Default)]
pub struct RevealAt {
    pub tab: bool,
    pub mark: Option<Mark>,
    /// The folder of the document the link was read in: the file is looked
    /// up only where that stays on this machine from there
    /// (`fs::volume::Here`), never on a server a hostile file names.
    pub from: Option<PathBuf>,
}

/// `ui.reveal`'s arguments: a file shown in its folder, from a link.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub(crate) struct RevealArgs {
    /// The file.
    path: PathArg,
    /// The line the preview shows it at, from 1.
    #[serde(default)]
    line: Option<u32>,
    /// In a tab of its own.
    #[serde(default)]
    tab: bool,
    #[serde(default)]
    pane: Option<skwir_core::workspace::PaneId>,
}

/// Labels for `count` links, in the order they are shown: the home row's
/// letters first, one letter each while there are few, two each when there
/// are more, so no label is the start of another.
pub fn labels(count: usize) -> Vec<String> {
    const KEYS: &[u8] = b"asdfghjklqwertyuiopzxcvbnm";
    if count <= KEYS.len() {
        return KEYS[..count]
            .iter()
            .map(|&key| char::from(key).to_string())
            .collect();
    }
    KEYS.iter()
        .flat_map(|&first| {
            KEYS.iter()
                .map(move |&second| format!("{}{}", char::from(first), char::from(second)))
        })
        .take(count)
        .collect()
}

/// Whether `target` is an address the desktop opens for skwir: a web page
/// or a mail address.
pub fn is_address(target: &str) -> bool {
    skwir_platform::open::is_web_uri(target) || skwir_platform::open::is_mail_uri(target)
}

impl Shell {
    /// The found link this frame draws as one: the one under the pointer,
    /// while Ctrl (Cmd) is held.
    pub(crate) fn armed_link(&self) -> Option<(ElementId, Range<usize>)> {
        self.links.hover.clone().filter(|_| self.links.held)
    }

    /// The pointer moved over a text with found links: `range` is the one
    /// under it, if any, in the text drawn as `element`.
    pub(crate) fn hover_link(
        &mut self,
        element: &ElementId,
        range: Option<Range<usize>>,
        held: bool,
        cx: &mut Context<Self>,
    ) {
        let hover = range.map(|range| (element.clone(), range));
        let left = hover.is_none()
            && self
                .links
                .hover
                .as_ref()
                .is_some_and(|(at, _)| at != element);
        if left {
            // Another text's link: that text says when it is left.
            return;
        }
        if self.links.hover != hover || self.links.held != held {
            self.links.hover = hover;
            self.links.held = held;
            cx.notify();
        }
    }

    /// The pointer left a text: a link of it is no longer under it.
    pub(crate) fn leave_link(&mut self, element: &ElementId, cx: &mut Context<Self>) {
        if self
            .links
            .hover
            .as_ref()
            .is_some_and(|(at, _)| at == element)
        {
            self.links.hover = None;
            cx.notify();
        }
    }

    /// Ctrl (Cmd) pressed or released: the link under the pointer becomes
    /// one, or stops being one, without the pointer moving.
    pub(crate) fn on_modifiers(
        &mut self,
        event: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let held = event.modifiers.secondary();
        if held == self.links.held {
            return;
        }
        self.links.held = held;
        if self.links.hover.is_some() || self.terminal_pointer_over_link() {
            cx.notify();
        }
    }

    /// `ui.open_url`: an address the user followed, opened by the desktop
    /// on a worker (it starts a program). Nothing but a web page or a mail
    /// address goes there.
    pub(crate) fn open_address(&mut self, url: &str, cx: &mut Context<Self>) {
        if !is_address(url) {
            self.set_status("Only web and mail links open from here", true, cx);
            return;
        }
        let app = Arc::clone(&self.app);
        let url = url.to_owned();
        self.app.scheduler.spawn(
            skwir_core::scheduler::Priority::Visible,
            "open-address",
            self.app.lifetime().child(),
            move || {
                if let Err(error) = skwir_platform::open::open_address(&url) {
                    app.notify(
                        skwir_core::app::NotifyLevel::Error,
                        format!("Could not open the link: {error}"),
                    );
                }
            },
        );
    }

    /// A link the document the preview shows holds, followed where it says
    /// (`docs/invariants/previews.md`): an address to the desktop through
    /// `ui.open_url`, anything else to a place on this machine, read next
    /// to the document and shown in its folder, selected, which the
    /// preview then follows; with `tab`, in a tab of its own. In a document
    /// inside a container, what a link names beside it is the container's
    /// (`follow_member_link`). A link is a place to show, never a file to
    /// run.
    pub(crate) fn follow_document_link(
        &mut self,
        href: &str,
        tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A preview a plugin made without the network keeps its links: a
        // click would carry what the plugin read where the link says.
        if let Some(why) = self.preview.closed_links.clone() {
            self.set_status(why, false, cx);
            return;
        }
        if is_address(href) {
            self.run(
                "ui.open_url",
                serde_json::json!({ "url": href }),
                window,
                cx,
            );
            return;
        }
        let member = self.preview.nested.last().cloned();
        if let Some(member) = member.as_ref().filter(|member| member.extracted())
            && !href.starts_with('/')
            && !href.starts_with("file:")
        {
            self.follow_member_link(member, href, window, cx);
            return;
        }
        // A track is a file of its own; a member written out is read from
        // its container's folder.
        let from = match &member {
            Some(member) if !member.extracted() => Some(member.path().to_path_buf()),
            _ => self.preview.path().map(std::path::Path::to_path_buf),
        };
        let Some((target, fragment)) = from.as_deref().and_then(|from| document_link(from, href))
        else {
            self.set_status("This link goes nowhere we can open", false, cx);
            return;
        };
        // The document itself, named: the place it says, here, as `#name`.
        if from.as_deref() == Some(target.as_path()) {
            if let Some(name) = fragment {
                self.show_mark(Mark::Anchor(name), cx);
            }
            return;
        }
        let pane = self.pane;
        let at = RevealAt {
            tab,
            mark: fragment.map(Mark::Anchor),
            from: from
                .as_deref()
                .and_then(std::path::Path::parent)
                .map(std::path::Path::to_path_buf),
        };
        if !self.reveal_path_at(pane, &target, at, window, cx) {
            let name = target.file_name().map_or_else(
                || target.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            self.set_status(format!("{name} is not there any more"), true, cx);
        }
    }

    /// A link of `member`, a document written out of its container, that
    /// names a file beside it: that file of the container, previewed in its
    /// place (`sibling`), at the mark the link names. Never the disk around
    /// the copy written out, which holds nothing of the container.
    fn follow_member_link(
        &mut self,
        member: &crate::preview::Nested,
        href: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let container = match self.preview.nested.iter().rev().nth(1) {
            Some(outer) => outer.name.clone(),
            None => self
                .preview
                .path()
                .and_then(std::path::Path::file_name)
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        };
        let found = match self.preview.parked() {
            Some(parked) => container_link(&parked.display.data.rows, member.target(), href),
            None => Err(Inside::Unlisted),
        };
        let (target, mark) = match found {
            Ok(found) => found,
            Err(why) => {
                self.set_status(why.said(&container), false, cx);
                return;
            }
        };
        if target == member.target() {
            if let Some(mark) = mark {
                self.show_mark(Mark::Anchor(mark), cx);
            }
            return;
        }
        self.inspect_here(
            &serde_json::json!({ "action": "sibling", "target": target }),
            window,
            cx,
        );
        if let (Some(mark), Some(pane), Some(path)) = (
            mark,
            self.preview.pane(),
            self.preview.path().map(std::path::Path::to_path_buf),
        ) {
            self.links.goto = Some(Goto {
                pane,
                path,
                member: Some(target),
                mark: Mark::Anchor(mark),
                until: Instant::now() + GOTO_WAIT,
            });
        }
    }

    /// `ui.reveal`: show a file in its folder (a folder itself), in the
    /// pane or a tab of its own, and the line its preview shows.
    pub(crate) fn reveal_command(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let args: RevealArgs = match serde_json::from_value(args.clone()) {
            Ok(args) => args,
            Err(error) => {
                self.set_status(format!("ui.reveal: {error}"), true, cx);
                return;
            }
        };
        let pane = args.pane.unwrap_or(self.pane);
        let at = RevealAt {
            tab: args.tab,
            mark: args.line.filter(|line| *line > 0).map(Mark::Line),
            from: None,
        };
        let path = args.path.into_inner();
        if !self.reveal_path_at(pane, &path, at, window, cx) {
            self.set_status("That link does not name a file", true, cx);
        }
    }

    /// A link of the terminal followed: an address to the desktop, a file
    /// or a folder into skwir, where the terminal follows the pane.
    pub(crate) fn follow_terminal_link(
        &mut self,
        target: &Target,
        tab: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target {
            Target::Uri(uri) => self.open_address(uri, cx),
            Target::Path { path, line, .. } => {
                let mut args = serde_json::json!({
                    "path": PathArg::from(path.clone()),
                    "tab": tab,
                });
                if let Some(line) = line {
                    args["line"] = (*line).into();
                }
                self.run("ui.reveal", args, window, cx);
            }
        }
    }

    /// Show `mark` of `path` once its preview arrives, opening the panel if
    /// it is closed: the place was asked for to be read. A file the panel
    /// shows already goes there at once.
    pub(crate) fn wait_for_mark(
        &mut self,
        pane: skwir_core::workspace::PaneId,
        path: PathBuf,
        mark: Mark,
        cx: &mut Context<Self>,
    ) {
        if !self.preview.open {
            self.preview.open = true;
            self.remember_session();
        }
        self.links.goto = Some(Goto {
            pane,
            path,
            member: None,
            mark,
            until: Instant::now() + GOTO_WAIT,
        });
        self.show_waiting_mark(cx);
        cx.notify();
    }

    /// The preview's content arrived or is there: show the place waiting
    /// for it, if it is of this file in the pane it was revealed in. One
    /// that waited too long is dropped, so selecting the file later does
    /// not jump. A mark the text does not hold leaves it at its start, and
    /// says so.
    pub(crate) fn show_waiting_mark(&mut self, cx: &mut Context<Self>) {
        let Some(goto) = self.links.goto.as_ref() else {
            return;
        };
        if goto.until < Instant::now() {
            self.links.goto = None;
            return;
        }
        if self.preview.path() != Some(goto.path.as_path())
            || self.preview.pane() != Some(goto.pane)
            || self.preview.nested.last().map(|member| member.target()) != goto.member.as_deref()
        {
            return;
        }
        if !matches!(
            self.preview.content,
            Some(crate::preview::PreviewContent::Inspection(_))
        ) {
            return;
        }
        if let Some(goto) = self.links.goto.take() {
            self.show_mark(goto.mark, cx);
        }
    }

    /// Show `mark` in the text the preview shows: a line, or the block
    /// holding a mark (`Block::anchors`), else the line it names as code
    /// hosts write one. A mark the text does not hold leaves it where it
    /// is, and says so.
    pub(crate) fn show_mark(&mut self, mark: Mark, cx: &mut Context<Self>) {
        let Some(crate::preview::PreviewContent::Inspection(display)) = &self.preview.content
        else {
            return;
        };
        let data = Arc::clone(&display.data);
        let line = match mark {
            Mark::Line(line) => line,
            Mark::Anchor(name) => {
                if self.preview.inspection.show_anchor(&data, &name) {
                    cx.notify();
                    return;
                }
                let Some(line) = line_of(&name) else {
                    let file = match self.preview.nested.last() {
                        Some(member) => member.name.clone(),
                        None => self
                            .preview
                            .path()
                            .and_then(std::path::Path::file_name)
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    };
                    self.set_status(format!("“{file}” has no “#{name}”"), false, cx);
                    return;
                };
                line
            }
        };
        self.preview.inspection.show_line(&data, line);
        // A text read where it is shown finds the line on a worker.
        self.ask_text();
        cx.notify();
    }
}

/// Why a link of a document inside a container leads to none of its
/// files.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Inside {
    /// The container's files are not listed above the document.
    Unlisted,
    /// Above the container's top.
    Out,
    /// No file of it has that path.
    Missing(String),
    /// A folder of it, which has no preview.
    Folder(String),
}

impl Inside {
    fn said(&self, container: &str) -> String {
        match self {
            Inside::Unlisted => format!("This link names a file inside “{container}”"),
            Inside::Out => format!("This link leads out of “{container}”"),
            Inside::Missing(path) => format!("“{container}” holds no “{path}”"),
            Inside::Folder(path) => format!("“{path}” is a folder of “{container}”"),
        }
    }
}

/// In a container's rows (a tree: each row a name, its first cell, at a
/// depth), the file a link `href` of member `own` (its `Row::target`)
/// names from where `own` is, and the mark the link names there.
pub(crate) fn container_link(
    rows: &[skwir_core::inspection::Row],
    own: &str,
    href: &str,
) -> Result<(String, Option<String>), Inside> {
    let decode = |text: &str| {
        String::from_utf8_lossy(&skwir_platform::uri::percent_decode(text)).into_owned()
    };
    let (href, mark) = href.split_once('#').unwrap_or((href, ""));
    let mark = (!mark.is_empty()).then(|| decode(mark));
    let href = decode(href.split('?').next().unwrap_or(href));
    // Each row's path, its folders' names before its own.
    let mut names: Vec<&str> = Vec::new();
    let paths: Vec<Vec<&str>> = rows
        .iter()
        .map(|row| {
            names.truncate(row.depth);
            names.push(row.cells.first().map_or("", String::as_str));
            names.clone()
        })
        .collect();
    let mine = rows
        .iter()
        .position(|row| row.target.as_deref() == Some(own))
        .ok_or(Inside::Unlisted)?;
    let mut wanted: Vec<&str> = paths[mine][..paths[mine].len() - 1].to_vec();
    if href.is_empty() {
        return Ok((own.to_owned(), mark));
    }
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                wanted.pop().ok_or(Inside::Out)?;
            }
            part => wanted.push(part),
        }
    }
    let said = wanted.join("/");
    let row = paths
        .iter()
        .position(|path| *path == wanted)
        .map(|index| &rows[index])
        .ok_or_else(|| Inside::Missing(said.clone()))?;
    match &row.target {
        Some(target) => Ok((target.clone(), mark)),
        None if row.branch => Err(Inside::Folder(said)),
        None => Err(Inside::Missing(said)),
    }
}

/// The line a fragment names as code hosts write it (`L12`, `L12-L20`,
/// `L12C5`), from 1.
fn line_of(fragment: &str) -> Option<u32> {
    let rest = fragment.strip_prefix('L')?;
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if !(rest[end..].is_empty() || rest[end..].starts_with(['-', 'C'])) {
        return None;
    }
    rest[..end].parse().ok().filter(|line| *line > 0)
}

/// Whether the link names a URI scheme (`mailto:`, `irc:`, `https:`) rather
/// than a path. A colon after a slash belongs to a file name, not a scheme.
/// Where a terminal's link leads, as it is said: its address, or its
/// path and the line it points at.
pub(crate) fn said(target: &Target) -> String {
    match target {
        Target::Uri(uri) => uri.clone(),
        Target::Path {
            path, line: None, ..
        } => path.display().to_string(),
        Target::Path {
            path,
            line: Some(line),
            ..
        } => format!("{}:{line}", path.display()),
    }
}

/// `said`, short enough for a menu's item: a long one keeps its start
/// and its end, where an address's host and a path's name are.
pub(crate) fn shortened(target: &Target) -> String {
    const MOST: usize = 60;
    let said = said(target);
    let chars: Vec<char> = said.chars().collect();
    if chars.len() <= MOST {
        return said;
    }
    let (head, tail) = (MOST * 2 / 3, MOST / 3 - 1);
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

fn has_scheme(href: &str) -> bool {
    let Some(colon) = href.find(':') else {
        return false;
    };
    let scheme = &href[..colon];
    !scheme.is_empty()
        && !scheme.contains('/')
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Where a document's link points on this machine, and the mark it names
/// there (its fragment), or `None` when it names something we cannot
/// resolve (another scheme, or a path that escapes into nothing). `from` is
/// the document holding the link, so a relative link is read the way the
/// author wrote it: next to their file.
pub(crate) fn document_link(
    from: &std::path::Path,
    href: &str,
) -> Option<(std::path::PathBuf, Option<String>)> {
    let decode = |text: &str| {
        String::from_utf8_lossy(&skwir_platform::uri::percent_decode(text)).into_owned()
    };
    let (href, fragment) = href.split_once('#').unwrap_or((href, ""));
    let fragment = (!fragment.is_empty()).then(|| decode(fragment));
    let href = href.split('?').next().filter(|h| !h.is_empty())?;
    let path = if href.starts_with("file:///") {
        // A file here, as the system writes it (`file:///C:/a%20b`).
        skwir_platform::uri::path_of(href)?
    } else if href.starts_with("file://") {
        // Another machine's: not ours to resolve.
        return None;
    } else if has_scheme(href) {
        return None;
    } else if href.starts_with('/') {
        std::path::PathBuf::from(decode(href))
    } else {
        from.parent()?.join(decode(href))
    };
    // Resolve `.` and `..` by text only. Canonicalizing would follow symlinks
    // and show the user a folder whose name their link never mentioned.
    let mut out = std::path::PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    Some((out, fragment))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_link_is_said_by_its_start_and_its_end() {
        let uri = |text: &str| Target::Uri(text.to_owned());
        let short = "https://example.org/a";
        assert_eq!(shortened(&uri(short)), short);
        let long = format!("https://example.org/{}/the-end.html", "é".repeat(80));
        let short = shortened(&uri(&long));
        assert_eq!(short.chars().count(), 60);
        assert!(short.starts_with("https://example.org/") && short.ends_with("/the-end.html"));
        assert!(short.contains('…'));
        let at = Target::Path {
            path: "/tmp/src/main.rs".into(),
            line: Some(12),
            column: Some(5),
        };
        assert_eq!(said(&at), "/tmp/src/main.rs:12");
    }

    #[test]
    fn a_documents_links_resolve_next_to_it() {
        let dir = std::path::Path::new("/notes/trip");
        let from = dir.join("Day one.md");
        let at = |href| document_link(&from, href).map(|(path, _)| path);
        assert_eq!(at("Day two.md"), Some(dir.join("Day two.md")));
        assert_eq!(at("Day%20two.md"), Some(dir.join("Day two.md")));
        assert_eq!(at("photos/"), Some(dir.join("photos")));
        assert_eq!(at("notes.md#later"), Some(dir.join("notes.md")));
        assert_eq!(
            at("file:///etc/hostname"),
            Some(std::path::PathBuf::from("/etc/hostname"))
        );
        assert_eq!(
            at("/srv/shared/plan.md"),
            Some("/srv/shared/plan.md".into())
        );
        assert_eq!(at("../gear/list.md"), Some("/notes/gear/list.md".into()));
        assert_eq!(at("./same.md"), Some(dir.join("same.md")));
        // Other schemes and empty targets are not places on this machine.
        assert_eq!(at("https://example.invalid/x"), None);
        assert_eq!(at("mailto:someone@example.invalid"), None);
        assert_eq!(at("file://host/share/x"), None);
        assert_eq!(at(""), None);
        assert_eq!(at("#section"), None);
    }

    /// A link of a document inside an archive names a file of the archive,
    /// from where the document is in it: its rows' tree, not the disk.
    #[test]
    fn a_link_inside_a_container_names_its_own_files() {
        let row = |name: &str, depth: usize, target: Option<&str>| skwir_core::inspection::Row {
            cells: vec![name.to_owned()],
            bytes: None,
            depth,
            branch: target.is_none(),
            target: target.map(str::to_owned),
        };
        let rows = [
            row("docs", 0, None),
            row("guide.md", 1, Some("1")),
            row("l'été", 1, None),
            row("notes.md", 2, Some("3")),
            row("read me.md", 1, Some("2")),
            row("top.md", 0, Some("4")),
        ];
        let at = |href| container_link(&rows, "2", href);
        let found =
            |target: &str, mark: Option<&str>| Ok((target.to_owned(), mark.map(str::to_owned)));
        assert_eq!(at("guide.md#setup"), found("1", Some("setup")));
        assert_eq!(at("./l%27%C3%A9t%C3%A9/notes.md"), found("3", None));
        assert_eq!(at("../top.md?raw=1"), found("4", None));
        assert_eq!(at("read%20me.md#the-end"), found("2", Some("the-end")));
        assert_eq!(at("../../top.md"), Err(Inside::Out));
        assert_eq!(
            at("missing.md"),
            Err(Inside::Missing("docs/missing.md".into()))
        );
        assert_eq!(at("l'été"), Err(Inside::Folder("docs/l'été".into())));
        assert_eq!(
            container_link(&rows, "9", "guide.md"),
            Err(Inside::Unlisted)
        );
    }

    #[test]
    fn a_link_names_a_mark_in_the_file_it_leads_to() {
        let from = std::path::Path::new("/notes/trip/Day one.md");
        let mark = |href| document_link(from, href).and_then(|(_, mark)| mark);
        assert_eq!(mark("notes.md#later"), Some("later".to_owned()));
        assert_eq!(mark("notes.md?plain=1#L12"), Some("L12".to_owned()));
        assert_eq!(
            mark("notes.md#caf%C3%A9-l'apr%C3%A8s"),
            Some("café-l'après".to_owned())
        );
        assert_eq!(mark("notes.md#"), None);
        assert_eq!(mark("notes.md"), None);
        assert_eq!(
            document_link(from, "Day%20two.md#a#b"),
            Some(("/notes/trip/Day two.md".into(), Some("a#b".to_owned())))
        );
        // A path found in a file's text links as its address: read back as
        // it is, whatever its name holds, on every system.
        let file = std::env::temp_dir().join("l'été #1").join("100% sûr.md");
        let found = format!("{}#L12", skwir_platform::uri::file_uri(&file));
        assert_eq!(
            document_link(from, &found),
            Some((file, Some("L12".to_owned())))
        );
        assert_eq!(document_link(from, "file://elsewhere/share/x.md"), None);
        // Lines as code hosts write them.
        assert_eq!(line_of("L12"), Some(12));
        assert_eq!(line_of("L12-L20"), Some(12));
        assert_eq!(line_of("L12C5-L12C9"), Some(12));
        for not in ["L", "L0", "12", "Lunch", "L12x", "l12"] {
            assert_eq!(line_of(not), None, "{not}");
        }
    }

    #[test]
    fn labels_are_short_and_none_starts_another() {
        assert_eq!(labels(3), ["a", "s", "d"]);
        assert_eq!(labels(26).len(), 26);
        let many = labels(100);
        assert_eq!(many.len(), 100);
        assert!(many.iter().all(|label| label.len() == 2));
        for (i, a) in many.iter().enumerate() {
            for b in &many[i + 1..] {
                assert!(!b.starts_with(a.as_str()) && a != b);
            }
        }
    }

    #[test]
    fn only_web_and_mail_addresses_reach_the_desktop() {
        assert!(is_address("https://example.org"));
        assert!(is_address("mailto:a@example.org"));
        assert!(!is_address("file:///etc/passwd"));
        assert!(!is_address("javascript:alert(1)"));
    }
}
