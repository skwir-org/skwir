//! A plugin from outside skwir, to start one from (`docs/plugins/README.md`):
//! a field Properties shows (how many words a text holds) and two commands
//! (how many words the files given hold: a value for scripts and agents,
//! and a number for the clipboard). The same code is a WebAssembly module
//! (`export!`, built by `just plugins`) and a program that speaks the
//! protocol (`main.rs`).
//!
//! It speaks protocol 2: what it says to a person is a message of its
//! catalogs (`l10n/`, `en.ftl` its English), which skwir says in the
//! reader's language, `Words::message("count-said").with("count", n)`.

use serde_json::{Value, json};
use skwir_plugin_api::guest::{Host, Plugin};
use skwir_plugin_api::protocol::{CommandAnswer, CommandParams, FieldParams, File, Words};

/// How much of a file is read, a page at a time: a field is cheap.
const LIMIT: u64 = 2 * 1024 * 1024;
const PAGE: u32 = 64 * 1024;

#[derive(Default)]
pub struct WordCount;

impl Plugin for WordCount {
    fn field(&self, host: &dyn Host, params: FieldParams) -> Result<Option<Value>, String> {
        match params.key.as_str() {
            "word-count.words" => Ok(Some(json!(words(host, &params.file)?))),
            _ => Ok(None),
        }
    }

    fn command(&self, host: &dyn Host, params: CommandParams) -> Result<CommandAnswer, String> {
        let copy = match params.command.as_str() {
            "word-count.count" => false,
            "word-count.copy" => true,
            // Said in the reader's language, as every error may be.
            _ => {
                return Err(Words::message("no-command")
                    .with("command", params.command)
                    .into());
            }
        };
        let mut files = Vec::new();
        let mut total = 0;
        for file in &params.entries {
            let count = words(host, file)?;
            total += count;
            files.push(json!({ "name": file.name, "words": count }));
        }
        Ok(if copy {
            // The number for the clipboard, and what is said of it.
            CommandAnswer::Text {
                text: total.to_string(),
                said: Words::message("count-said").with("count", total),
            }
        } else {
            CommandAnswer::Value {
                value: json!({ "files": files, "total": total }),
            }
        })
    }
}

/// The words of `file`'s first `LIMIT` bytes, as text: runs of anything
/// but white space. A word cut by a page's end is counted once.
pub fn words(host: &dyn Host, file: &File) -> Result<u64, String> {
    let mut count = 0;
    let mut inside = false;
    let mut offset = 0;
    while offset < LIMIT {
        let bytes = host.read(file.handle, offset, PAGE)?;
        if bytes.is_empty() {
            break;
        }
        for byte in &bytes {
            let space = byte.is_ascii_whitespace();
            if !space && !inside {
                count += 1;
            }
            inside = !space;
        }
        offset += bytes.len() as u64;
    }
    Ok(count)
}

skwir_plugin_api::export!(WordCount);

#[cfg(test)]
mod tests {
    use super::*;

    struct Text(&'static str);

    impl Host for Text {
        fn read(&self, _: u32, offset: u64, length: u32) -> Result<Vec<u8>, String> {
            let bytes = self.0.as_bytes();
            let start = usize::try_from(offset).unwrap().min(bytes.len());
            let end = (start + length as usize).min(bytes.len());
            Ok(bytes[start..end].to_vec())
        }
        fn log(&self, _: &str, _: &str) {}
    }

    fn file() -> File {
        File {
            handle: 1,
            name: "note.txt".into(),
            path: "/note.txt".into(),
            mime: Some("text/plain".into()),
            size: None,
        }
    }

    #[test]
    fn words_are_runs_between_white_space() {
        assert_eq!(words(&Text("one two  three\nfour"), &file()), Ok(4));
        assert_eq!(words(&Text("  "), &file()), Ok(0));
        assert_eq!(words(&Text("élan vital"), &file()), Ok(2));
    }

    /// What it answers for the clipboard is a message of its catalogs,
    /// counted; a command it has not fails as one.
    #[test]
    fn it_says_its_words_as_messages() {
        let asked = |command: &str| {
            WordCount.command(
                &Text("one two three"),
                CommandParams {
                    command: command.into(),
                    args: Value::Null,
                    entries: vec![file()],
                },
            )
        };
        assert_eq!(
            asked("word-count.copy"),
            Ok(CommandAnswer::Text {
                text: "3".into(),
                said: Words::message("count-said").with("count", 3),
            })
        );
        let failed = asked("word-count.other").unwrap_err();
        assert_eq!(
            Words::in_error(&failed),
            Some(Words::message("no-command").with("command", "word-count.other"))
        );
    }
}
