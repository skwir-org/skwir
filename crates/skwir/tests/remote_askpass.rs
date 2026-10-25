//! What a tool asks while Skwir runs it reaches the window through the
//! `skwir` executable (ADR-077): rclone runs it as its password command,
//! which asks over the connection's socket, on Windows its named pipe
//! (`skwir_platform::askpass`). Against a configuration of the test's own,
//! with the rclone installed, or under Wine rclone's own Windows release
//! (`SKWIR_TEST_RCLONE`, `just test-windows`); skipped, saying why, with
//! none. Never the user's rclone configuration.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use skwir_core::cancel::CancelToken;
use skwir_plugin_remote::connector::Ask;
use skwir_plugin_remote::rclone::Rclone;
use skwir_plugin_remote::rclone_config::{self, Session};

/// rclone's own Windows release under Wine, else the one installed.
fn rclone() -> Option<PathBuf> {
    std::env::var_os("SKWIR_TEST_RCLONE")
        .map(PathBuf::from)
        .or_else(|| skwir_core::os_path::find_program("rclone"))
}

/// Answers as the window would, with `answer` (Cancel where `None`), and
/// says what it was asked.
struct Window {
    answer: Option<&'static str>,
    asked: Mutex<Vec<String>>,
}

impl Window {
    fn answering(answer: Option<&'static str>) -> Arc<Window> {
        Arc::new(Window {
            answer,
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Ask for Window {
    fn secret(&self, title: &str, _: &str, _: &CancelToken) -> Option<String> {
        self.asked.lock().unwrap().push(title.to_owned());
        self.answer.map(str::to_owned)
    }
    fn confirm(&self, _: &str, _: &str, _: &CancelToken) -> bool {
        false
    }
}

const PASSWORD: &str = "a pass phrase, it's \"mine\" ✓";

#[test]
fn an_encrypted_configuration_s_password_is_asked_of_the_window_through_skwir() {
    let Some(rclone) = rclone() else {
        eprintln!("skipped: rclone is not installed");
        return;
    };
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("rclone.conf");
    std::fs::write(&config, "[nas]\ntype = local\n").unwrap();
    let with = Rclone {
        config: Some(config.clone()),
        // As rclone runs it: a whole path, on Windows a drive's.
        askpass: Some(std::path::absolute(env!("CARGO_BIN_EXE_skwir")).unwrap()),
        ..Rclone::default()
    };
    let cancel = CancelToken::new();

    // Encrypted with a password given through the `skwir` executable,
    // never on rclone's command line.
    rclone_config::encrypt(&rclone, &with, PASSWORD, &cancel).unwrap();
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(
        text.starts_with("# Encrypted rclone configuration File"),
        "{text}"
    );
    assert!(rclone_config::encrypted(&rclone, &with, &cancel).unwrap());

    // Opened with the password the window gives, asked once.
    let window = Window::answering(Some(PASSWORD));
    let ask: Arc<dyn Ask> = window.clone();
    let session = Session::start(&rclone, &with, &ask, &cancel).unwrap();
    assert_eq!(session.remotes(&cancel).unwrap(), ["nas"]);
    drop(session);
    assert_eq!(window.asked(), [rclone_config::PROMPT]);

    // Cancelled in the window, it does not open; nor with a wrong one.
    for answer in [None, Some("not it")] {
        let window = Window::answering(answer);
        let ask: Arc<dyn Ask> = window.clone();
        let opened = Session::start(&rclone, &with, &ask, &cancel)
            .and_then(|session| session.remotes(&cancel));
        assert!(opened.is_err(), "{answer:?}: {opened:?}");
        assert_eq!(window.asked(), [rclone_config::PROMPT], "{answer:?}");
    }
}
