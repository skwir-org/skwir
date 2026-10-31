//! Grammars the user added, to bat and to Sublime Text: the
//! `.sublime-syntax` files of bat's folder (`bat::syntaxes_dir`, as `bat
//! cache --build` finds them) and of Sublime Text's packages (its
//! `Packages` folders, and the zipped `.sublime-package` files of its
//! `Installed Packages`). They join bat's grammars as bat joins its own:
//! one named as one of bat's, or claiming its extension, takes its place,
//! and bat's folder comes last, over Sublime Text's.
//!
//! The set built with them is kept in skwir's cache (`keep_in`), named by
//! skwir's own build and the files it was made of (path, length, time
//! modified), so reading it again once the grammars were let go of
//! (`GRAMMARS_KEPT`) costs what reading bat's own does; it is built again
//! when one of them changes.
use parking_lot::RwLock;
use skwir_core::cache::CacheSlot;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;
use syntect::parsing::{SyntaxDefinition, SyntaxSet};

/// Far more than a grammar holds: a file this large is none.
const LIMIT: u64 = 4 << 20;
/// Grammars read at most, from every folder and package together.
const MOST: usize = 2000;
/// How deep a folder is looked into: a package's grammars lie a level or
/// two below it.
const DEPTH: usize = 6;
/// What a grammar's file is named.
const SUFFIX: &str = ".sublime-syntax";

/// Where a grammar was added.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Source {
    Bat,
    SublimeText,
}

impl Source {
    /// Its name, as the Reading page says it.
    pub fn label(self) -> &'static str {
        match self {
            Source::Bat => "bat",
            Source::SublimeText => "Sublime Text",
        }
    }
}

/// A grammar the user added, as it was last read.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Added {
    /// Its name, or its file's when it does not read.
    pub name: String,
    pub source: Source,
    /// Its file, or the package holding it.
    pub path: PathBuf,
    /// Its name in the package holding it.
    pub member: Option<String>,
    /// The extensions and names of the files it colors.
    pub file_types: Vec<String>,
    /// Why it was left out.
    pub problem: Option<String>,
}

/// A file or package found, in the order it joins.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Found {
    source: Source,
    path: PathBuf,
    /// A `.sublime-package`, read as a zip.
    package: bool,
    len: u64,
    modified: Option<SystemTime>,
}

/// Where the built set is kept, once the window says (`keep_in`).
static KEPT: OnceLock<CacheSlot> = OnceLock::new();
/// The grammars added, as the set last read or built found them.
static LISTED: RwLock<Option<Arc<[Added]>>> = RwLock::new(None);

/// Keep the set built with the user's grammars in `slot`, from now on.
/// Without it (a test, a tool), it is built each time it is read.
pub fn keep_in(slot: CacheSlot) {
    let _ = KEPT.set(slot);
}

/// The grammars added, as last read; `None` before they are. Reading them
/// again (`read`) is a worker's work.
pub fn listed() -> Option<Arc<[Added]>> {
    LISTED.read().clone()
}

/// Read the grammars added again, building the set they make if it is not
/// kept for these files, and let it go: at start and when the settings
/// open, so the next file colored finds it ready and the Reading page
/// what was found. It reads files: a worker's work.
pub fn read() -> Arc<[Added]> {
    list(read_in(&folders(), KEPT.get()))
}

/// bat's grammars with the user's: as kept for the files found, else built
/// and kept. bat's own when none was added.
pub(super) fn set() -> SyntaxSet {
    let (set, listed) = set_in(&folders(), KEPT.get());
    list(listed);
    set
}

fn read_in(folders: &[(Source, PathBuf)], slot: Option<&CacheSlot>) -> Vec<Added> {
    let found = found(folders);
    if found.is_empty() {
        return Vec::new();
    }
    if let Some(slot) = slot
        && let Some(listed) = kept_list(slot, &key(&found))
        && kept_paths(slot, &key(&found)).0.is_file()
    {
        return listed;
    }
    let (set, listed) = build(&found);
    if let Some(slot) = slot {
        keep(slot, &found, &set, &listed);
    }
    listed
}

fn set_in(folders: &[(Source, PathBuf)], slot: Option<&CacheSlot>) -> (SyntaxSet, Vec<Added>) {
    let found = found(folders);
    if found.is_empty() {
        return (two_face::syntax::extra_newlines(), Vec::new());
    }
    if let Some(slot) = slot {
        let key = key(&found);
        if let Some(listed) = kept_list(slot, &key)
            && let Ok(set) = syntect::dumps::from_uncompressed_dump_file(kept_paths(slot, &key).0)
        {
            return (set, listed);
        }
    }
    let (set, listed) = build(&found);
    if let Some(slot) = slot {
        keep(slot, &found, &set, &listed);
    }
    (set, listed)
}

fn list(listed: Vec<Added>) -> Arc<[Added]> {
    let listed: Arc<[Added]> = listed.into();
    *LISTED.write() = Some(Arc::clone(&listed));
    listed
}

/// The folders grammars are added in, in the order they join: Sublime
/// Text's (its third version's, then its own), then bat's.
fn folders() -> Vec<(Source, PathBuf)> {
    let mut folders: Vec<_> = sublime_text_folders()
        .into_iter()
        .map(|dir| (Source::SublimeText, dir))
        .collect();
    folders.extend(crate::bat::syntaxes_dir().map(|dir| (Source::Bat, dir)));
    folders
}

/// Sublime Text's own folders, each holding `Packages` and `Installed
/// Packages`: in the configuration folder on Linux and the BSDs, in
/// Application Support on macOS, in the roaming `AppData` on Windows.
fn sublime_text_folders() -> Vec<PathBuf> {
    #[cfg(any(target_os = "macos", windows))]
    let (base, names) = (dirs::config_dir(), ["Sublime Text 3", "Sublime Text"]);
    #[cfg(not(any(target_os = "macos", windows)))]
    let (base, names) = (
        skwir_core::xdg::config_home(),
        ["sublime-text-3", "sublime-text"],
    );
    base.map(|base| names.iter().map(|name| base.join(name)).collect())
        .unwrap_or_default()
}

/// The grammars' files and packages under `folders`, in order, each with
/// what tells another version of it. A folder that is not there is left
/// out.
fn found(folders: &[(Source, PathBuf)]) -> Vec<Found> {
    let mut found = Vec::new();
    for (source, dir) in folders {
        match source {
            Source::Bat => walk(*source, dir, 0, &mut found),
            Source::SublimeText => {
                walk(*source, &dir.join("Packages"), 0, &mut found);
                for path in sorted(&dir.join("Installed Packages")) {
                    if path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().ends_with(".sublime-package"))
                    {
                        push(*source, path, true, &mut found);
                    }
                }
            }
        }
    }
    found.truncate(MOST);
    found
}

/// The `.sublime-syntax` files under `dir`, a link followed as bat follows
/// it (a configuration linked from elsewhere), `DEPTH` levels at most.
fn walk(source: Source, dir: &Path, depth: usize, found: &mut Vec<Found>) {
    if depth > DEPTH || found.len() >= MOST {
        return;
    }
    for path in sorted(dir) {
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            walk(source, &path, depth + 1, found);
        } else if meta.is_file()
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(SUFFIX))
        {
            push(source, path, false, found);
        }
    }
}

fn push(source: Source, path: PathBuf, package: bool, found: &mut Vec<Found>) {
    let Ok(meta) = std::fs::metadata(&path) else {
        return;
    };
    found.push(Found {
        source,
        path,
        package,
        len: meta.len(),
        modified: meta.modified().ok(),
    });
}

/// What `dir` holds, in the order of its names; nothing when it cannot be
/// listed.
fn sorted(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    paths.sort();
    paths
}

/// The name the set built from `found` is kept by: skwir's own build (a
/// new one may read grammars another way) and every file it was made of.
fn key(found: &[Found]) -> String {
    let mut hash = md5::Context::new();
    if let Some(meta) = std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::metadata(exe).ok())
    {
        hash.consume(format!("{} {:?}\n", meta.len(), meta.modified().ok()));
    }
    for file in found {
        hash.consume(format!(
            "{:?} {} {} {:?} ",
            file.source, file.package, file.len, file.modified
        ));
        hash.consume(file.path.as_os_str().as_encoded_bytes());
        hash.consume(b"\n");
    }
    format!("{:x}", hash.finalize())
}

/// Where the set of `key` is kept, and what was found making it: in the
/// slot's own folder, which holds nothing else.
fn kept_paths(slot: &CacheSlot, key: &str) -> (PathBuf, PathBuf) {
    (
        slot.folder(&format!("{key}.syntaxes")),
        slot.folder(&format!("{key}.json")),
    )
}

fn kept_list(slot: &CacheSlot, key: &str) -> Option<Vec<Added>> {
    let bytes = crate::file::read_regular(&kept_paths(slot, key).1, LIMIT).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Keep `set` and what was found making it, under the files' key, in
/// place of what was kept for others.
fn keep(slot: &CacheSlot, found: &[Found], set: &SyntaxSet, listed: &[Added]) {
    let key = key(found);
    let (dump, list) = kept_paths(slot, &key);
    let Some(dir) = dump.parent() else {
        return;
    };
    let written = (|| -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = tempfile::Builder::new().prefix(".tmp-").tempfile_in(dir)?;
        syntect::dumps::dump_to_uncompressed_file(set, tmp.path())?;
        crate::file::take_place(tmp, &dump)?;
        let mut tmp = tempfile::Builder::new().prefix(".tmp-").tempfile_in(dir)?;
        std::io::Write::write_all(&mut tmp, &serde_json::to_vec(listed)?)?;
        crate::file::take_place(tmp, &list)?;
        Ok(())
    })();
    if let Err(error) = written {
        tracing::debug!("grammars not kept: {error:#}");
        return;
    }
    // Only the set of the files as they are now is kept.
    for path in sorted(dir) {
        if path != dump && path != list {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// bat's grammars with those of `found` joined in their order, and what
/// each was found to be. One that does not read is left out, said why.
fn build(found: &[Found]) -> (SyntaxSet, Vec<Added>) {
    let mut builder = two_face::syntax::extra_newlines().into_builder();
    let mut listed = Vec::new();
    for file in found {
        let read = if file.package {
            package(&file.path)
        } else {
            vec![(None, read_text(&file.path))]
        };
        for (member, text) in read {
            let stem = member
                .as_deref()
                .map_or(file.path.as_path(), Path::new)
                .file_name()
                .map(|name| {
                    let name = name.to_string_lossy();
                    name.strip_suffix(SUFFIX).unwrap_or(&name).to_owned()
                })
                .unwrap_or_default();
            let definition = text.and_then(|text| {
                SyntaxDefinition::load_from_str(&text, true, Some(&stem))
                    .map_err(|error| error.to_string())
            });
            let mut added = Added {
                name: stem,
                source: file.source,
                path: file.path.clone(),
                member,
                file_types: Vec::new(),
                problem: None,
            };
            match definition {
                Ok(definition) => {
                    added.name.clone_from(&definition.name);
                    added.file_types.clone_from(&definition.file_extensions);
                    builder.add(definition);
                }
                Err(problem) => added.problem = Some(problem),
            }
            listed.push(added);
        }
    }
    (builder.build(), listed)
}

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = crate::file::read_regular(path, LIMIT).map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_owned())
}

/// The grammars a zipped package holds, by their name in it; nothing when
/// it does not read as a zip.
fn package(path: &Path) -> Vec<(Option<String>, Result<String, String>)> {
    use std::io::Read as _;
    let Ok(file) = crate::file::open_regular(path) else {
        return Vec::new();
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return vec![(None, Err("not a zipped package".to_owned()))];
    };
    let mut grammars = Vec::new();
    for index in 0..zip.len().min(MOST * 8) {
        let Ok(member) = zip.by_index(index) else {
            continue;
        };
        let Ok(name) = member.name().map(std::borrow::Cow::into_owned) else {
            continue;
        };
        if !name.ends_with(SUFFIX) || !member.is_file() {
            continue;
        }
        let text = if member.size() > LIMIT {
            Err("larger than a grammar".to_owned())
        } else {
            let mut text = String::new();
            member
                .take(LIMIT)
                .read_to_string(&mut text)
                .map(|_| text)
                .map_err(|error| error.to_string())
        };
        grammars.push((Some(name), text));
        if grammars.len() >= MOST {
            break;
        }
    }
    grammars
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// A language no grammar of bat's knows.
    fn grammar(name: &str, extension: &str, scope: &str) -> String {
        format!(
            r#"%YAML 1.2
---
name: {name}
file_extensions: [{extension}]
scope: {scope}
contexts:
  main:
    - match: '#.*$'
      scope: comment.line.number-sign
    - match: '\b(let|in)\b'
      scope: keyword.control
    - match: '"[^"]*"'
      scope: string.quoted.double
"#
        )
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// The colors `set` gives a line in the grammar for `token`.
    fn colors(set: &SyntaxSet, token: &str, line: &str) -> std::collections::BTreeSet<[u8; 3]> {
        let syntax = set.find_syntax_by_token(token).unwrap();
        let theme =
            super::super::bat_themes().get(two_face::theme::EmbeddedThemeName::Base16OceanLight);
        syntect::easy::HighlightLines::new(syntax, theme)
            .highlight_line(&format!("{line}\n"), set)
            .unwrap()
            .into_iter()
            .map(|(style, _)| [style.foreground.r, style.foreground.g, style.foreground.b])
            .collect()
    }

    fn folders(root: &Path) -> Vec<(Source, PathBuf)> {
        vec![
            (Source::SublimeText, root.join("sublime-text")),
            (Source::Bat, root.join("bat")),
        ]
    }

    #[test]
    fn grammars_added_to_bat_and_sublime_text_join_bat_s_the_last_taking_the_place() {
        let tmp = tempfile::tempdir().unwrap();
        let (bat, sublime) = (tmp.path().join("bat"), tmp.path().join("sublime-text"));
        write(
            &bat.join("Skwirl.sublime-syntax"),
            &grammar("Skwirlang", "skwirl", "source.skwirl"),
        );
        write(
            &bat.join("deeper/More.sublime-syntax"),
            &grammar("More", "more", "source.more"),
        );
        write(&bat.join("broken.sublime-syntax"), "name: [never closed");
        write(
            &bat.join("notes.txt"),
            &grammar("Notes", "notes", "source.notes"),
        );
        // bat's own Rust, taken over by a grammar of the user's, as in bat.
        write(
            &sublime.join("Packages/Rusty/Rust.sublime-syntax"),
            &grammar("Rust", "rs", "source.rust.users"),
        );
        // The same language in both: bat's folder comes last, and wins.
        write(
            &sublime.join("Packages/Both/Both.sublime-syntax"),
            &grammar("Both", "both", "source.both.sublime"),
        );
        write(
            &bat.join("Both.sublime-syntax"),
            &grammar("Both", "both", "source.both.bat"),
        );
        let mut zipped = zip::ZipWriter::new(
            std::fs::File::create({
                let path = sublime.join("Installed Packages/Zipped.sublime-package");
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                path
            })
            .unwrap(),
        );
        zipped
            .start_file(
                "Zipl.sublime-syntax",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        zipped
            .write_all(grammar("Zipl", "zipl", "source.zipl").as_bytes())
            .unwrap();
        zipped.finish().unwrap();

        let (set, listed) = build(&found(&folders(tmp.path())));
        assert!(colors(&set, "skwirl", r#"let x = "y" # z"#).len() > 2);
        assert_eq!(set.find_syntax_by_token("more").unwrap().name, "More");
        assert_eq!(set.find_syntax_by_token("zipl").unwrap().name, "Zipl");
        assert_eq!(
            set.find_syntax_by_extension("rs")
                .unwrap()
                .scope
                .to_string(),
            "source.rust.users"
        );
        assert_eq!(
            set.find_syntax_by_extension("both")
                .unwrap()
                .scope
                .to_string(),
            "source.both.bat"
        );
        // bat's others are all there.
        assert!(set.find_syntax_by_token("toml").is_some());
        let named: Vec<_> = listed
            .iter()
            .map(|added| (added.name.as_str(), added.source, added.problem.is_some()))
            .collect();
        assert_eq!(
            named,
            [
                ("Both", Source::SublimeText, false),
                ("Rust", Source::SublimeText, false),
                ("Zipl", Source::SublimeText, false),
                ("Both", Source::Bat, false),
                ("Skwirlang", Source::Bat, false),
                ("broken", Source::Bat, true),
                ("More", Source::Bat, false),
            ]
        );
        assert_eq!(listed[2].member.as_deref(), Some("Zipl.sublime-syntax"));
        assert_eq!(listed[4].file_types, ["skwirl"]);
    }

    #[test]
    fn nothing_added_is_bat_s_own_set() {
        let tmp = tempfile::tempdir().unwrap();
        let (set, listed) = set_in(&folders(tmp.path()), None);
        assert!(listed.is_empty());
        assert_eq!(
            set.syntaxes().len(),
            two_face::syntax::extra_newlines().syntaxes().len()
        );
    }

    #[test]
    fn the_set_is_kept_for_its_files_and_built_again_when_one_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let slot = skwir_core::cache::Cache::new(tmp.path().join("cache")).namespace("grammars");
        let file = tmp.path().join("bat/Skwirl.sublime-syntax");
        write(&file, &grammar("Skwirlang", "skwirl", "source.skwirl"));
        let folders = folders(tmp.path());
        let (_, listed) = set_in(&folders, Some(&slot));
        assert_eq!(listed[0].name, "Skwirlang");
        let kept = sorted(&slot.folder(""));
        assert_eq!(kept.len(), 2, "{kept:?}");
        // Read again: what was kept, the same files.
        let (set, again) = set_in(&folders, Some(&slot));
        assert_eq!(again, listed);
        assert!(set.find_syntax_by_token("skwirl").is_some());
        assert_eq!(read_in(&folders, Some(&slot)), listed);
        assert_eq!(sorted(&slot.folder("")), kept);
        // Changed: built again, the old set let go of.
        write(&file, &grammar("Skwirlang Two", "skwirl", "source.skwirl"));
        let (set, changed) = set_in(&folders, Some(&slot));
        assert_eq!(changed[0].name, "Skwirlang Two");
        assert_eq!(
            set.find_syntax_by_token("skwirl").unwrap().name,
            "Skwirlang Two"
        );
        let now = sorted(&slot.folder(""));
        assert_eq!(now.len(), 2);
        assert!(now.iter().all(|path| !kept.contains(path)));
    }
}
