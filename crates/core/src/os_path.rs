//! Paths and file names that cross the command surface without loss.
//!
//! JSON strings are Unicode, file names are bytes. A path that is valid
//! UTF-8 serializes as a plain string, which is what every caller writes by
//! hand; any other path serializes as `{"bytes": [..]}` so it round-trips
//! exactly instead of turning into U+FFFD or failing `serde_json::to_value`
//! (which `json!` unwraps). Both forms deserialize.
//!
//! Use [`PathArg`] as a command argument type, or `#[serde(with =
//! "skwir_core::os_path")]` on a `PathBuf` field, and never pass
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
fn decode_bytes(bytes: Vec<u8>) -> Result<OsString, String> {
    use std::os::unix::ffi::OsStringExt;
    Ok(OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn decode_bytes(bytes: Vec<u8>) -> Result<OsString, String> {
    #[cfg(windows)]
    if let Some(units) = wtf8_units(&bytes) {
        return Ok(std::os::windows::ffi::OsStringExt::from_wide(&units));
    }
    String::from_utf8(bytes)
        .map(OsString::from)
        .map_err(|e| e.to_string())
}

/// A name or a path from bytes: those `OsStr::as_encoded_bytes` gave, or
/// those another program wrote (a `.trashinfo`, mountinfo, an index).
/// Exact on Unix, where a name is bytes, and on Windows, where it is
/// WTF-8 (UTF-8 that may hold the lone surrogates a Windows name can);
/// what is neither is read with what is not UTF-8 replaced.
pub fn from_bytes(bytes: Vec<u8>) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(bytes)
    }
    #[cfg(not(unix))]
    {
        #[cfg(windows)]
        if let Some(units) = wtf8_units(&bytes) {
            return std::os::windows::ffi::OsStringExt::from_wide(&units);
        }
        match String::from_utf8(bytes) {
            Ok(text) => OsString::from(text),
            Err(e) => OsString::from(String::from_utf8_lossy(e.as_bytes()).into_owned()),
        }
    }
}

/// [`from_bytes`] without a copy where it can: borrowed on Unix, and
/// elsewhere when the bytes are UTF-8.
pub fn from_bytes_ref(bytes: &[u8]) -> Cow<'_, OsStr> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Cow::Borrowed(OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    match std::str::from_utf8(bytes) {
        Ok(text) => Cow::Borrowed(OsStr::new(text)),
        Err(_) => Cow::Owned(from_bytes(bytes.to_vec())),
    }
}

/// The UTF-16 units WTF-8 `bytes` stand for: UTF-8 in which a lone
/// surrogate (`U+D800` to `U+DFFF`, unpaired) may stand too, as Windows
/// names hold them. `None` for bytes that are not WTF-8: not UTF-8 so
/// extended, overlong, or a pair of surrogates written apart. Tested
/// everywhere.
#[cfg(any(windows, test))]
fn wtf8_units(bytes: &[u8]) -> Option<Vec<u16>> {
    let continued = |b: Option<&u8>| {
        b.filter(|b| (0x80..0xC0).contains(*b))
            .map(|b| u32::from(b & 0x3F))
    };
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&lead) = bytes.get(i) {
        let (point, len) = match lead {
            0x00..0x80 => (u32::from(lead), 1),
            0xC2..0xE0 => (
                (u32::from(lead & 0x1F) << 6) | continued(bytes.get(i + 1))?,
                2,
            ),
            0xE0..0xF0 => {
                let point = (u32::from(lead & 0x0F) << 12)
                    | (continued(bytes.get(i + 1))? << 6)
                    | continued(bytes.get(i + 2))?;
                (point >= 0x800).then_some((point, 3))?
            }
            0xF0..0xF5 => {
                let point = (u32::from(lead & 0x07) << 18)
                    | (continued(bytes.get(i + 1))? << 12)
                    | (continued(bytes.get(i + 2))? << 6)
                    | continued(bytes.get(i + 3))?;
                ((0x1_0000..0x11_0000).contains(&point)).then_some((point, 4))?
            }
            _ => return None,
        };
        // A high surrogate then a low one is a character, written in four
        // bytes: written apart, they are not WTF-8. (A high surrogate last
        // came alone: a character's pair ends with its low one.)
        if (0xDC00..0xE000).contains(&point)
            && units.last().is_some_and(|u| (0xD800..0xDC00).contains(u))
        {
            return None;
        }
        if let Ok(unit) = u16::try_from(point) {
            units.push(unit);
        } else {
            let point = point - 0x1_0000;
            units.push(0xD800 | u16::try_from(point >> 10).ok()?);
            units.push(0xDC00 | u16::try_from(point & 0x3FF).ok()?);
        }
        i += len;
    }
    Some(units)
}

/// Serialize any `AsRef<OsStr>` (a `Path`, `PathBuf`, `OsString`) losslessly.
pub fn serialize<P: AsRef<OsStr>, S: Serializer>(path: &P, s: S) -> Result<S::Ok, S::Error> {
    to_repr(path.as_ref()).serialize(s)
}

/// Deserialize either a string or `{"bytes": [..]}` into a `PathBuf` or `OsString`.
pub fn deserialize<'de, P: From<OsString>, D: Deserializer<'de>>(d: D) -> Result<P, D::Error> {
    let os = match Repr::deserialize(d)? {
        Repr::Text(text) => OsString::from(text.into_owned()),
        Repr::Bytes { bytes } => decode_bytes(bytes.into_owned()).map_err(D::Error::custom)?,
    };
    Ok(P::from(os))
}

/// `#[serde(with = "crate::os_path::bytes")]` on a `Vec<u8>` that is
/// mostly text (what a trash says of an item): a string when it is UTF-8,
/// `{"bytes": [..]}` otherwise, as a path is written.
pub mod bytes {
    use super::{Cow, Deserialize, Deserializer, Repr, Serialize, Serializer};

    /// # Errors
    /// The serializer's.
    #[allow(clippy::ptr_arg)]
    pub fn serialize<S: Serializer>(bytes: &Vec<u8>, s: S) -> Result<S::Ok, S::Error> {
        match std::str::from_utf8(bytes) {
            Ok(text) => Repr::Text(Cow::Borrowed(text)),
            Err(_) => Repr::Bytes {
                bytes: Cow::Borrowed(bytes),
            },
        }
        .serialize(s)
    }

    /// # Errors
    /// The deserializer's.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        Ok(match Repr::deserialize(d)? {
            Repr::Text(text) => text.into_owned().into_bytes(),
            Repr::Bytes { bytes } => bytes.into_owned(),
        })
    }
}

/// [`bytes`] for an `Option<Vec<u8>>`.
pub mod bytes_opt {
    use super::{Deserialize, Deserializer, Serializer};

    /// # Errors
    /// The serializer's.
    #[allow(clippy::ref_option)]
    pub fn serialize<S: Serializer>(bytes: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            Some(bytes) => super::bytes::serialize(bytes, s),
            None => s.serialize_none(),
        }
    }

    /// # Errors
    /// The deserializer's.
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        #[derive(Deserialize)]
        struct Wrapped(#[serde(with = "super::bytes")] Vec<u8>);
        Ok(Option::<Wrapped>::deserialize(d)?.map(|Wrapped(bytes)| bytes))
    }
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

/// `path`, refused unless absolute: a path a command is given may come
/// from another process (`skwir cmd`, an agent, ADR-043), whose working
/// folder is not the window's.
///
/// # Errors
/// When `path` is relative.
pub fn absolute(path: &Path) -> anyhow::Result<&Path> {
    anyhow::ensure!(
        path.is_absolute(),
        "{} is not an absolute path",
        path.display()
    );
    Ok(path)
}

/// The user's home folder: `$HOME`, else the system's account record.
/// The one reading of it, for `~`, Home in the sidebar and the trail, the
/// search's root, the terminal's start.
pub fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

/// `std::fs::canonicalize`, the one way a path is made absolute and freed
/// of its links: on Windows without the `\\?\` the system puts before
/// it where the path reads the same without (`C:\…`, `\\server\share\…`),
/// so it compares, joins and shows as every other path does.
///
/// # Errors
/// As `std::fs::canonicalize`: nothing there, or a link that leads
/// nowhere.
pub fn canonical(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path).map(plain)
}

/// `path` without the `\\?\` Windows puts before a path it resolved
/// (`canonical`, a handle's final path), where it reads the same
/// without; `path` itself elsewhere.
pub fn plain(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    if let Some(plain) = path.to_str().and_then(without_verbatim) {
        return PathBuf::from(plain);
    }
    path
}

/// A Windows path written `\\?\C:\…` or `\\?\UNC\server\…` as it
/// reads without the prefix, when it reads the same: shorter than
/// `MAX_PATH`, and no name the prefix let through that Windows would
/// read otherwise (`CON`, a trailing dot). Tested everywhere.
#[cfg(any(windows, test))]
fn without_verbatim(path: &str) -> Option<String> {
    let (plain, names) = if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        (format!(r"\\{rest}"), rest)
    } else {
        let rest = path.strip_prefix(r"\\?\")?;
        let drive = rest.as_bytes();
        if !(drive.len() >= 3 && drive[0].is_ascii_alphabetic() && drive[1..3] == *b":\\") {
            return None;
        }
        (rest.to_owned(), &rest[3..])
    };
    let reads_the_same = plain.len() < 260
        && names
            .split('\\')
            .filter(|name| !name.is_empty())
            .all(|name| crate::op::windows_name_problem(name).is_none());
    reads_the_same.then_some(plain)
}

/// The executable `program` would run as: a path containing a separator
/// is taken as is, anything else is searched on `PATH`, then where the
/// system's package managers put programs a desktop session's `PATH` may
/// lack (`also_searched`). On Unix only regular files with an execute bit
/// count; on Windows a file counts by its extension, `PATHEXT`'s
/// (`zoxide` runs as `zoxide.exe`).
pub fn find_program(program: impl AsRef<OsStr>) -> Option<PathBuf> {
    let program = Path::new(program.as_ref());
    if program.as_os_str().is_empty() {
        return None;
    }
    if program.components().count() > 1 || program.is_absolute() {
        return names_of(program).into_iter().find(|p| is_executable(p));
    }
    let paths = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&paths)
        .chain(also_searched())
        .find_map(|dir| {
            names_of(&dir.join(program))
                .into_iter()
                .find(|candidate| is_executable(candidate))
        })
}

/// Where programs are that a desktop session's `PATH` may lack, searched
/// after it: on macOS an application started from the Finder or the Dock
/// has the system's folders alone, so Homebrew's (`/opt/homebrew/bin` on
/// Apple silicon, `/usr/local/bin` on Intel) and `MacPorts`' (`/opt/local/bin`)
/// are searched too. None elsewhere: a Linux session has them on its
/// `PATH`, and on Windows winget and Scoop put theirs on the user's.
fn also_searched() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"]
            .into_iter()
            .map(PathBuf::from)
            .collect()
    } else {
        Vec::new()
    }
}

/// The files `program` may be: itself on Unix; on Windows itself when its
/// extension is an executable's, then with each of `PATHEXT`'s.
fn names_of(program: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let pathext = std::env::var("PATHEXT").unwrap_or_default();
        windows_names(program, &pathext)
    }
    #[cfg(not(windows))]
    {
        vec![program.to_path_buf()]
    }
}

/// [`names_of`] on Windows, with `pathext` as `PATHEXT` says it
/// (`.COM;.EXE;.BAT;.CMD` when it says nothing). Tested everywhere.
#[cfg(any(windows, test))]
fn windows_names(program: &Path, pathext: &str) -> Vec<PathBuf> {
    let pathext = if pathext.trim().is_empty() {
        ".COM;.EXE;.BAT;.CMD"
    } else {
        pathext
    };
    let extensions: Vec<&str> = pathext
        .split(';')
        .map(str::trim)
        .filter(|e| e.starts_with('.') && e.len() > 1)
        .collect();
    let mut names = Vec::new();
    let own = program
        .extension()
        .and_then(OsStr::to_str)
        .map(|e| format!(".{e}"));
    if own.is_some_and(|own| extensions.iter().any(|e| e.eq_ignore_ascii_case(&own))) {
        names.push(program.to_path_buf());
    }
    for extension in extensions {
        let mut name = program.as_os_str().to_owned();
        name.push(extension.to_ascii_lowercase());
        names.push(PathBuf::from(name));
    }
    names
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
        assert!(find_program("definitely-not-a-skwir-program").is_none());
    }

    #[test]
    fn windows_runs_a_program_by_its_extensions() {
        let names = |program: &str, pathext: &str| -> Vec<String> {
            windows_names(Path::new(program), pathext)
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            names("zoxide", ""),
            ["zoxide.com", "zoxide.exe", "zoxide.bat", "zoxide.cmd"]
        );
        assert_eq!(
            names("tool.EXE", ".COM;.EXE"),
            ["tool.EXE", "tool.EXE.com", "tool.EXE.exe"],
            "an executable's name is itself first"
        );
        assert_eq!(
            names("python3.11", ".EXE;;bad;.PY "),
            ["python3.11.exe", "python3.11.py"],
            "a dot that is no executable's extension is part of the name"
        );
    }

    #[test]
    fn a_windows_path_loses_its_prefix_where_it_reads_the_same() {
        assert_eq!(
            without_verbatim(r"\\?\C:\Users\it's me\notes.txt").as_deref(),
            Some(r"C:\Users\it's me\notes.txt")
        );
        assert_eq!(without_verbatim(r"\\?\D:\").as_deref(), Some(r"D:\"));
        assert_eq!(
            without_verbatim(r"\\?\UNC\server\share\a b").as_deref(),
            Some(r"\\server\share\a b")
        );
        assert_eq!(without_verbatim(r"C:\already"), None, "no prefix");
        assert_eq!(without_verbatim(r"\\?\Volume{0-1}\x"), None, "no drive");
        assert_eq!(without_verbatim(r"\\?\C:\a\CON"), None, "a device");
        assert_eq!(without_verbatim(r"\\?\C:\a\dot."), None, "a trailing dot");
        let long = format!(r"\\?\C:\{}", "x".repeat(300));
        assert_eq!(without_verbatim(&long), None, "past MAX_PATH");
    }

    #[test]
    fn windows_names_are_read_from_wtf8_exactly() {
        let units = |bytes: &[u8]| wtf8_units(bytes);
        assert_eq!(units(b"a b"), Some(vec![0x61, 0x20, 0x62]));
        assert_eq!(units("été".as_bytes()), Some(vec![0xE9, 0x74, 0xE9]));
        assert_eq!(units("😀".as_bytes()), Some(vec![0xD83D, 0xDE00]));
        // A lone surrogate, as a Windows name may hold one.
        assert_eq!(units(b"x\xED\xA0\x80y"), Some(vec![0x78, 0xD800, 0x79]));
        assert_eq!(units(b"\xED\xB0\x80"), Some(vec![0xDC00]));
        // Paired surrogates written apart, overlong, truncated, past U+10FFFF.
        assert_eq!(units(b"\xED\xA0\x80\xED\xB0\x80"), None);
        assert_eq!(units(b"\xC0\x80"), None);
        assert_eq!(units(b"\xE0\x80\x80"), None);
        assert_eq!(units(b"\xE2\x82"), None);
        assert_eq!(units(b"\xF4\x90\x80\x80"), None);
        assert_eq!(units(b"\xFF"), None);
    }
}
