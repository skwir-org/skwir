//! The pane row's menus of places: the history behind Back or ahead of
//! Forward (their right-click, `ui.history_menu`), the places inside a
//! place of the trail (the chevron after it, `ui.subfolder_menu`): a
//! folder's folders after the places of it (its tags,
//! `LocationProvider::places_of`), a tag's tags
//! (`LocationProvider::places`), and the
//! folders a trail leaves out above its first place (the chevron before
//! it, `ui.parents_menu`), which there is no Up button to reach, and
//! what a narrow pane wrapped out of sight above the first place it
//! shows (that place's chevron, the same menu).
//!
//! All hang from what opened them in the pane's row, which draws their
//! anchor (`address::render_address`). A folder is read on a worker, since
//! a sleeping disk can block it, and so are a provider's places; the menu
//! opens when the answer arrives, if it is still wanted. The folders above
//! one are known from its path alone.

use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Context, FocusHandle, Pixels, ScrollStrategy, SharedString,
    UniformListScrollHandle, Window, div, prelude::*, px,
};
use skwir_core::os_path::PathArg;
use skwir_core::workspace::PaneId;

use crate::keymap::RunCommand;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::toolbar::{self, Button, Popup};

/// The most places the history menu lists, as browsers keep theirs short.
const HISTORY_ROWS: usize = 15;
/// The most folders a folder's menu lists; one more row opens the folder.
const FOLDER_ROWS: usize = 500;

/// What a menu of places lists.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NavWhat {
    History {
        forward: bool,
    },
    /// The places inside `of`: a folder's folders, a tag's tags.
    Subfolders {
        of: skwir_core::Location,
    },
    /// What is above `of`, a place of the trail: the folders that hold
    /// it, or for a place that is no folder the trail's places before it.
    Parents {
        of: skwir_core::Location,
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
    /// A line above it: a folder's folders after the places of it (its
    /// tags).
    rule: bool,
}

/// A row's height in the menu: its button, and the gap above it where
/// the line of a row with `rule` is drawn. The same for every row, so
/// only those in view are drawn (`uniform_list`): a folder's menu of
/// five hundred folders built every row at every frame it was open.
const ROW: f32 = 34.;

/// The open menu of places.
pub struct NavMenu {
    pub pane: PaneId,
    pub what: NavWhat,
    rows: Vec<NavRow>,
    popup: Popup,
    /// Each row's focus: the keyboard goes to a row not drawn yet, once
    /// scrolled to (`Shell::walk_nav_menu`).
    focus: Vec<FocusHandle>,
    scroll: UniformListScrollHandle,
    /// As wide as its widest row, measured once as it opened.
    width: Pixels,
}

impl NavMenu {
    fn new(
        pane: PaneId,
        what: NavWhat,
        rows: Vec<NavRow>,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> Self {
        let font = cx.global::<Theme>().font.clone();
        let width = width_of(&rows, &font, window);
        let focus = rows.iter().map(|_| cx.focus_handle()).collect();
        NavMenu {
            pane,
            what,
            rows,
            popup: Popup::new("nav", window, cx),
            focus,
            scroll: UniformListScrollHandle::new(),
            width,
        }
    }
}

/// The width of a menu of `rows`: its widest label, shaped in `font` as
/// a menu row draws it, with what a row and the popup add around it,
/// from 200 to 360 pixels as the menu was.
fn width_of(rows: &[NavRow], font: &SharedString, window: &mut Window) -> Pixels {
    let size = window.rem_size() * 0.75;
    let font = gpui::font(font.clone());
    let widest = rows
        .iter()
        .map(|row| {
            let run = gpui::TextRun {
                len: row.label.len(),
                font: font.clone(),
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
                position: gpui::RunPosition::Baseline,
            };
            let label = window
                .text_system()
                .shape_line(row.label.clone(), size, &[run], None)
                .width;
            label + px(12. * row.depth as f32)
        })
        .fold(
            px(0.),
            |widest, width| if width > widest { width } else { widest },
        );
    // The icon and the gap after it, the row's padding and edge, the
    // popup's padding and edge, and a pixel of slack.
    let around = 16. + 10. + 2. * 8. + 2. + 2. * toolbar::POPUP_PAD + 2. + 1.;
    (widest + px(around)).clamp(px(200.), px(360.))
}

impl Shell {
    /// Close the menu of places, if one is open; what it listed.
    pub(crate) fn close_nav_menu(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> Option<NavWhat> {
        let menu = self.nav_menu.take()?;
        menu.popup.restore(window, cx);
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
        cx.notify();
        if !self.toggle_popup(&what.key(), args, window, cx) {
            return;
        }
        let pane = self.pane;
        let places: Vec<(usize, skwir_core::Location)> = {
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
                icon: crate::trail::glyph(&self.app, &location),
                command: if forward { "forward" } else { "back" },
                args: serde_json::json!({ "steps": steps }),
                current: false,
                enabled: true,
                depth: 0,
                rule: false,
            })
            .collect();
        self.nav_menu = Some(NavMenu::new(pane, what, rows, window, cx));
    }

    /// `ui.subfolder_menu { location, current }`: the folders inside
    /// `location` (a folder), or the places a
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
                .map(|arg| skwir_core::Location::parse_os(arg.as_os_str()))
        };
        cx.notify();
        let of = location("location");
        let what = of.clone().map(|of| NavWhat::Subfolders { of });
        // A later request, or a close, makes an answer on its way unwanted
        // (`toggle_popup` counts it in `nav_asked`).
        let key = what.as_ref().map(NavWhat::key).unwrap_or_default();
        let (Some(of), true) = (of, self.toggle_popup(&key, args, window, cx)) else {
            return;
        };
        let asked = self.nav_asked;
        let current = location("current");
        let hidden = self.sort_spec().show_hidden;
        let pane = self.pane;
        // Answered for the location the pane shows now: one shown since
        // asks again.
        let shown = self.app.pane_location(pane);
        let app = self.app.clone();
        let reading = of.clone();
        let along = current.clone();
        let lane = of
            .path()
            .map_or(skwir_core::scheduler::Lane::SHARED, |path| {
                skwir_core::fs::reach::lane(&path)
            });
        self.off_thread(
            lane,
            "inside",
            move || app.places_in(&reading, along.as_ref(), hidden),
            move |shell, read, window, cx| {
                if shell.nav_asked != asked || shell.app.pane_location(pane) != shown {
                    return;
                }
                let inside = match read {
                    Ok(inside) => inside,
                    Err(error) => {
                        let name = shell.app.location_title(&of);
                        shell.set_status(format!("Could not read “{name}”: {error}"), true, cx);
                        return;
                    }
                };
                let rows = inside_rows(&shell.app, &of, inside, current.as_ref());
                shell.close_nav_menu(window, cx);
                shell.nav_menu = Some(NavMenu::new(
                    pane,
                    NavWhat::Subfolders { of },
                    rows,
                    window,
                    cx,
                ));
                cx.notify();
            },
            window,
            cx,
        );
    }
}

impl Shell {
    /// `ui.parents_menu { location }`: what is above `location`, a place
    /// of the active pane's trail, down to it (marked): a folder's folders
    /// from the root down, a place's (a tag's, a trashed folder's) the
    /// trail's places before it, after the folders above the trail's first
    /// when that is one. `{ close: true }` closes it. Known from the path
    /// and the trail: nothing is read.
    pub fn toggle_parents_menu(
        &mut self,
        args: &serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
        // A folder as the trail names it (`Location::from_path`), so the
        // chevron that opened it knows its menu.
        let of = args
            .get("location")
            .and_then(|v| serde_json::from_value::<PathArg>(v.clone()).ok())
            .map(|arg| skwir_core::Location::parse_os(arg.as_os_str()))
            .map(|of| of.path().map_or(of, skwir_core::Location::from_path));
        let key = of
            .clone()
            .map(|of| NavWhat::Parents { of }.key())
            .unwrap_or_default();
        let (Some(of), true) = (of, self.toggle_popup(&key, args, window, cx)) else {
            return;
        };
        let rows = match of.path() {
            Some(top) => parent_rows(&top, true),
            None => self.trail_rows(&of),
        };
        if rows.is_empty() {
            return;
        }
        let pane = self.pane;
        self.nav_menu = Some(NavMenu::new(
            pane,
            NavWhat::Parents { of },
            rows,
            window,
            cx,
        ));
    }

    /// The rows of what is above `of` in the active pane's trail, `of`
    /// a place that is no folder: the folders above the trail's first
    /// segment when that is a folder, root down, then its segments down to
    /// `of` (marked), as the trail names them.
    fn trail_rows(&self, of: &skwir_core::Location) -> Vec<NavRow> {
        let Some(trail) = self.ui().place.trail.as_ref() else {
            return Vec::new();
        };
        let segments = crate::trail::segments(trail, &self.roots.roots);
        let Some((folder, places)) = above_in_trail(&segments, of) else {
            return Vec::new();
        };
        let mut rows = folder.map_or_else(Vec::new, |path| parent_rows(&path, false));
        let app = &self.app;
        for segment in places {
            rows.push(NavRow {
                label: segment.label.clone().into(),
                tip: app.location_title(&segment.location).into(),
                icon: crate::trail::glyph(app, &segment.location),
                command: "navigate",
                args: serde_json::json!({ "location": segment.location.arg() }),
                current: segment.location.same_place(of),
                enabled: true,
                depth: rows.len(),
                rule: false,
            });
        }
        rows
    }
}

impl NavWhat {
    /// What its toggle names it (`Shell::toggle_popup`).
    pub(crate) fn key(&self) -> String {
        format!("nav {self:?}")
    }
}

/// The rows of the menu of what is inside `of` (`App::places_in`): each a
/// step in, `current` (where the trail goes on) marked; the first
/// `FOLDER_ROWS`, then a row that opens `of` for the rest, or one that
/// says there is nothing inside.
fn inside_rows(
    app: &skwir_core::App,
    of: &skwir_core::Location,
    inside: Vec<skwir_core::location::Crumb>,
    current: Option<&skwir_core::Location>,
) -> Vec<NavRow> {
    let total = inside.len();
    let first_folder = inside.iter().position(|p| p.location.is_path());
    let navigate = |l: &skwir_core::Location| serde_json::json!({ "location": l.arg() });
    let mut rows: Vec<NavRow> = inside
        .into_iter()
        .take(FOLDER_ROWS)
        .map(|place| NavRow {
            label: place.label.clone().into(),
            tip: match place.location.path() {
                Some(path) => path.display().to_string().into(),
                None => app.location_title(&place.location).into(),
            },
            icon: crate::trail::glyph(app, &place.location),
            command: "navigate",
            args: navigate(&place.location),
            current: current.is_some_and(|c| c.same_place(&place.location)),
            enabled: true,
            depth: 0,
            rule: false,
        })
        .collect();
    // The folders apart from the places of the folder before them.
    if let Some(row) = first_folder
        .filter(|&i| i > 0)
        .and_then(|i| rows.get_mut(i))
    {
        row.rule = true;
    }
    let note = |label: String, enabled: bool, icon: &'static str| NavRow {
        tip: label.clone().into(),
        label: label.into(),
        icon,
        command: "navigate",
        args: navigate(of),
        current: false,
        enabled,
        depth: 0,
        rule: false,
    };
    if total == 0 {
        rows.push(note("Nothing inside".to_owned(), false, ""));
    } else if total > FOLDER_ROWS {
        rows.push(note(
            format!("{} more: open it", total - FOLDER_ROWS),
            true,
            "open",
        ));
    }
    rows
}

/// What is above `of`, a place of a trail of `segments` that is no
/// folder: the last of the folders that lead the trail, one inside the
/// other (those above it and it come first, under their own names), then
/// the trail's places down to `of`.
fn above_in_trail<'a>(
    segments: &'a [skwir_core::location::Crumb],
    of: &skwir_core::Location,
) -> Option<(Option<PathBuf>, &'a [skwir_core::location::Crumb])> {
    let at = segments.iter().position(|s| s.location.same_place(of))?;
    let upto = &segments[..=at];
    let folders = upto.iter().take_while(|s| s.location.is_path()).count();
    let folder = upto[..folders].last().and_then(|s| s.location.path());
    Some((folder, &upto[folders..]))
}

/// The rows of the menu of what a trail leaves out above `top`, its
/// first folder: every folder from the root down to `top` itself
/// (marked, with `mark`), each a step further in, under its own name as
/// the other chevrons' menus list folders: `/ › home › example`, never the
/// sidebar's `Home`.
fn parent_rows(top: &Path, mark: bool) -> Vec<NavRow> {
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
            current: mark && folder == top,
            enabled: true,
            depth,
            rule: false,
        })
        .collect()
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
    let rows = gpui::uniform_list(
        "nav-rows",
        menu.rows.len(),
        cx.processor(
            move |shell: &mut Shell, range: std::ops::Range<usize>, _, cx| {
                shell.with_pane(pane, |shell| rows(shell, range, cx))
            },
        ),
    )
    .track_scroll(&menu.scroll)
    .flex_1()
    .min_h_0();
    // As tall as its rows, shrunk to the room the popover has below its
    // anchor: the list fills it, and draws the rows that fit.
    let tall = ROW * menu.rows.len() as f32 + 2. * toolbar::POPUP_PAD + 2.;
    let panel = toolbar::popup_panel(&menu.popup, close, &theme, cx)
        .w(menu.width)
        .h(px(tall))
        .on_action(cx.listener(move |shell, action: &RunCommand, window, cx| {
            if !shell.walk_nav_menu(pane, &action.name, window, cx) {
                cx.propagate();
            }
        }))
        .child(rows);
    Some(panel.into_any_element())
}

/// The menu's rows in `range`, the only ones drawn.
fn rows(shell: &Shell, range: std::ops::Range<usize>, cx: &mut Context<Shell>) -> Vec<AnyElement> {
    let Some(menu) = shell.nav_menu.as_ref() else {
        return Vec::new();
    };
    let border = cx.global::<Theme>().border;
    range
        .filter_map(|i| {
            let (row, focus) = (menu.rows.get(i)?, menu.focus.get(i)?);
            let button = Button::new(
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
            .track_focus(focus)
            .when(row.depth > 0, |d| d.pl(px(8. + 12. * row.depth as f32)));
            Some(
                div()
                    .relative()
                    .h(px(ROW))
                    .pt(px(ROW - 30.))
                    .when(row.rule, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(1.5))
                                .left_0()
                                .right_0()
                                .h(px(1.))
                                .bg(border),
                        )
                    })
                    .child(button)
                    .into_any_element(),
            )
        })
        .collect()
}

impl Shell {
    /// The arrows, Tab, Home and End in the open menu of places: the row
    /// they reach (wrapping around, passing the rows that cannot be
    /// chosen), scrolled into view and given the keyboard. Only the rows
    /// in view are drawn, so GPUI's tab order, which the other popups
    /// walk, knows none beyond them. Whether `command` was one of those.
    fn walk_nav_menu(
        &mut self,
        pane: PaneId,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(menu) = self.nav_menu.as_ref().filter(|m| m.pane == pane) else {
            return false;
        };
        let stops: Vec<usize> = (0..menu.rows.len())
            .filter(|&i| menu.rows[i].enabled)
            .collect();
        let Some(last) = stops.len().checked_sub(1) else {
            return false;
        };
        let here = menu
            .focus
            .iter()
            .position(|focus| focus.is_focused(window))
            .and_then(|row| stops.iter().position(|&stop| stop == row));
        let (to, strategy) = match command {
            "ui.toolbar_next" => (
                here.map_or(0, |h| if h == last { 0 } else { h + 1 }),
                ScrollStrategy::Bottom,
            ),
            "ui.toolbar_prev" => (
                here.map_or(last, |h| h.checked_sub(1).unwrap_or(last)),
                ScrollStrategy::Top,
            ),
            "ui.toolbar_first" => (0, ScrollStrategy::Top),
            "ui.toolbar_last" => (last, ScrollStrategy::Bottom),
            _ => return false,
        };
        let row = stops[to];
        menu.scroll.scroll_to_item(row, strategy);
        window.focus(&menu.focus[row], cx);
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{above_in_trail, parent_rows};
    use skwir_core::fs::subfolders;
    use std::path::Path;

    /// From the root down to the folder the trail starts at, which is
    /// marked, each by its own name, whatever the sidebar calls it.
    #[test]
    fn the_folders_above_a_trail_go_from_the_root_down_to_it() {
        let rows = parent_rows(Path::new("/home/example/Documents"), true);
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
            serde_json::json!({ "location": skwir_core::os_path::PathArg::from(Path::new("/home")) })
        );
        let rows = parent_rows(Path::new("/srv/it's data"), true);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_ref()).collect();
        assert_eq!(labels, ["/", "srv", "it's data"]);
    }

    /// Above a place of a trail that is no folder: the folders leading
    /// the trail (the last of them, whose own folders come first), then
    /// its places down to it, as a narrow pane may have hidden them.
    #[test]
    fn above_a_place_of_a_trail_are_its_folders_then_its_places() {
        use skwir_core::Location;
        use skwir_core::location::Crumb;
        let segments = [
            Crumb::new("Home", Location::from_path("/home/example")),
            Crumb::new("Papers", Location::from_path("/home/example/Documents")),
            Crumb::new("Tags", Location::parse("tags:/")),
            Crumb::new("Places", Location::parse("tags:/Places")),
            Crumb::new("France", Location::parse("tags:/Places/France")),
        ];
        let (folder, places) = above_in_trail(&segments, &Location::parse("tags:/Places")).unwrap();
        assert_eq!(
            folder.as_deref(),
            Some(Path::new("/home/example/Documents"))
        );
        let labels: Vec<&str> = places.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Tags", "Places"]);
        // A trail of places only, as the trash's.
        let trash = [
            Crumb::new("Trash", Location::parse("trash:")),
            Crumb::new("old project", Location::parse("trash:/t/files/old project")),
            Crumb::new("sub", Location::parse("trash:/t/files/old project/sub")),
        ];
        let (folder, places) =
            above_in_trail(&trash, &Location::parse("trash:/t/files/old project/sub")).unwrap();
        assert_eq!(folder, None);
        assert_eq!(places.len(), 3);
        assert!(above_in_trail(&trash, &Location::parse("tags:/")).is_none());
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
        let mut folders = vec!["a 9", "a 10", "Été", "it's b"];
        if cfg!(unix) {
            folders.push("link");
        }
        assert_eq!(names(false), folders);
        assert_eq!(names(true)[0], ".hidden");
        assert!(subfolders(&tmp.path().join("missing"), false).is_err());
    }
}
