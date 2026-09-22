//! Paths and file names that cross the command surface without loss.
//!
//! JSON strings are Unicode, file names are bytes. A path that is valid
//! UTF-8 serializes as a plain string, which is what every caller writes by
//! hand; any other path serializes as `{"bytes": [..]}` so it round-trips
//! exactly instead of turning into U+FFFD or failing `serde_json::to_value`
//! (which `json!` unwraps). Both forms deserialize.
//!
//! Use [`PathArg`] as a command argument type, or `#[serde(with =
//! "bibli_core::os_path")]` on a `PathBuf` field, and never pass
//! `to_string_lossy()` where a path is expected.
//!
//! [`find_program`] is the one `PATH` lookup: plugins, platform adapters and
//! requirement probes all use it, so "installed" means the same everywhere.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Repr<'a> {
    Text(Cow<'a, str>),
    Bytes { bytes: Cow<'a, [u8]> },
}

fn to_repr(s: &OsStr) -> Repr<'_> {
    match s.to_str() {
        Some(text) => Repr::Text(Cow::Borrowed(text)),
        None => Repr::Bytes {
            bytes: Cow::Borrowed(os_bytes(s)),
        },
    }
}

#[cfg(unix)]
fn os_bytes(s: &OsStr) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes()
}

#[cfg(not(unix))]
fn os_bytes(s: &OsStr) -> &[u8] {
    s.as_encoded_bytes()
}

#[cfg(unix)]
// Infallible here; the non-Unix twin can fail and callers see one signature.
#[allow(clippy::unnecessary_wraps)]
fn from_bytes(bytes: Vec<u8>) -> Result<OsString, String> {
    use std::os::unix::ffi::OsStringExt;
    Ok(OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn from_bytes(bytes: Vec<u8>) -> Result<OsString, String> {
    String::from_utf8(bytes)
        .map(OsString::from)
        .map_err(|e| e.to_string())
}

/// Serialize any `AsRef<OsStr>` (a `Path`, `PathBuf`, `OsString`) losslessly.
pub fn serialize<P: AsRef<OsStr>, S: Serializer>(path: &P, s: S) -> Result<S::Ok, S::Error> {
    to_repr(path.as_ref()).serialize(s)
}

/// Deserialize either a string or `{"bytes": [..]}` into a `PathBuf` or `OsString`.
pub fn deserialize<'de, P: From<OsString>, D: Deserializer<'de>>(d: D) -> Result<P, D::Error> {
    let os = match Repr::deserialize(d)? {
        Repr::Text(text) => OsString::from(text.into_owned()),
        Repr::Bytes { bytes } => from_bytes(bytes.into_owned()).map_err(D::Error::custom)?,
    };
    Ok(P::from(os))
}

/// A path as a command argument or result: lossless in JSON, see the module docs.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PathArg(pub PathBuf);

impl PathArg {
    pub fn into_inner(self) -> PathBuf {
        self.0
    }
}

impl std::ops::Deref for PathArg {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for PathArg {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl From<PathBuf> for PathArg {
    fn from(p: PathBuf) -> Self {
        Self(p)
    }
}

impl From<&Path> for PathArg {
    fn from(p: &Path) -> Self {
        Self(p.to_path_buf())
    }
}

impl From<&str> for PathArg {
    fn from(p: &str) -> Self {
        Self(PathBuf::from(p))
    }
}

impl From<PathArg> for PathBuf {
    fn from(p: PathArg) -> Self {
        p.0
    }
}

impl Serialize for PathArg {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        serialize(&self.0, s)
    }
}

impl<'de> Deserialize<'de> for PathArg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        deserialize::<PathBuf, D>(d).map(Self)
    }
}

impl schemars::JsonSchema for PathArg {
    fn schema_name() -> Cow<'static, str> {
        "Path".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A filesystem path. A path that is not valid UTF-8 is written as {\"bytes\": [..]}.",
            "anyOf": [
                { "type": "string" },
                {
                    "type": "object",
                    "properties": { "bytes": { "type": "array", "items": { "type": "integer", "minimum": 0, "maximum": 255 } } },
                    "required": ["bytes"]
                }
            ]
        })
    }
}

/// The executable `program` would run as: a path containing `/` is taken as
/// is, anything else is searched on `PATH`. Only regular files with an
/// execute bit count.
pub fn find_program(program: impl AsRef<OsStr>) -> Option<PathBuf> {
    let program = Path::new(program.as_ref());
    if program.as_os_str().is_empty() {
        return None;
    }
    if program.components().count() > 1 || program.is_absolute() {
        return is_executable(program).then(|| program.to_path_buf());
    }
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_paths_are_plain_strings() {
        let arg = PathArg::from("/a/b c");
        assert_eq!(
            serde_json::to_value(&arg).unwrap(),
            serde_json::json!("/a/b c")
        );
        let back: PathArg = serde_json::from_value(serde_json::json!("/a/b c")).unwrap();
        assert_eq!(back, arg);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_round_trip() {
        use std::os::unix::ffi::OsStrExt;
        let raw = Path::new(OsStr::from_bytes(b"/tmp/caf\xe9"));
        let value = serde_json::to_value(PathArg::from(raw)).unwrap();
        assert!(value.get("bytes").is_some());
        let back: PathArg = serde_json::from_value(value).unwrap();
        assert_eq!(&*back, raw);
    }

    #[test]
    fn find_program_needs_an_executable_file() {
        assert!(find_program("").is_none());
        assert!(find_program("/").is_none());
        assert!(find_program("definitely-not-a-bibli-program").is_none());
    }
}
