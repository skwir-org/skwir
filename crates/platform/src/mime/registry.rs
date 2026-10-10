//! Windows' own types (ADR-037): what the registry's classes say of an
//! extension (`HKEY_CLASSES_ROOT\.png`: its `Content Type`, its
//! `PerceivedType`), read as Go's and Python's `mime` read it, of a type
//! the extension it is saved with (`MIME\Database\Content Type\…`), and
//! what Explorer calls it. It knows no hierarchy and reads no bytes:
//! those stay the database's.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use crate::windows::classes;

use super::{Kind, MEMO, Mime};

pub const ASKED: bool = true;

/// The extension each type the registry gave was asked by: a type its
/// MIME database does not list still finds its `PerceivedType` and name.
fn asked_by() -> &'static RwLock<HashMap<String, String>> {
    static ASKED_BY: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
    ASKED_BY.get_or_init(RwLock::default)
}

/// The type the registry gives files named `*.extension`: its
/// `Content Type`, when that is a type (`top/sub`), lowercase as the
/// database writes them.
pub fn by_extension(extension: &str) -> Option<Mime> {
    let mime = classes::value(&format!(".{extension}"), "Content Type")?.to_ascii_lowercase();
    let (top, sub) = mime.split_once('/')?;
    if top.is_empty() || sub.is_empty() || sub.contains('/') {
        return None;
    }
    if let Ok(mut asked) = asked_by().write() {
        if asked.len() >= MEMO {
            asked.clear();
        }
        asked
            .entry(mime.clone())
            .or_insert_with(|| extension.to_owned());
    }
    Some(Mime(mime))
}

/// The extensions of the type `m` the registry knows: the one its MIME
/// database saves it with, and the one it was asked by.
pub fn extensions(m: &Mime) -> Vec<String> {
    let listed = classes::value(
        &format!(r"MIME\Database\Content Type\{}", m.as_str()),
        "Extension",
    )
    .and_then(|e| Some(e.strip_prefix('.')?.to_lowercase()))
    .filter(|e| !e.is_empty());
    let asked = asked_by()
        .read()
        .ok()
        .and_then(|a| a.get(m.as_str()).cloned());
    let mut all: Vec<String> = listed.into_iter().collect();
    if let Some(asked) = asked.filter(|a| !all.contains(a)) {
        all.push(asked);
    }
    all
}

/// The type the registry gives the extension it saves `m` with.
pub fn canonical(m: &Mime) -> Option<Mime> {
    by_extension(extensions(m).first()?)
}

/// What Explorer calls files of type `m` ("Text Document"), in the
/// user's language, by an extension of the type's.
pub fn description(m: &Mime) -> Option<String> {
    let extension = extensions(m)
        .into_iter()
        .chain(super::database::extensions(m))
        .next()?;
    classes::friendly_name(&format!(".{extension}"))
}

/// The kinds of `m`: the database's; where it gives none, what the
/// registry perceives files of the type as. `PerceivedType` has no
/// spreadsheets, presentations or fonts, and calls Office files
/// documents: it does not group as skwir does, so it only fills in.
pub fn kinds(m: &Mime, database: u16) -> u16 {
    if database != 0 {
        return database;
    }
    let perceived = extensions(m)
        .iter()
        .find_map(|e| classes::value(&format!(".{e}"), "PerceivedType"));
    match perceived.map(|p| p.to_ascii_lowercase()).as_deref() {
        Some("image") => Kind::Image.bit(),
        Some("video") => Kind::Video.bit(),
        Some("audio") => Kind::Audio.bit(),
        Some("text") => Kind::Text.bit(),
        Some("compressed") => Kind::Archive.bit(),
        Some("document") => Kind::Document.bit(),
        Some("application") => Kind::Program.bit(),
        _ => 0,
    }
}
