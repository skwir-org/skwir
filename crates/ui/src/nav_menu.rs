//! The pane row's menus of places: the history behind Back or ahead of
//! Forward (their right-click, `ui.history_menu`), the places inside a
//! place of the trail (the chevron after it, `ui.subfolder_menu`): a
//! folder's folders, a tag's tags (`LocationProvider::places`), and the
//! folders a trail leaves out above its first place (the chevron before
//! it, `ui.parents_menu`), which there is no Up button to reach.
//!
//! All hang from what opened them in the pane's row, which draws their
//! anchor (`address::render_address`). A folder is read on a worker, since
//! a sleeping disk can block it, and so are a provider's places; the menu
//! opens when the answer arrives, if it is still wanted. The folders above
//! one are known from its path alone.

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
    History {
        forward: bool,
    },
    /// The places inside `of`: a folder's folders, a tag's tags.
    Subfolders {
        of: bibli_core::Location,
    },
    /// The folders that hold `of`, where a trail starts.
    Parents {
        of: PathBuf,
    },
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
    /// How far in the row stands: the folders above a trail are a tree.
    depth: usize,
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
                label: self.location_name(&location).into(),
                tip: match location.path() {
                    Some(path) => path.display().to_string().into(),
                    None => location.to_string().into(),
                },
                icon: crate::trail::glyph(&self.app, &location),
                command: if forward { "forward" } else { "back" },
                args: serde_json::json!({ "steps": steps }),
                current: false,
                enabled: true,
                depth: 0,
            })
            .collect();
        self.nav_menu = Some(NavMenu {
            pane,
            what,
            rows,
            popup: Popup::new("nav", window, cx),
        });
    }

    /// `ui.subfolder_menu { location, current }`: the folders inside
    /// `location` (a folder; `path` is its old name), or the places a
    /// provider says are inside one that is no folder, `current` (where
    /// the trail goes on) marked; `{ close: true }` closes it. Hidden
    /// folders are listed when the pane shows hidden files.
    pub fn toggle_subfolder_menu(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let location = |key: &str| {
            args.get(key)
                .and_then(|v| serde_json::from_value::<PathArg>(v.clone()).ok())
                .map(|arg| bibli_core::Location::parse_os(arg.as_os_str()))
        };
        let was = self.close_nav_menu(window);
        cx.notify();
        // A later request, or a close, makes an answer on its way unwanted.
        self.nav_asked += 1;
        if self.nav_closing(args, was.as_ref()) {
            return;
        }
        let Some(of) = location("location").or_else(|| location("path")) else {
            return;
        };
        let what = NavWhat::Subfolders { of: of.clone() };
        if was.as_ref() == Some(&what) || self.just_dismissed(&what.key()) {
            return;
        }
        let asked = self.nav_asked;
        let current = location("current");
        let Some(folder) = of.path() else {
            self.places_menu(of, current, asked, window, cx);
            return;
        };
        let current = current.as_ref().and_then(bibli_core::Location::path);
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
                    what: NavWhat::Subfolders {
                        of: bibli_core::Location::from_path(folder),
                    },
                    rows,
                    popup: Popup::new("nav", window, cx),
                });
                cx.notify();
            },
            window,
            cx,
        );
    }

    /// The places a provider says are inside `of`, a place of the trail
    /// that is no folder (a tag's tags), asked on a worker.
    fn places_menu(
        &mut self,
        of: bibli_core::Location,
        current: Option<bibli_core::Location>,
        asked: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane;
        let app = self.app.clone();
        let reading = of.clone();
        self.off_thread(
            "places",
            move || app.places_in(&reading),
            move |shell, places, window, cx| {
                if shell.nav_asked != asked {
                    return;
                }
                let navigate =
                    |l: &bibli_core::Location| serde_json::json!({ "location": l.to_string() });
                let mut rows: Vec<NavRow> = places
                    .iter()
                    .take(FOLDER_ROWS)
                    .map(|place| NavRow {
                        label: place.label.clone().into(),
                        tip: shell.app.location_title(&place.location).into(),
                        icon: crate::trail::glyph(&shell.app, &place.location),
                        command: "navigate",
                        args: navigate(&place.location),
                        current: current.as_ref() == Some(&place.location),
                        enabled: true,
                        depth: 0,
                    })
                    .collect();
                if rows.is_empty() {
                    rows.push(NavRow {
                        label: "Nothing inside".into(),
                        tip: "Nothing inside".into(),
                        icon: "",
                        command: "navigate",
                        args: navigate(&of),
                        current: false,
                        enabled: false,
                        depth: 0,
                    });
                }
                shell.close_nav_menu(window);
                shell.nav_menu = Some(NavMenu {
                    pane,
                    what: NavWhat::Subfolders { of },
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

impl Shell {
    /// `ui.parents_menu { location }`: the folders that hold `location`,
    /// the folder a trail starts at, from the root down to it (marked);
    /// `{ close: true }` closes it. Known from the path: nothing is read.
    pub fn toggle_parents_menu(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let was = self.close_nav_menu(window);
        cx.notify();
        // A folder's menu on its way is no longer wanted.
        self.nav_asked += 1;
        if self.nav_closing(args, was.as_ref()) {
            return;
        }
        let Some(top) = args
            .get("location")
            .and_then(|v| serde_json::from_value::<PathArg>(v.clone()).ok())
            .and_then(|arg| bibli_core::Location::parse_os(arg.as_os_str()).path())
        else {
            return;
        };
        let what = NavWhat::Parents { of: top.clone() };
        if was.as_ref() == Some(&what) || self.just_dismissed(&what.key()) {
            return;
        }
        self.nav_menu = Some(NavMenu {
            pane: self.pane,
            what,
            rows: parent_rows(&top),
            popup: Popup::new("nav", window, cx),
        });
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
            depth: 0,
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
            depth: 0,
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
            depth: 0,
        });
    }
    rows
}

/// The rows of the menu of what a trail leaves out above `top`, its
/// first folder: every folder from the root down to `top` itself
/// (marked), each a step further in, under its own name as the other
/// chevrons' menus list folders: `/ › home › example`, never the
/// sidebar's `Home`.
fn parent_rows(top: &Path) -> Vec<NavRow> {
    let mut folders: Vec<&Path> = top.ancestors().collect();
    folders.reverse();
    folders
        .into_iter()
        .enumerate()
        .map(|(depth, folder)| NavRow {
            label: folder
                .file_name()
                .map_or_else(
                    || folder.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                )
                .into(),
            tip: folder.display().to_string().into(),
            icon: "folder",
            command: "navigate",
            args: serde_json::json!({ "location": PathArg::from(folder) }),
            current: folder == top,
            enabled: true,
            depth,
        })
        .collect()
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
        NavWhat::Parents { .. } => "ui.parents_menu",
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
            .render(shell, cx)
            .when(row.depth > 0, |d| {
                d.pl(gpui::px(8. + 12. * row.depth as f32))
            }),
        );
    }
    Some(panel.into_any_element())
}

#[cfg(test)]
mod tests {
    use super::{parent_rows, subfolders};
    use std::path::Path;

    /// From the root down to the folder the trail starts at, which is
    /// marked, each by its own name, whatever the sidebar calls it.
    #[test]
    fn the_folders_above_a_trail_go_from_the_root_down_to_it() {
        let rows = parent_rows(Path::new("/home/example/Documents"));
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_ref()).collect();
        assert_eq!(labels, ["/", "home", "example", "Documents"]);
        assert_eq!(
            rows.iter().map(|r| r.depth).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(
            rows.iter().map(|r| r.current).collect::<Vec<_>>(),
            [false, false, false, true]
        );
        assert_eq!(
            rows[1].args,
            serde_json::json!({ "location": bibli_core::os_path::PathArg::from(Path::new("/home")) })
        );
        let rows = parent_rows(Path::new("/srv/it's data"));
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_ref()).collect();
        assert_eq!(labels, ["/", "srv", "it's data"]);
    }

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
