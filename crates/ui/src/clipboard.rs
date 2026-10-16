//! Files on the system's clipboard (ADR-062). What a copy or a cut hands the
//! window (`Outcome::Files`) goes there as Linux file managers put it, with
//! the paths as text; a command that wants the clipboard (`WantsClipboard`:
//! a paste with nothing in Skwir's own) runs again with what another
//! application put there; once Skwir's files are gone from it, Skwir's own
//! clipboard forgets them, and once Skwir's own is empty (a cut pasted, or
//! forgotten), so is the system's of Skwir's files.
//!
//! Built on Linux; elsewhere Skwir's own clipboard stays apart from the
//! system's (`skwir_core::system::not_yet`'s rule: the others' answer is
//! beside it).

use std::path::PathBuf;

use gpui::{Context, Window};
use skwir_core::command::Invocation;

use crate::shell::Shell;

/// What of Skwir's the system's clipboard may hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Held {
    /// Nothing Skwir put there, or not any more.
    #[default]
    Nothing,
    /// Files a copy or a cut put there, until anything else is copied.
    Files,
}

/// The paths as text, one a line, as Copy Path writes them; a path that is
/// no text as its URI, which names it byte for byte (Thunar's choice).
#[cfg(target_os = "linux")]
fn text_of(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| match path.to_str() {
            Some(text) => text.to_owned(),
            None => skwir_platform::uri::file_uri(path),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl Shell {
    /// Put the files a copy or a cut handed over on the system's clipboard.
    pub(crate) fn put_files_on_clipboard(
        &mut self,
        paths: Vec<PathBuf>,
        cut: bool,
        cx: &mut Context<Self>,
    ) {
        #[cfg(target_os = "linux")]
        {
            let text = text_of(&paths);
            cx.write_files_to_clipboard(
                gpui::ClipboardFiles {
                    paths,
                    cut,
                    others: Vec::new(),
                },
                text,
            );
            self.clipboard_holds = Held::Files;
        }
        // Not yet: Skwir's own clipboard holds them, apart (ADR-062).
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (paths, cut, cx);
        }
    }

    /// Run `invocation` again with the files another application put on the
    /// system's clipboard as its `clipboard`: read off the UI thread, then
    /// each read from the disk on a worker, as a drop from another
    /// application is. The pane it was asked in is the one it pastes into,
    /// whichever is active once that is done.
    pub(crate) fn run_with_clipboard(
        &mut self,
        invocation: Invocation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (invocation, window);
            self.set_status("Nothing to paste", false, cx);
        }
        #[cfg(target_os = "linux")]
        {
            let mut invocation = invocation;
            if !invocation.args.is_object() {
                invocation.args = serde_json::json!({});
            }
            if invocation.args.get("pane").is_none() {
                invocation.args["pane"] = serde_json::json!(self.pane);
            }
            let read = cx.read_files_from_clipboard();
            cx.spawn_in(window, async move |this, cx| {
                let files = read.await;
                this.update_in(cx, |shell, window, cx| {
                    shell.paste_files(invocation, files, window, cx);
                })
                .ok();
            })
            .detach();
        }
    }

    /// What was read of the system's clipboard, for `invocation`.
    #[cfg(target_os = "linux")]
    fn paste_files(
        &mut self,
        invocation: Invocation,
        files: Option<gpui::ClipboardFiles>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use skwir_core::command::ClipboardArg;
        let Some(files) = files else {
            self.set_status("Nothing to paste", false, cx);
            return;
        };
        if files.paths.is_empty() {
            // Skwir speaks no file-system protocol (ADR-021).
            self.set_status(unreachable(&files.others), true, cx);
            return;
        }
        let others = files.others;
        let cut = files.cut;
        let paths = files.paths;
        self.off_thread(
            "clipboard-entries",
            // Another application may name a file that is not there (Dolphin
            // writes a name that is no UTF-8 as another): said, not pasted.
            move || {
                let entries: Vec<_> = paths
                    .iter()
                    .filter_map(skwir_core::op::EntryRef::from_path)
                    .collect();
                let gone = paths.len() - entries.len();
                (entries, gone)
            },
            move |shell, (entries, gone): (Vec<skwir_core::op::EntryRef>, usize), window, cx| {
                if entries.is_empty() {
                    shell.set_status("The copied items no longer exist", true, cx);
                    return;
                }
                if gone > 0 {
                    let noun = if gone == 1 {
                        "item no longer exists"
                    } else {
                        "items no longer exist"
                    };
                    shell.set_status(format!("{gone} copied {noun}"), true, cx);
                }
                if !others.is_empty() {
                    shell.set_status(unreachable(&others), true, cx);
                }
                let mut invocation = invocation;
                invocation.args[ClipboardArg::FIELD] = serde_json::json!(ClipboardArg {
                    entries: entries.into_iter().map(Into::into).collect(),
                    cut,
                });
                shell.run(&invocation.name, invocation.args, window, cx);
            },
            window,
            cx,
        );
    }

    /// The files Skwir put on the system's clipboard are gone from it:
    /// Skwir's own forgets them, and cut entries are no longer dimmed.
    pub(crate) fn clipboard_files_gone(&mut self, cx: &mut Context<Self>) {
        if self.clipboard_holds != Held::Files {
            return;
        }
        self.clipboard_holds = Held::Nothing;
        if !self.app.pending.get().is_empty()
            && let Err(error) = self.app.dispatch(&Invocation::new("clipboard_clear"))
        {
            tracing::debug!(%error, "forgetting the clipboard");
        }
        cx.notify();
    }

    /// Once Skwir's own clipboard is empty (a cut all pasted, or forgotten),
    /// the system's holds none of its files any more. Asked at each sync.
    pub(crate) fn sync_clipboard_files(&mut self, cx: &mut Context<Self>) {
        if self.clipboard_holds == Held::Files && self.pending.is_empty() {
            self.clipboard_holds = Held::Nothing;
            cx.clear_clipboard_files();
        }
    }
}

/// What the clipboard named that Skwir cannot reach: another protocol's
/// locations (ADR-021), by the first one's scheme.
#[cfg(target_os = "linux")]
fn unreachable(others: &[String]) -> String {
    let scheme = others
        .first()
        .and_then(|uri| uri.split_once(':'))
        .map_or("remote", |(scheme, _)| scheme);
    let n = others.len();
    format!(
        "The clipboard holds {n} {scheme}: {} Skwir cannot reach",
        if n == 1 { "item" } else { "items" },
    )
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn the_text_is_the_paths_and_a_name_that_is_no_text_its_uri() {
        let mut paths = vec![PathBuf::from("/data/a b.txt"), PathBuf::from("/data/it's")];
        assert_eq!(text_of(&paths), "/data/a b.txt\n/data/it's");
        paths.push(PathBuf::from(skwir_core::os_path::from_bytes(
            b"/data/lat\xe9.txt".to_vec(),
        )));
        assert!(text_of(&paths).ends_with("\nfile:///data/lat%E9.txt"));
    }

    #[test]
    fn what_cannot_be_reached_is_said_by_its_scheme() {
        assert_eq!(
            unreachable(&["sftp://host/a".into()]),
            "The clipboard holds 1 sftp: item Skwir cannot reach"
        );
        assert_eq!(
            unreachable(&["smb://h/a".into(), "smb://h/b".into()]),
            "The clipboard holds 2 smb: items Skwir cannot reach"
        );
    }
}
