//! skwir plugin: terminal. A shell in a panel under the panes, as in
//! Dolphin, that keeps to the folder shown (ADR-019).
//!
//! The plugin owns the pseudo-terminal and the VT engine
//! (`alacritty_terminal`) and publishes what the terminal shows as core's
//! `terminal::Screen`; the UI draws it and sends back `terminal::Input`. It
//! registers a `TerminalProvider` and the panel's key; the panel itself is
//! the shell's (`ui.terminal`), so disabling the plugin closes it and every
//! shell it started is hung up on.

mod follow;
mod keys;
mod links;
mod screen;
mod session;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use skwir_core::App;
use skwir_core::command::Invocation;
use skwir_core::location::Waker;
use skwir_core::plugin::{Category, Plugin};
use skwir_core::registry::KeyBinding;
use skwir_core::settings::SettingSpec;
use skwir_core::terminal::{Spawn, TerminalProvider, TerminalSession};

const ID: &str = "terminal";
const PROGRAM_KEY: &str = "terminal.program";
const SCROLLBACK_KEY: &str = "terminal.scrollback";
const FOLLOW_KEY: &str = "terminal.follow";
/// Lines of history by default: what `less` of a long build log needs.
/// A cell takes 24 bytes (alacritty's own test holds it to that), so
/// 10,000 lines of a 120-column terminal take about 29 MB once full.
const SCROLLBACK: i64 = 10_000;
/// The most lines of history the setting allows: about 86 MB once full at
/// 120 columns, 145 MB at 200. A hundred thousand held 290 to 480 MB for
/// a terminal beside a file manager, a terminal of its own's to keep.
/// A value saved before above it is read as it.
const SCROLLBACK_MOST: i64 = 30_000;

struct Provider {
    options: Mutex<session::Options>,
    /// Every session started, to hang up on when the plugin is disabled.
    sessions: Mutex<Vec<Weak<session::Shared>>>,
}

impl TerminalProvider for Provider {
    fn open(&self, spawn: Spawn, waker: Waker) -> anyhow::Result<Arc<dyn TerminalSession>> {
        let options = self.options.lock().clone();
        let (handle, shared) = session::open(spawn, waker, &options)?;
        let mut sessions = self.sessions.lock();
        sessions.retain(|session| session.strong_count() > 0);
        sessions.push(Arc::downgrade(&shared));
        Ok(handle)
    }
}

impl Provider {
    fn stop_all(&self) {
        for session in self.sessions.lock().drain(..) {
            if let Some(session) = session.upgrade() {
                session.stop();
            }
        }
    }
}

fn read_options(app: &App, follow: Arc<AtomicBool>) -> session::Options {
    follow.store(
        app.settings.get::<bool>(FOLLOW_KEY).unwrap_or(true),
        Ordering::Relaxed,
    );
    session::Options {
        program: app
            .settings
            .get::<String>(PROGRAM_KEY)
            .map(|program| program.trim().to_owned())
            .filter(|program| !program.is_empty()),
        scrollback: app
            .settings
            .get::<i64>(SCROLLBACK_KEY)
            .unwrap_or(SCROLLBACK)
            .clamp(0, SCROLLBACK_MOST)
            .try_into()
            .unwrap_or(0),
        follow,
    }
}

#[derive(Default)]
pub struct TerminalPlugin {
    provider: Mutex<Option<Arc<Provider>>>,
}

impl Plugin for TerminalPlugin {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Terminal"
    }
    fn description(&self) -> &'static str {
        "A terminal under the panes (F4) that goes where the pane goes, and takes the pane where you cd"
    }
    fn category(&self) -> Category {
        Category::Desktop
    }
    fn capabilities(&self) -> skwir_core::permissions::Capabilities {
        use skwir_core::permissions::Capability;
        skwir_core::permissions::Capabilities::of(&[Capability::Programs])
    }
    fn register(&self, app: &App) {
        app.settings.declare(SettingSpec::command(
            PROGRAM_KEY,
            "Shell",
            "The program the terminal runs. Leave empty for your login shell",
            "",
        ));
        app.settings.declare(
            SettingSpec::integer(
                SCROLLBACK_KEY,
                "History",
                "How many lines you can scroll back to, for terminals opened from now on, at most 30,000. Once full, every 10,000 lines of a wide terminal take about 30 MB",
                SCROLLBACK,
                (0, SCROLLBACK_MOST),
            )
            .unit("lines")
            .step(1000),
        );
        app.settings.declare(SettingSpec::toggle(
            FOLLOW_KEY,
            "Follow the folder shown",
            "The terminal changes to the folder the pane shows, and the pane goes where you cd. Nothing is typed while a program runs in the terminal",
            true,
        ));
        app.registry.keymap.add(
            ID,
            KeyBinding {
                keys: "f4".into(),
                command: Invocation::new("ui.terminal"),
                when: None,
            },
        );
        let provider = Arc::new(Provider {
            options: Mutex::new(read_options(app, Arc::new(AtomicBool::new(true)))),
            sessions: Mutex::default(),
        });
        app.terminals.register(ID, Arc::clone(&provider) as _);
        *self.provider.lock() = Some(provider);
    }
    fn on_settings_changed(&self, app: &App, keys: &[String]) {
        if !keys.iter().any(|key| key.starts_with("terminal.")) {
            return;
        }
        if let Some(provider) = self.provider.lock().as_ref() {
            let mut options = provider.options.lock();
            *options = read_options(app, Arc::clone(&options.follow));
        }
    }
    fn on_disable(&self, _: &App) {
        if let Some(provider) = self.provider.lock().take() {
            provider.stop_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// History is read as set, up to the most the setting allows: a
    /// value saved when more was allowed is read as that most.
    #[test]
    fn history_is_capped_whatever_was_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        let follow = Arc::new(AtomicBool::new(true));
        assert_eq!(read_options(&app, Arc::clone(&follow)).scrollback, 10_000);
        app.settings
            .set(SCROLLBACK_KEY, toml::Value::Integer(20_000));
        assert_eq!(read_options(&app, Arc::clone(&follow)).scrollback, 20_000);
        app.settings
            .set(SCROLLBACK_KEY, toml::Value::Integer(100_000));
        assert_eq!(read_options(&app, follow).scrollback, 30_000);
    }
}
