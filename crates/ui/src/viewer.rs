//! A window viewing files (`skwir view`, ADR-151): peek's card, the file's
//! bar and its preview, filling a window of its own (`preview::render_viewed`),
//! in the Skwir running, which its launch handed them to and waits on
//! (`handover::view_elsewhere`), else in a program that started only what
//! viewing needs. Its pane, never drawn, shows the file's folder, the
//! cursor on the file: what the preview shows is the pane's, as everywhere.
//! Steps go through that folder, or through the files given
//! (`Viewer::files`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gpui::{Context, Window};
use skwir_core::Location;
use skwir_core::words;
use skwir_core::words::Words;

use crate::shell::Shell;

/// The size a viewer's window was left at (ADR-151, 3), apart from the
/// user's windows (`session::SESSION_KEY`) and the dialogs'.
pub const SIZE_KEY: &str = "ui.viewer_size";

/// A viewer's size until one was left, at 100 %.
const WIDTH: f32 = 960.;
const HEIGHT: f32 = 720.;

/// What a launch asks to view (`skwir view`): the files, each a whole path,
/// the file standing for standard input, removed once the window closes,
/// and the desktop's token for the window.
pub use skwir_platform::handover::Viewing;

/// The files a viewer's window views.
pub(crate) struct Viewer {
    /// Given several, the steps go through them alone, in this order; one,
    /// through its folder, as peek's do.
    files: Vec<PathBuf>,
    /// The one of `files` shown.
    at: usize,
    /// What standard input gave, removed as the window goes in a viewer's
    /// own program; a launch that handed it over removes its own (told its
    /// window closed, or that the Skwir showing it ended), and the platform
    /// the spool of one gone first (ADR-151, 7).
    piped: Option<PathBuf>,
    /// The launch that handed them over and waits for the window to close
    /// (ADR-151, 1); none in a viewer's own program.
    handed: Option<skwir_platform::handover::Viewed>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Size {
    width: f32,
    height: f32,
}

/// The folder a viewer's pane shows for `file`, and the name it selects
/// there: its parent and its name, a folder with none (`/`) itself.
pub(crate) fn place_of(file: &Path) -> (PathBuf, Option<OsString>) {
    match (file.parent(), file.file_name()) {
        (Some(folder), Some(name)) => (folder.to_path_buf(), Some(name.to_os_string())),
        _ => (file.to_path_buf(), None),
    }
}

/// What the window keeps: its size (`SIZE_KEY`).
pub(crate) fn declare(app: &skwir_core::App) {
    app.settings
        .declare(skwir_core::settings::SettingSpec::state(
            SIZE_KEY,
            words!(
                viewer_size_setting = "The size of the last window viewing a file (skwir view)",
                note = "What a kept setting holds: the size the last window showing one file's preview alone was left at, so the next opens so."
            ),
            toml::Value::Table(toml::map::Map::new()),
        ));
}

/// Where a viewer's window opens: the size the last was left at, else a
/// viewer's, the compositor placing it, never maximized.
pub(crate) fn placement(app: &skwir_core::App) -> crate::session::Placement {
    let kept = app
        .settings
        .get::<Size>(SIZE_KEY)
        .filter(|size| size.width.is_finite() && size.height.is_finite())
        .filter(|size| size.width > 0. && size.height > 0.);
    crate::session::Placement {
        width: kept.map_or(WIDTH, |size| size.width),
        height: kept.map_or(HEIGHT, |size| size.height),
        maximized: false,
        at: None,
    }
}

/// A viewer's title: the name of the file shown, then skwir's.
pub(crate) fn title(shell: &Shell) -> Words {
    crate::title_bar::window_title(&Words::as_written(shell.preview.info.name.clone()))
}

impl Shell {
    /// This window views `files` (ADR-151): its preview drawn in peek's card,
    /// which takes the keyboard. Its pane was opened on the first one's
    /// folder (`finishing::open_viewer`). `piped` goes with the window;
    /// `handed`, the launch waiting for it, is told it closed, and closes
    /// it by going.
    pub(crate) fn view_files(
        &mut self,
        files: Vec<PathBuf>,
        piped: Option<PathBuf>,
        handed: Option<skwir_platform::handover::Viewed>,
        cx: &mut Context<Self>,
    ) {
        if let Some(handed) = &handed {
            // Its launch gone (its terminal's Ctrl+C): the window goes at
            // its next wake (`viewer_sync`).
            let wake = self.waker();
            handed.when_gone(move || wake());
        }
        self.viewer = Some(Viewer {
            files,
            at: 0,
            piped,
            handed,
        });
        self.preview.peek = true;
        self.preview.peek_focus = Some(cx.focus_handle());
    }

    /// Whether this window is a viewer's own program's (ADR-151, 1), the one
    /// window of that program: no launch handed it its files.
    pub(crate) fn views_alone(&self) -> bool {
        self.viewer
            .as_ref()
            .is_some_and(|viewer| viewer.handed.is_none())
    }

    /// The window goes once the launch that handed its files over went:
    /// whether it went.
    pub(crate) fn viewer_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let gone = self
            .viewer
            .as_ref()
            .and_then(|viewer| viewer.handed.as_ref())
            .is_some_and(skwir_platform::handover::Viewed::is_gone);
        if gone {
            tracing::info!("the launch viewing these files went");
            self.leave(window, cx);
            window.remove_window();
        }
        gone
    }

    /// The viewer's card takes the keyboard as the window opens: whether
    /// this window is a viewer's.
    pub(crate) fn viewer_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.viewer.is_none() {
            return false;
        }
        if let Some(focus) = &self.preview.peek_focus {
            window.focus(focus, cx);
        }
        true
    }

    /// `ui.viewer_step`: the file `delta` after the one shown, of the files
    /// given. Whether it stepped so; given one file, it steps through its
    /// folder as peek does, which the caller asks (`ui.cursor`).
    pub(crate) fn viewer_step(&mut self, delta: i64, cx: &mut Context<Self>) -> bool {
        let Some(viewer) = &mut self.viewer else {
            return false;
        };
        if viewer.files.len() < 2 {
            return false;
        }
        let last = viewer.files.len() - 1;
        let at = i64::try_from(viewer.at).unwrap_or(0).saturating_add(delta);
        let at = usize::try_from(at.max(0)).unwrap_or(0).min(last);
        if at == viewer.at {
            return true;
        }
        viewer.at = at;
        let file = viewer.files[at].clone();
        let (folder, name) = place_of(&file);
        let folder = Location::from_path(folder);
        let shown = self.app.pane_location(self.pane).as_ref() == Some(&folder);
        if !shown && let Err(error) = self.app.navigate(self.pane, &folder) {
            self.set_status(Words::of_error(error), true, cx);
            return true;
        }
        self.app.reveal_names(self.pane, name.into_iter().collect());
        cx.notify();
        true
    }

    /// `ui.viewer_reveal`: the folder of the file shown, in Skwir, the file
    /// selected: a launch of the program, which a Skwir running takes over
    /// (ADR-122), naming the file.
    pub(crate) fn viewer_reveal(&mut self, cx: &mut Context<Self>) {
        let shown = self.cursor_id().and_then(|id| {
            self.app
                .workspace
                .read()
                .pane(self.pane)
                .and_then(|pane| pane.path_of(id))
        });
        let Some(path) = shown else {
            return;
        };
        // On its own, as a launch from the desktop would be: in a process
        // group of its own (a Ctrl+C in the terminal a viewer's own program
        // waits in does not reach the Skwir it starts), its exit collected
        // (the Skwir running hands it over and ends at once).
        let started = own_program().and_then(|program| {
            skwir_platform::process::spawn_detached(
                skwir_platform::process::command(program)
                    .arg(&path)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null()),
            )
        });
        if let Err(error) = started {
            self.set_status(
                words!(
                    viewer_reveal_failed = "Skwir did not start: { $error }",
                    error = error.to_string(),
                    note = "Said in a viewer's window when Show in Skwir could not start Skwir. error: why, as the system says it."
                ),
                true,
                cx,
            );
        }
    }

    /// The viewer's window goes (`leave`): its size kept for the next
    /// (`SIZE_KEY`), the size it was given while it floated or the user
    /// dragged it to, never a tile's (`Placement::of_dialog`, as a dialog's
    /// is kept, ADR-144, 4), saved now; what standard input gave removed
    /// in a viewer's own program; the launch waiting told; its window out
    /// of the core.
    pub(crate) fn viewer_let_go(&mut self, cx: &mut Context<Self>) {
        let Some(viewer) = self.viewer.take() else {
            return;
        };
        if viewer.handed.is_none()
            && let Some(piped) = viewer.piped.clone()
        {
            // A file of up to 1 GiB, never removed on the window's thread.
            self.app.scheduler.spawn(
                skwir_core::scheduler::Priority::Background,
                "viewer-piped",
                skwir_core::CancelToken::new(),
                move || let_go_of_piped(&piped),
            );
        }
        if let Some(handed) = &viewer.handed {
            handed.close();
        }
        // As the window last moved or was resized (`Shell::window_moved`):
        // maximized or on the whole screen, the size it goes back to.
        let size = Size {
            width: self.placement.width,
            height: self.placement.height,
        };
        if let Ok(value) = toml::Value::try_from(size) {
            self.app.settings.set(SIZE_KEY, value);
            self.save_settings_later();
        }
        if self.app.workspace.read().window(self.window_id).is_none() {
            return;
        }
        let home = skwir_core::os_path::home().unwrap_or_else(|| PathBuf::from("/"));
        let folder = skwir_core::settings::startup_folder(&self.app.settings, &home);
        match self.app.close_window_apart(self.window_id, &folder) {
            // The core's last, a window of the user's own took its place:
            // asleep until one is asked, as the others, never opened unasked
            // at the next wake (`follow_windows`).
            Ok(Some(placed)) => crate::finishing::sleeps(cx, placed),
            Ok(None) => {}
            Err(error) => tracing::warn!("a viewer's window closes: {error:#}"),
        }
    }
}

/// The program Show in Skwir starts: this one, by the file it is, links
/// followed. A viewer started as `skw` may name that link (macOS says the
/// path it was started by), and a program started under it views
/// (ADR-151, 1) rather than open the folder.
fn own_program() -> std::io::Result<PathBuf> {
    let program = std::env::current_exe()?;
    #[cfg(unix)]
    if let Ok(followed) = std::fs::canonicalize(&program) {
        return Ok(followed);
    }
    Ok(program)
}

/// Remove the file standing for standard input (`Viewing::piped`), and the
/// folder of its launch's that holds it, in a viewer's own program, as its
/// window closes and as it ends, however it does: as a launch removes its
/// own (`handover::let_go_of_spool`).
pub(crate) fn let_go_of_piped(piped: &Path) {
    skwir_platform::handover::let_go_of_spool(piped);
}
