//! The pane row's menus of places: the history behind Back or ahead of
//! Forward (their right-click, `ui.history_menu`), and the folders inside
//! a folder of the trail (the chevron after it, `ui.subfolder_menu`).
//!
//! Both hang from what opened them in the pane's row, which draws their
//! anchor (`address::render_address`). A folder is read on a worker, since
//! a sleeping disk can block it; the menu opens when the answer arrives,
//! if it is still wanted.

use std::path::{Path, PathBuf};

use bibli_core::os_path::PathArg;
use bibli_core::workspace::PaneId;
use gpui::{AnyElement, Context, SharedString, Window, prelude::*};

use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::{self, Button, Popup};

/// The most places the history menu lists, as browsers keep theirs short.
const HISTORY_ROWS: usize = 15;
/// The most folders a folder's menu lists; one more row opens the folder.
const FOLDER_ROWS: usize = 500;

/// What a menu of places lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavWhat {
    History { forward: bool },
    Subfolders { folder: PathBuf },
}

/// A place in the menu: what it runs, and whether the pane is there
/// (the folder the trail goes on into).
struct NavRow {
    label: SharedString,
    tip: SharedString,
    icon: &'static str,
    command: &'static str,
    args: serde_json::Value,
    current: bool,
    enabled: bool,
}

/// The open menu of places.
pub struct NavMenu {
    pub pane: PaneId,
    pub what: NavWhat,
    rows: Vec<NavRow>,
    popup: Popup,
}

impl Shell {
    /// Close the menu of places, if one is open; what it listed.
    pub(crate) fn close_nav_menu(&mut self, window: &mut Window) -> Option<NavWhat> {
        let menu = self.nav_menu.take()?;
        menu.popup.restore(window);
        Some(menu.what)
    }

    /// `ui.history_menu { forward }`: the active pane's history behind
    /// Back (or ahead of Forward), nearest first; `{ close: true }` closes
    /// it. Choosing one runs `back` or `forward` with its `steps`.
    pub fn toggle_history_menu(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let forward = args.get("forward").and_then(serde_json::Value::as_bool) == Some(true);
        let what = NavWhat::History { forward };
        let was = self.close_nav_menu(window);
        cx.notify();
        if self.nav_closing(args, was.as_ref()) || was.as_ref() == Some(&what) {
            return;
        }
        if self.just_dismissed(&what.key()) {
            return;
        }
        let pane = self.pane;
        let places: Vec<(usize, bibli_core::Location)> = {
            let ws = self.app.workspace.read();
            let Some(p) = ws.pane(pane) else {
                return;
            };
            let pos = p.history_pos;
            if forward {
                p.history
                    .iter()
                    .enumerate()
                    .skip(pos + 1)
                    .take(HISTORY_ROWS)
                    .map(|(i, l)| (i - pos, l.clone()))
                    .collect()
            } else {
                p.history[..pos.min(p.history.len())]
                    .iter()
                    .enumerate()
                    .rev()
                    .take(HISTORY_ROWS)
                    .map(|(i, l)| (pos - i, l.clone()))
                    .collect()
            }
        };
        if places.is_empty() {
            return;
        }
        let rows = places
            .into_iter()
            .map(|(steps, location)| NavRow {
                label: self.app.location_title(&location).into(),
                tip: match location.path() {
                    Some(path) => path.display().to_string().into(),
                    None => location.to_string().into(),
                },
                icon: match &*location.scheme {
                    "query" => "search",
                    "shelf" => "shelf",
                    _ => "folder",
                },
                command: if forward { "forward" } else { "back" },
                args: serde_json::json!({ "steps": steps }),
                current: false,
                enabled: true,
            })
            .collect();
        self.nav_menu = Some(NavMenu {
            pane,
            what,
            rows,
            popup: Popup::new("nav", window, cx),
        });
    }

    /// `ui.subfolder_menu { path, current }`: the folders inside `path`,
    /// `current` (where the trail goes on) marked; `{ close: true }`
    /// closes it. Hidden folders are listed when the pane shows hidden
    /// files.
    pub fn toggle_subfolder_menu(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = |key: &str| {
            args.get(key)
                .and_then(|v| serde_json::from_value::<PathArg>(v.clone()).ok())
                .map(PathArg::into_inner)
        };
        let was = self.close_nav_menu(window);
        cx.notify();
        // A later request, or a close, makes an answer on its way unwanted.
        self.nav_asked += 1;
        if self.nav_closing(args, was.as_ref()) {
            return;
        }
        let Some(folder) = path("path") else {
            return;
        };
        let what = NavWhat::Subfolders {
            folder: folder.clone(),
        };
        if was.as_ref() == Some(&what) || self.just_dismissed(&what.key()) {
            return;
        }
        let asked = self.nav_asked;
        let current = path("current");
        let hidden = self.sort_spec().show_hidden;
        let pane = self.pane;
        let reading = folder.clone();
        self.off_thread(
            "subfolders",
            move || subfolders(&reading, hidden),
            move |shell, read, window, cx| {
                // Still asked for, and the pane still shows that folder or
                // one inside it.
                let shows = shell
                    .app
                    .workspace
                    .read()
                    .pane(pane)
                    .and_then(|p| p.location())
                    .and_then(bibli_core::Location::path)
                    .is_some_and(|shown| shown.starts_with(&folder));
                if shell.nav_asked != asked || !shows {
                    return;
                }
                let dirs = match read {
                    Ok(dirs) => dirs,
                    Err(error) => {
                        let name = folder.file_name().map_or_else(
                            || folder.display().to_string(),
                            |n| n.to_string_lossy().into_owned(),
                        );
                        shell.set_status(format!("Could not read “{name}”: {error}"), true, cx);
                        return;
                    }
                };
                let rows = folder_rows(&folder, dirs, current.as_deref());
                shell.close_nav_menu(window);
                shell.nav_menu = Some(NavMenu {
                    pane,
                    what: NavWhat::Subfolders { folder },
                    rows,
                    popup: Popup::new("nav", window, cx),
                });
                cx.notify();
            },
            window,
            cx,
        );
    }
}

impl NavWhat {
    /// What `Shell::note_dismissed` remembers of it.
    fn key(&self) -> String {
        format!("nav {self:?}")
    }
}

impl Shell {
    /// Whether `args` close the menu (`was`, now closed), noting a press
    /// outside that did.
    fn nav_closing(&mut self, args: &serde_json::Value, was: Option<&NavWhat>) -> bool {
        let closing = args.get("close").and_then(serde_json::Value::as_bool) == Some(true);
        if closing && let Some(was) = was {
            self.note_dismissed(args, was.key());
        }
        closing
    }
}

/// The rows of a folder's menu: its folders up to `FOLDER_ROWS`, then one
/// that opens the folder for the rest; a folder with none says so.
fn folder_rows(folder: &Path, dirs: Vec<PathBuf>, current: Option<&Path>) -> Vec<NavRow> {
    let total = dirs.len();
    let navigate = |path: &Path| serde_json::json!({ "location": PathArg::from(path) });
    let mut rows: Vec<NavRow> = dirs
        .into_iter()
        .take(FOLDER_ROWS)
        .map(|dir| NavRow {
            label: dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
            tip: dir.display().to_string().into(),
            icon: "folder",
            command: "navigate",
            args: navigate(&dir),
            current: current == Some(dir.as_path()),
            enabled: true,
        })
        .collect();
    if total == 0 {
        rows.push(NavRow {
            label: "No folders inside".into(),
            tip: "No folders inside".into(),
            icon: "",
            command: "navigate",
            args: navigate(folder),
            current: false,
            enabled: false,
        });
    } else if total > FOLDER_ROWS {
        let more = format!("{} more: open the folder", total - FOLDER_ROWS);
        rows.push(NavRow {
            label: more.clone().into(),
            tip: more.into(),
            icon: "open",
            command: "navigate",
            args: navigate(folder),
            current: false,
            enabled: true,
        });
    }
    rows
}

/// The folders inside `folder` in the panes' name order, links to folders
/// included; names starting with a dot only when `hidden`.
fn subfolders(folder: &Path, hidden: bool) -> std::io::Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let Ok(entry) = entry else {
            continue;
        };
        if !hidden && entry.file_name().as_encoded_bytes().starts_with(b".") {
            continue;
        }
        let dir = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => entry.path().is_dir(),
            Ok(kind) => kind.is_dir(),
            Err(_) => false,
        };
        if dir {
            dirs.push(entry.path());
        }
    }
    dirs.sort_by_cached_key(|dir| {
        bibli_core::collate::sort_key(dir.file_name().unwrap_or_default())
    });
    Ok(dirs)
}

/// The open menu's panel, for the row of `pane` that draws its anchor.
pub fn panel(shell: &Shell, pane: PaneId, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let menu = shell.nav_menu.as_ref().filter(|m| m.pane == pane)?;
    let theme = cx.global::<Theme>().clone();
    let close = match menu.what {
        NavWhat::History { .. } => "ui.history_menu",
        NavWhat::Subfolders { .. } => "ui.subfolder_menu",
    };
    let mut panel = toolbar::popup_panel(&menu.popup, close, &theme, cx)
        .w_auto()
        .min_w(gpui::px(200.))
        .max_w(gpui::px(360.));
    for (i, row) in menu.rows.iter().enumerate() {
        panel = panel.child(
            Button::new(
                SharedString::from(format!("nav-{i}")),
                row.icon,
                row.tip.clone(),
                row.command,
            )
            .args(row.args.clone())
            .label(row.label.clone())
            .menu()
            .active(row.current)
            .enabled(row.enabled)
            .render(shell, cx),
        );
    }
    Some(panel.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::subfolders;

    #[test]
    fn a_folder_s_menu_lists_its_folders_in_name_order() {
        let tmp = tempfile::tempdir().unwrap();
        for dir in ["it's b", "a 10", "a 9", ".hidden", "Été"] {
            std::fs::create_dir(tmp.path().join(dir)).unwrap();
        }
        std::fs::write(tmp.path().join("a file"), b"example").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.path().join("a 9"), tmp.path().join("link")).unwrap();
        let names = |hidden| -> Vec<String> {
            subfolders(tmp.path(), hidden)
                .unwrap()
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(names(false), ["a 9", "a 10", "Été", "it's b", "link"]);
        assert_eq!(names(true)[0], ".hidden");
        assert!(subfolders(&tmp.path().join("missing"), false).is_err());
    }
}
