//! The methods bibli and a plugin from outside call, and their JSON
//! (ADR-045). A program speaks them as JSON-RPC 2.0, one message per line
//! on its standard input and output ([`Message`]); a WebAssembly module
//! answers them through its one `call` export, and calls bibli through
//! its imports, `read` and `log` (`wit/plugin.wit`).
//!
//! bibli asks ([`method`]): `initialize` (a program, first), `field`,
//! `thumbnail`, `inspect`, `convert` (version 2), `page` (version 3),
//! `command`. A plugin asks back, while it
//! answers: `read` (a file of the call, by its handle), `log`. bibli
//! sends a program `$/cancel` when it stops waiting for an answer, and
//! closes its standard input when it is no longer needed.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The version of the protocol this crate speaks (`api` in a manifest).
/// Version 2 added `convert` (a preview that is another file), version
/// 3 `page` (a preview a page at a time).
pub const VERSION: u32 = 3;

/// The oldest version bibli still speaks: a plugin written for it runs
/// unchanged.
pub const OLDEST: u32 = 1;

/// The longest message, in bytes with its newline: a thumbnail's PNG or a
/// page of bytes read, in base64.
pub const MESSAGE_LIMIT: usize = 64 * 1024 * 1024;

/// The most bytes one `read` gives.
pub const READ_LIMIT: u32 = 4 * 1024 * 1024;

/// The methods' names.
pub mod method {
    /// bibli → a program, first: [`InitializeParams`](super::InitializeParams).
    pub const INITIALIZE: &str = "initialize";
    /// bibli → plugin: [`FieldParams`](super::FieldParams) → [`FieldAnswer`](super::FieldAnswer).
    pub const FIELD: &str = "field";
    /// bibli → plugin: [`ThumbnailParams`](super::ThumbnailParams) →
    /// [`ThumbnailAnswer`](super::ThumbnailAnswer).
    pub const THUMBNAIL: &str = "thumbnail";
    /// bibli → plugin: [`InspectParams`](super::InspectParams) → an
    /// [`Inspection`](super::Inspection) or null.
    pub const INSPECT: &str = "inspect";
    /// bibli → plugin, version 2: [`ConvertParams`](super::ConvertParams)
    /// → a [`ConvertAnswer`](super::ConvertAnswer) or null.
    pub const CONVERT: &str = "convert";
    /// bibli → plugin, version 3: [`PageParams`](super::PageParams) →
    /// a [`PageAnswer`](super::PageAnswer) or null.
    pub const PAGE: &str = "page";
    /// bibli → plugin: [`CommandParams`](super::CommandParams) →
    /// [`CommandAnswer`](super::CommandAnswer).
    pub const COMMAND: &str = "command";
    /// bibli → a program, a notification: stop answering `{ "id" }`.
    pub const CANCEL: &str = "$/cancel";
    /// plugin → bibli: [`ReadParams`](super::ReadParams) → [`ReadAnswer`](super::ReadAnswer).
    pub const READ: &str = "read";
    /// plugin → bibli, a notification: [`LogParams`](super::LogParams).
    pub const LOG: &str = "log";
}

/// A file a call is about. Its bytes are read through bibli by `handle`,
/// valid while the call is answered; its path and name are for showing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub handle: u32,
    /// Its name, as text (a name that is no UTF-8 is shown lossily).
    pub name: String,
    /// Its path, as text, for showing.
    pub path: String,
    /// Its MIME type, when known.
    #[serde(default)]
    pub mime: Option<String>,
    /// Its size in bytes, when known.
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeParams {
    /// The protocol's version bibli speaks.
    pub api: u32,
    /// What the plugin may do, as granted.
    pub capabilities: Vec<crate::manifest::Capability>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializeAnswer {
    /// The protocol's version the plugin speaks.
    pub api: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldParams {
    /// One of the manifest's field keys.
    pub key: String,
    pub file: File,
}

/// A field's value: a string, a number, a boolean or a list of them;
/// null when the file says nothing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FieldAnswer {
    #[serde(default)]
    pub value: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbnailParams {
    pub file: File,
    /// The side of the square the picture fits, in pixels.
    pub size: u32,
}

/// A thumbnail: a PNG, in base64; null when it has none for the file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbnailAnswer {
    #[serde(default)]
    pub png: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectParams {
    pub file: File,
    /// Where the page asked starts: a byte, a row, a section, as the
    /// plugin numbers them (its `previous` and `next`).
    #[serde(default)]
    pub offset: u64,
    /// What to find, from `offset` on: the answer's `position` says where.
    #[serde(default)]
    pub query: Option<String>,
    /// Asked for its source rather than its presentation.
    #[serde(default)]
    pub source: bool,
    /// A member it listed (a row's `target`), asked for.
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvertParams {
    pub file: File,
}

/// A preview that is another file (version 2): one of a format bibli
/// shows (a PDF, a PNG, Markdown, CSV, GPX…), which bibli previews as it
/// would that file, typed by its name and bytes. Null: the plugin shows
/// nothing for this one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvertAnswer {
    /// Its name, whose extension says its type (`page.pdf`).
    pub name: String,
    /// Its bytes, in base64: at most what a message holds.
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageParams {
    pub file: File,
    /// Which page, from 1.
    pub page: u32,
    /// The longest side to draw it at, in pixels; 0 asks no picture (how
    /// many pages there are, a page's text alone).
    pub side: u32,
    /// Its text asked too, where it lies on the page.
    #[serde(default)]
    pub text: bool,
}

/// A page of a preview shown a page at a time (version 3): how many
/// pages the file has, and the one asked: its picture as a PNG, in
/// base64 (null past the last, or when no picture was asked), its own
/// size, and its text when asked. Null: the plugin shows nothing for
/// this file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageAnswer {
    pub pages: u32,
    #[serde(default)]
    pub png: Option<String>,
    /// The page's own size, in CSS pixels (1/96 inch): what 100% shows.
    /// Its picture's, as drawn, when not said.
    #[serde(default)]
    pub width: Option<f32>,
    #[serde(default)]
    pub height: Option<f32>,
    /// Its text, in reading order: what is selected, copied and searched.
    #[serde(default)]
    pub text: Vec<TextRun>,
}

/// A run of a page's text (a word, a line) and the box it lies in on the
/// page, as fractions of the page's width and height from its top left.
/// bibli puts a space between two runs on one line, a line break between
/// lines.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// How an inspection is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionKind {
    /// Paragraphs of text.
    #[default]
    Text,
    /// Lines of code, in a monospaced font.
    Code,
    /// Rows of an offset, bytes and their characters.
    Hex,
    /// Rows and columns.
    Table,
}

/// What a preview shows of a file: text, or rows under columns, a page at
/// a time.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inspection {
    #[serde(default)]
    pub kind: InspectionKind,
    pub title: String,
    /// Paragraphs, or lines of code.
    #[serde(default)]
    pub text: Vec<String>,
    #[serde(default)]
    pub columns: Vec<String>,
    #[serde(default)]
    pub rows: Vec<Row>,
    /// Where the page before starts, if any.
    #[serde(default)]
    pub previous: Option<u64>,
    /// Where the page after starts, if any.
    #[serde(default)]
    pub next: Option<u64>,
    /// Where what was asked (found) is.
    #[serde(default)]
    pub position: u64,
    /// A line over the page: what a search found, what was left out.
    #[serde(default)]
    pub notice: String,
    /// Facts the format knows, for Details.
    #[serde(default)]
    pub details: Vec<Detail>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub cells: Vec<String>,
    /// Its size, for a member of a container.
    #[serde(default)]
    pub bytes: Option<u64>,
    #[serde(default)]
    pub depth: usize,
    #[serde(default)]
    pub branch: bool,
    /// What `target` asks for this row.
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detail {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommandParams {
    /// One of the manifest's command names.
    pub command: String,
    /// Its arguments, as its schema says.
    #[serde(default)]
    pub args: Value,
    /// The files it acts on: those its caller named, else a pane's
    /// selection.
    #[serde(default)]
    pub entries: Vec<File>,
}

/// What a command answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum CommandAnswer {
    Done,
    /// A value for its caller (a script, an agent).
    Value {
        value: Value,
    },
    /// A text for the clipboard, and what to say once it is there.
    Text {
        text: String,
        said: String,
    },
    /// A plan its caller applies or proposes: needs `write`.
    Plan {
        title: String,
        #[serde(default)]
        note: Option<String>,
        steps: Vec<Step>,
    },
}

/// A step of a plan, on the files of the call by handle, or at a path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Step {
    /// Give a file another name, in its folder.
    Rename { file: u32, name: String },
    /// Put a file in the trash.
    Trash { file: u32 },
    /// Move a file into the folder `to`, an absolute path.
    Move { file: u32, to: String },
    /// Copy a file into the folder `to`, an absolute path.
    Copy { file: u32, to: String },
    /// Make the folder `path`, an absolute path.
    Mkdir { path: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadParams {
    /// A handle of a file of the call being answered.
    pub file: u32,
    pub offset: u64,
    /// At most [`READ_LIMIT`].
    pub length: u32,
}

/// Bytes read, in base64: fewer than asked at the file's end.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadAnswer {
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogParams {
    /// `error`, `warn`, `info` or `debug`.
    pub level: String,
    pub message: String,
}

/// A JSON-RPC 2.0 message, one line: a request (`id` and `method`), its
/// response (`id` and `result` or `error`), or a notification (`method`
/// alone).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl Message {
    pub fn request(id: u64, method: &str, params: Value) -> Message {
        Message {
            jsonrpc: "2.0".into(),
            id: Some(id),
            method: Some(method.into()),
            params: Some(params),
            ..Message::default()
        }
    }

    pub fn notification(method: &str, params: Value) -> Message {
        Message {
            jsonrpc: "2.0".into(),
            method: Some(method.into()),
            params: Some(params),
            ..Message::default()
        }
    }

    pub fn answer(id: u64, answer: Result<Value, String>) -> Message {
        let (result, error) = match answer {
            Ok(value) => (Some(value), None),
            Err(message) => (
                None,
                Some(RpcError {
                    code: -32000,
                    message,
                }),
            ),
        };
        Message {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result,
            error,
            ..Message::default()
        }
    }

    /// A response to a request (`id`, no `method`).
    pub fn is_response(&self) -> bool {
        self.id.is_some() && self.method.is_none()
    }

    /// What a response says: its result, or its error's message.
    ///
    /// # Errors
    /// The error the response carries.
    pub fn outcome(self) -> Result<Value, String> {
        match (self.result, self.error) {
            (_, Some(error)) => Err(error.message),
            (Some(result), None) => Ok(result),
            (None, None) => Ok(Value::Null),
        }
    }

    /// The message as one line, its newline included.
    pub fn line(&self) -> Vec<u8> {
        let mut line = serde_json::to_vec(self).unwrap_or_default();
        line.push(b'\n');
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn messages_are_json_rpc_lines() {
        let request = Message::request(7, method::FIELD, json!({ "key": "a.b" }));
        let line = request.line();
        assert_eq!(line.last(), Some(&b'\n'));
        let back: Message = serde_json::from_slice(&line).unwrap();
        assert_eq!(back, request);
        assert!(!back.is_response());
        let answer = Message::answer(7, Err("no such file".into()));
        assert!(answer.is_response());
        assert_eq!(answer.outcome(), Err("no such file".into()));
        let answer: Message =
            serde_json::from_value(json!({ "jsonrpc": "2.0", "id": 7, "result": { "value": 3 } }))
                .unwrap();
        assert_eq!(answer.outcome(), Ok(json!({ "value": 3 })));
    }

    #[test]
    fn a_command_answers_a_value_a_text_or_a_plan_of_steps() {
        let plan: CommandAnswer = serde_json::from_value(json!({
            "answer": "plan",
            "title": "Rename",
            "steps": [
                { "op": "rename", "file": 1, "name": "b.txt" },
                { "op": "move", "file": 2, "to": "/x" },
            ],
        }))
        .unwrap();
        let CommandAnswer::Plan { steps, .. } = plan else {
            panic!("a plan")
        };
        assert_eq!(
            steps[0],
            Step::Rename {
                file: 1,
                name: "b.txt".into()
            }
        );
        assert_eq!(
            serde_json::to_value(CommandAnswer::Done).unwrap(),
            json!({ "answer": "done" })
        );
    }
}
