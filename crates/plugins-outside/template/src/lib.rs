//! A plugin from outside skwir, to start one from (`docs/plugins/README.md`):
//! a field Details shows (how many words a text holds) and a command
//! (how many words the files given hold). The same code is a WebAssembly
//! module (`export!`, built by `just plugins`) and a program that speaks
//! the protocol (`main.rs`).

use serde_json::{Value, json};
use skwir_plugin_api::guest::{Host, Plugin};
use skwir_plugin_api::protocol::{CommandAnswer, CommandParams, FieldParams, File};

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
        if params.command != "word-count.count" {
            return Err(format!("no command `{}`", params.command));
        }
        let mut files = Vec::new();
        let mut total = 0;
        for file in &params.entries {
            let count = words(host, file)?;
            total += count;
            files.push(json!({ "name": file.name, "words": count }));
        }
        Ok(CommandAnswer::Value {
            value: json!({ "files": files, "total": total }),
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
}
