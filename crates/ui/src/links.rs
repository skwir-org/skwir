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
    /// A line to show once the preview of its file arrives.
    goto: Option<Goto>,
}

struct Goto {
    /// The pane the file was revealed in: its preview is the one to wait
    /// for, not another pane's of the same file.
    pane: skwir_core::workspace::PaneId,
    path: PathBuf,
    line: u32,
    until: Instant,
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
/// the line its preview shows.
#[derive(Clone, Copy, Debug, Default)]
pub struct RevealAt {
    pub tab: bool,
    pub line: Option<u32>,
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

    /// A link a document holds, followed where it says
    /// (`docs/invariants/previews.md`): an address to the desktop through
    /// `ui.open_url`, anything else to a place on this machine, read next
    /// to `from` (the document holding it) and shown in its folder,
    /// selected, which the preview then follows; with `tab`, in a tab of
    /// its own. A link is a place to show, never a file to run.
    pub(crate) fn follow_document_link(
        &mut self,
        href: &str,
        from: Option<&std::path::Path>,
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
        let Some(target) = from.and_then(|from| document_link(from, href)) else {
            self.set_status("This link goes nowhere we can open", false, cx);
            return;
        };
        let pane = self.pane;
        let at = RevealAt { tab, line: None };
        if !self.reveal_path_at(pane, &target, at, window, cx) {
            let name = target.file_name().map_or_else(
                || target.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            self.set_status(format!("{name} is not there any more"), true, cx);
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
            line: args.line.filter(|line| *line > 0),
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

    /// Show `line` of `path` once its preview arrives, opening the panel if
    /// it is closed: the line was asked for to be read. A file the panel
    /// shows already goes there at once.
    pub(crate) fn wait_for_line(
        &mut self,
        pane: skwir_core::workspace::PaneId,
        path: PathBuf,
        line: u32,
        cx: &mut Context<Self>,
    ) {
        if !self.preview.open {
            self.preview.open = true;
            self.remember_session();
        }
        self.links.goto = Some(Goto {
            pane,
            path,
            line,
            until: Instant::now() + GOTO_WAIT,
        });
        self.show_waiting_line();
        cx.notify();
    }

    /// The preview's content arrived or is there: show the line waiting
    /// for it, if it is of this file in the pane it was revealed in. One
    /// that waited too long is dropped, so selecting the file later does
    /// not jump.
    pub(crate) fn show_waiting_line(&mut self) {
        let Some(goto) = self.links.goto.as_ref() else {
            return;
        };
        if goto.until < Instant::now() {
            self.links.goto = None;
            return;
        }
        if self.preview.path() != Some(goto.path.as_path())
            || self.preview.pane() != Some(goto.pane)
        {
            return;
        }
        let Some(crate::preview::PreviewContent::Inspection(display)) = &self.preview.content
        else {
            return;
        };
        let line = goto.line;
        let data = Arc::clone(&display.data);
        self.links.goto = None;
        self.preview.inspection.show_line(&data, line);
        // A text read where it is shown finds the line on a worker.
        self.ask_text();
    }
}

/// Whether the link names a URI scheme (`mailto:`, `irc:`, `https:`) rather
/// than a path. A colon after a slash belongs to a file name, not a scheme.
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

/// Where a document's link points on this machine, or `None` when it names
/// something we cannot resolve (another scheme, or a path that escapes into
/// nothing). `from` is the document holding the link, so a relative link is
/// read the way the author wrote it: next to their file.
pub(crate) fn document_link(from: &std::path::Path, href: &str) -> Option<std::path::PathBuf> {
    let href = href.split(['#', '?']).next().filter(|h| !h.is_empty())?;
    let href = String::from_utf8_lossy(&skwir_platform::uri::percent_decode(href)).into_owned();
    let path = if let Some(rest) = href.strip_prefix("file://") {
        // file://host/path is not ours to resolve; file:///path is.
        std::path::PathBuf::from(rest.strip_prefix('/').map(|p| format!("/{p}"))?)
    } else if has_scheme(&href) {
        return None;
    } else if href.starts_with('/') {
        std::path::PathBuf::from(href)
    } else {
        from.parent()?.join(href)
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
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_documents_links_resolve_next_to_it() {
        let dir = std::path::Path::new("/notes/trip");
        let from = dir.join("Day one.md");
        let at = |href| document_link(&from, href);
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
