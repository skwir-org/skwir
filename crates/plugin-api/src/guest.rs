//! A plugin's side, for one written in Rust: implement [`Plugin`], then
//! serve it as a program ([`stdio::serve`], from `main`) or export it as a
//! WebAssembly module ([`crate::export!`]). Both answer through
//! [`answer`], so one plugin can be built either way.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::protocol::{
    CommandAnswer, CommandParams, ConvertParams, FieldParams, InspectParams, Inspection,
    ThumbnailParams, method,
};

/// What bibli does for a plugin while it answers a call.
pub trait Host {
    /// Bytes of a file of the call, by its handle, from `offset`, at most
    /// `length` (fewer at its end).
    ///
    /// # Errors
    /// Refused (no such handle, may not read), or the file could not be
    /// read.
    fn read(&self, file: u32, offset: u64, length: u32) -> Result<Vec<u8>, String>;
    /// A line for bibli's log.
    fn log(&self, level: &str, message: &str);
}

/// What a plugin answers. Each method has a default that has nothing to
/// say, so a plugin implements those its manifest declares.
pub trait Plugin {
    /// A field of a file: `None` when it says nothing.
    ///
    /// # Errors
    /// Why it could not tell.
    fn field(&self, _host: &dyn Host, _params: FieldParams) -> Result<Option<Value>, String> {
        Ok(None)
    }

    /// A file's thumbnail, as a PNG: `None` when it has none.
    ///
    /// # Errors
    /// Why it could not draw it.
    fn thumbnail(
        &self,
        _host: &dyn Host,
        _params: ThumbnailParams,
    ) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    /// A page of a file's preview: `None` when it has none.
    ///
    /// # Errors
    /// Why it could not show it.
    fn inspect(
        &self,
        _host: &dyn Host,
        _params: InspectParams,
    ) -> Result<Option<Inspection>, String> {
        Ok(None)
    }

    /// The file a preview shows in this one's place (protocol version 2,
    /// a manifest's `converts`): its name, whose extension says its type,
    /// and its bytes. `None` when it shows nothing for this one.
    ///
    /// # Errors
    /// Why it could not make it.
    fn convert(
        &self,
        _host: &dyn Host,
        _params: ConvertParams,
    ) -> Result<Option<(String, Vec<u8>)>, String> {
        Ok(None)
    }

    /// Run one of its commands.
    ///
    /// # Errors
    /// Why it did not.
    fn command(&self, _host: &dyn Host, params: CommandParams) -> Result<CommandAnswer, String> {
        Err(format!("no command `{}`", params.command))
    }
}

/// Answer `method` with `params` (JSON), as `plugin` does: the answer as
/// JSON.
///
/// # Errors
/// An unknown method, parameters that do not parse, or the plugin's own.
pub fn answer(
    plugin: &dyn Plugin,
    host: &dyn Host,
    method: &str,
    params: &str,
) -> Result<String, String> {
    fn parsed<T: DeserializeOwned>(params: &str) -> Result<T, String> {
        serde_json::from_str(params).map_err(|e| format!("parameters: {e}"))
    }
    fn json(value: impl Serialize) -> Result<String, String> {
        serde_json::to_string(&value).map_err(|e| e.to_string())
    }
    match method {
        method::FIELD => {
            let value = plugin.field(host, parsed(params)?)?;
            json(crate::protocol::FieldAnswer { value })
        }
        method::THUMBNAIL => {
            let png = plugin.thumbnail(host, parsed(params)?)?;
            json(crate::protocol::ThumbnailAnswer {
                png: png.map(|png| crate::base64::encode(&png)),
            })
        }
        method::INSPECT => json(plugin.inspect(host, parsed(params)?)?),
        method::CONVERT => {
            let made = plugin.convert(host, parsed(params)?)?;
            json(made.map(|(name, bytes)| crate::protocol::ConvertAnswer {
                name,
                data: crate::base64::encode(&bytes),
            }))
        }
        method::COMMAND => json(plugin.command(host, parsed(params)?)?),
        other => Err(format!("unknown method `{other}`")),
    }
}

/// A plugin that runs as a program: the protocol over standard input and
/// output, one call at a time (bibli sends the next once one is
/// answered).
pub mod stdio {
    use std::cell::{Cell, RefCell};
    use std::io::{BufRead, Write};

    use serde_json::{Value, json};

    use super::{Host, Plugin};
    use crate::protocol::{
        InitializeAnswer, LogParams, MESSAGE_LIMIT, Message, ReadAnswer, ReadParams, VERSION,
        method,
    };

    /// Serve `plugin` until bibli closes standard input.
    ///
    /// # Errors
    /// Standard input or output failing.
    pub fn serve(plugin: &dyn Plugin) -> std::io::Result<()> {
        let stdin = std::io::stdin();
        let host = Stdio {
            input: RefCell::new(stdin.lock()),
            next: Cell::new(1),
            cancelled: Cell::new(None),
        };
        while let Some(message) = host.next_message()? {
            let Some(id) = message.id else {
                continue; // A notification between calls: a late cancel.
            };
            let name = message.method.unwrap_or_default();
            let params = message.params.unwrap_or(Value::Null);
            host.cancelled.set(None);
            let answer = if name == method::INITIALIZE {
                Ok(json!(InitializeAnswer { api: VERSION }))
            } else {
                super::answer(plugin, &host, &name, &params.to_string())
                    .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
            };
            write(&Message::answer(id, answer))?;
        }
        Ok(())
    }

    fn write(message: &Message) -> std::io::Result<()> {
        let mut out = std::io::stdout().lock();
        out.write_all(&message.line())?;
        out.flush()
    }

    struct Stdio<R> {
        input: RefCell<R>,
        next: Cell<u64>,
        /// A call bibli stopped waiting for: reads fail from then on.
        cancelled: Cell<Option<u64>>,
    }

    impl<R: BufRead> Stdio<R> {
        fn next_message(&self) -> std::io::Result<Option<Message>> {
            let mut line = Vec::new();
            let mut input = self.input.borrow_mut();
            let input: &mut R = &mut input;
            let read =
                std::io::Read::take(input, MESSAGE_LIMIT as u64).read_until(b'\n', &mut line)?;
            if read == 0 {
                return Ok(None);
            }
            serde_json::from_slice(&line)
                .map(Some)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        }
    }

    impl<R: BufRead> Host for Stdio<R> {
        fn read(&self, file: u32, offset: u64, length: u32) -> Result<Vec<u8>, String> {
            if self.cancelled.get().is_some() {
                return Err("cancelled".into());
            }
            let id = self.next.get();
            self.next.set(id + 1);
            let params = ReadParams {
                file,
                offset,
                length,
            };
            write(&Message::request(id, method::READ, json!(params))).map_err(|e| e.to_string())?;
            loop {
                let message = self
                    .next_message()
                    .map_err(|e| e.to_string())?
                    .ok_or("bibli closed the channel")?;
                if message.is_response() && message.id == Some(id) {
                    let answer: ReadAnswer =
                        serde_json::from_value(message.outcome()?).map_err(|e| e.to_string())?;
                    return crate::base64::decode(&answer.data)
                        .ok_or_else(|| "bytes that are no base64".to_owned());
                }
                if message.method.as_deref() == Some(method::CANCEL) {
                    self.cancelled.set(Some(0));
                    return Err("cancelled".into());
                }
            }
        }

        fn log(&self, level: &str, message: &str) {
            let params = LogParams {
                level: level.into(),
                message: message.into(),
            };
            let _ = write(&Message::notification(method::LOG, json!(params)));
        }
    }
}

/// A plugin that runs as a WebAssembly module: bindings to bibli's host.
#[cfg(target_arch = "wasm32")]
pub mod wasm {
    #[allow(clippy::all, clippy::pedantic, unsafe_code)]
    pub mod bindings {
        wit_bindgen::generate!({
            path: "wit",
            world: "plugin",
            pub_export_macro: true,
            export_macro_name: "export_guest",
            default_bindings_module: "bibli_plugin_api::guest::wasm::bindings",
        });
    }

    pub use bindings::export_guest;
    pub use bindings::exports::bibli::plugin::guest::Guest;

    /// bibli, as a module's imports reach it.
    pub struct Imports;

    impl super::Host for Imports {
        fn read(&self, file: u32, offset: u64, length: u32) -> Result<Vec<u8>, String> {
            bindings::bibli::plugin::host::read(file, offset, length)
        }

        fn log(&self, level: &str, message: &str) {
            bindings::bibli::plugin::host::log(level, message);
        }
    }
}

/// Export `$plugin` (a type that is [`Plugin`] and `Default`) as the
/// module's answers. Does nothing where it is not built for WebAssembly,
/// so the same crate also builds as a program.
#[macro_export]
macro_rules! export {
    ($plugin:ty) => {
        #[cfg(target_arch = "wasm32")]
        const _: () = {
            struct Exported;
            impl $crate::guest::wasm::Guest for Exported {
                fn call(method: String, params: String) -> Result<String, String> {
                    let plugin = <$plugin as ::core::default::Default>::default();
                    $crate::guest::answer(&plugin, &$crate::guest::wasm::Imports, &method, &params)
                }
            }
            $crate::guest::wasm::export_guest!(Exported);
        };
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{FieldAnswer, File};
    use serde_json::json;

    struct Words;

    impl Plugin for Words {
        fn field(&self, host: &dyn Host, params: FieldParams) -> Result<Option<Value>, String> {
            let text = host.read(params.file.handle, 0, 1024)?;
            let words = String::from_utf8_lossy(&text).split_whitespace().count();
            Ok(Some(json!(words)))
        }
    }

    struct Bytes(&'static [u8]);

    impl Host for Bytes {
        fn read(&self, file: u32, offset: u64, length: u32) -> Result<Vec<u8>, String> {
            if file != 1 {
                return Err("no such file".into());
            }
            let start = usize::try_from(offset).unwrap().min(self.0.len());
            let end = (start + length as usize).min(self.0.len());
            Ok(self.0[start..end].to_vec())
        }
        fn log(&self, _: &str, _: &str) {}
    }

    #[test]
    fn a_method_is_answered_as_the_plugin_says() {
        let file = File {
            handle: 1,
            name: "a.txt".into(),
            path: "/a.txt".into(),
            mime: Some("text/plain".into()),
            size: Some(11),
        };
        let params = json!({ "key": "words.count", "file": file }).to_string();
        let said = answer(&Words, &Bytes(b"three short words"), method::FIELD, &params).unwrap();
        let said: FieldAnswer = serde_json::from_str(&said).unwrap();
        assert_eq!(said.value, Some(json!(3)));
        assert!(answer_of("nothing").is_err());
        let preview = answer(
            &Words,
            &Bytes(b""),
            method::INSPECT,
            &json!({ "file": file }).to_string(),
        );
        assert_eq!(preview.as_deref(), Ok("null"), "nothing to show: null");
        let converted = answer(
            &Words,
            &Bytes(b""),
            method::CONVERT,
            &json!({ "file": file }).to_string(),
        );
        assert_eq!(converted.as_deref(), Ok("null"), "no other file: null");
        let shout = answer(
            &Shout,
            &Bytes(b"quiet"),
            method::CONVERT,
            &json!({ "file": file }).to_string(),
        )
        .unwrap();
        let shout: crate::protocol::ConvertAnswer = serde_json::from_str(&shout).unwrap();
        assert_eq!(shout.name, "a.md");
        assert_eq!(
            crate::base64::decode(&shout.data).as_deref(),
            Some(&b"# QUIET"[..])
        );
    }

    /// Shows a text as a Markdown heading of it, louder.
    struct Shout;

    impl Plugin for Shout {
        fn convert(
            &self,
            host: &dyn Host,
            params: crate::protocol::ConvertParams,
        ) -> Result<Option<(String, Vec<u8>)>, String> {
            let text = host.read(params.file.handle, 0, 1024)?;
            let loud = String::from_utf8_lossy(&text).to_uppercase();
            Ok(Some(("a.md".into(), format!("# {loud}").into_bytes())))
        }
    }

    fn answer_of(method: &str) -> Result<String, String> {
        answer(&Words, &Bytes(b""), method, "{}")
    }
}
