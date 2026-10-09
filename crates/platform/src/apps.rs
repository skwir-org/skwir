//! The desktop's applications: their desktop entries, which types each one
//! opens (`mimeapps.list` and the entries' own `MimeType`), and launching
//! one on files.
//!
//! Reading the catalog walks every applications directory: load it on a
//! worker, keep it, and ask [`Stamp::current`] (a few `stat`s) whether an
//! install or a changed default made it stale.

use crate::keyfile;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::mime::{self, Mime};

/// One installed application, from its desktop entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Application {
    /// The desktop file ID: `org.gnome.Loupe.desktop`, a subdirectory
    /// joined with `-` (`kde4-okular.desktop`).
    pub id: String,
    /// `Name`, in the user's language when the entry has it.
    pub name: String,
    /// `GenericName` (`Image Viewer`), localized likewise.
    pub generic_name: Option<String>,
    /// A theme icon name, or an absolute path.
    pub icon: Option<String>,
    /// The command line, unescaped as a string value, still quoted.
    pub exec: String,
    /// Runs in a terminal.
    pub terminal: bool,
    /// `Path`: the working directory to start in.
    pub working_dir: Option<PathBuf>,
    /// The types it declares it opens.
    pub mime_types: Vec<String>,
    /// Listed among all applications: not `NoDisplay`, and not excluded
    /// from this desktop by `OnlyShowIn` / `NotShowIn`. An entry that is
    /// not shown still opens the types it declares.
    pub shown: bool,
    /// Where the entry was read, for `%k`.
    pub source: PathBuf,
}

/// What `mimeapps.list` files say about one type.
#[derive(Clone, Debug, Default)]
struct Associations {
    /// `[Default Applications]`, the most important file's first.
    defaults: Vec<String>,
    /// `[Added Associations]` not removed by a more important file.
    added: Vec<String>,
    /// `[Removed Associations]`, from any file.
    removed: HashSet<String>,
}

/// The applications and what opens which type.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    apps: Vec<Application>,
    by_id: HashMap<String, usize>,
    /// Declared by the entries, in the order the entries were read.
    declared: HashMap<String, Vec<usize>>,
    lists: HashMap<String, Associations>,
}

/// What [`Catalog::load`] reads the list of, as it stands: each
/// applications directory and the folders in it, and the `mimeapps.list`
/// files, by where they lead and when they changed. Installing or removing
/// an application changes its directory; changing a default rewrites a
/// list. Where they lead counts as much as the time: on NixOS a profile is
/// a link into the store, whose every folder is dated 1970, so switching
/// to another changes only where it leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp(Vec<(Option<PathBuf>, Option<SystemTime>)>);

impl Stamp {
    /// Stats the directories and lists: call from a worker.
    pub fn current() -> Self {
        let paths = Sources::from_env();
        Self::of(&paths.app_dirs, &paths.lists)
    }

    /// The stamp of explicit directories and lists, as
    /// [`Catalog::from_paths`] reads them.
    pub fn of(app_dirs: &[PathBuf], lists: &[PathBuf]) -> Self {
        let mut seen = Vec::new();
        let mut dirs = app_dirs.to_vec();
        // The folders inside, as `collect_entries` walks them (not through
        // links to folders).
        let mut at = 0;
        while let Some(dir) = dirs.get(at).cloned() {
            at += 1;
            if let Ok(read) = std::fs::read_dir(&dir) {
                dirs.extend(
                    read.flatten()
                        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                        .map(|e| e.path()),
                );
            }
            seen.push(dir);
        }
        Stamp(
            seen.iter()
                .chain(lists)
                .map(|p| {
                    (
                        std::fs::canonicalize(p).ok(),
                        std::fs::metadata(p).and_then(|m| m.modified()).ok(),
                    )
                })
                .collect(),
        )
    }
}

/// Where the catalog is read from, most important first.
struct Sources {
    app_dirs: Vec<PathBuf>,
    lists: Vec<PathBuf>,
    /// The lists in `$XDG_CONFIG_HOME`, the user's own: where a default the
    /// user chooses is written.
    user_lists: Vec<PathBuf>,
}

impl Sources {
    fn from_env() -> Self {
        let data_home = crate::xdg::data_home();
        let data_dirs = crate::xdg::data_dirs();
        let config_home = crate::xdg::config_home();
        let config_dirs = crate::xdg::config_dirs();
        let data: Vec<PathBuf> = data_home
            .into_iter()
            .chain(data_dirs)
            .map(|d| d.join("applications"))
            .collect();
        let user_lists: Vec<PathBuf> = config_home
            .iter()
            .flat_map(|dir| {
                current_desktops()
                    .into_iter()
                    .map(|d| dir.join(format!("{}-mimeapps.list", d.to_lowercase())))
                    .chain([dir.join("mimeapps.list")])
            })
            .collect();
        let config: Vec<PathBuf> = config_home.into_iter().chain(config_dirs).collect();
        let mut app_dirs = data.clone();
        dedup(&mut app_dirs);
        // Per directory, the desktop's own list before the shared one.
        let mut lists = Vec::new();
        for dir in config.iter().chain(&data) {
            for desktop in current_desktops() {
                lists.push(dir.join(format!("{}-mimeapps.list", desktop.to_lowercase())));
            }
            lists.push(dir.join("mimeapps.list"));
        }
        dedup(&mut lists);
        Sources {
            app_dirs,
            lists,
            user_lists,
        }
    }
}

fn dedup(paths: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));
}

/// `$XDG_CURRENT_DESKTOP`, split: `GNOME`, `Hyprland`.
fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(str::to_owned)
        .collect()
}

impl Catalog {
    /// Read the desktop's applications and associations. Walks every
    /// applications directory: call from a worker.
    pub fn load() -> Self {
        let sources = Sources::from_env();
        Self::read(
            &sources.app_dirs,
            &sources.lists,
            &Locale::from_env(),
            &current_desktops(),
        )
    }

    /// Read from explicit directories and lists, most important first.
    pub fn from_paths(app_dirs: &[PathBuf], lists: &[PathBuf]) -> Self {
        Self::read(app_dirs, lists, &Locale::default(), &[])
    }

    fn read(app_dirs: &[PathBuf], lists: &[PathBuf], locale: &Locale, desktops: &[String]) -> Self {
        let mut catalog = Catalog::default();
        // An ID seen once is taken, even by a `Hidden` entry: that is how a
        // user's own file deletes a system one.
        let mut seen = HashSet::new();
        for dir in app_dirs {
            let mut files = Vec::new();
            collect_entries(dir, dir, &mut files);
            files.sort();
            for (id, path) in files {
                if !seen.insert(id.clone()) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Some(app) = parse_entry(&id, &path, &text, locale, desktops) else {
                    continue;
                };
                let index = catalog.apps.len();
                for m in &app.mime_types {
                    catalog
                        .declared
                        .entry(canonical(m))
                        .or_default()
                        .push(index);
                }
                catalog.by_id.insert(app.id.clone(), index);
                catalog.apps.push(app);
            }
        }
        for list in lists {
            let Ok(text) = std::fs::read_to_string(list) else {
                continue;
            };
            catalog.read_list(&text);
        }
        catalog
    }

    /// One `mimeapps.list`, less important than those read before it.
    fn read_list(&mut self, text: &str) {
        let mut removed_here: Vec<(String, String)> = Vec::new();
        for (group, key, value) in keyfile::entries(text) {
            let key = &canonical(key);
            let ids = value
                .split(';')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
            let associations = self.lists.entry(key.to_owned()).or_default();
            match group {
                "Default Applications" => associations.defaults.extend(ids),
                "Added Associations" => {
                    for id in ids {
                        if !associations.removed.contains(&id) && !associations.added.contains(&id)
                        {
                            associations.added.push(id);
                        }
                    }
                }
                "Removed Associations" => {
                    removed_here.extend(ids.map(|id| (key.to_owned(), id)));
                }
                _ => {}
            }
        }
        // A file's removals apply to the files after it, not to its own
        // additions.
        for (key, id) in removed_here {
            self.lists.entry(key).or_default().removed.insert(id);
        }
    }

    /// Every application read, shown or not.
    pub fn all(&self) -> &[Application] {
        &self.apps
    }

    pub fn get(&self, id: &str) -> Option<&Application> {
        self.by_id.get(id).map(|&i| &self.apps[i])
    }

    /// The application that opens `mime` itself, if the lists name one
    /// that is installed. Not inherited from a parent type: see
    /// [`Catalog::default_for_lineage`].
    pub fn default_for(&self, mime: &str) -> Option<&Application> {
        self.lists
            .get(mime)?
            .defaults
            .iter()
            .find_map(|id| self.get(id))
    }

    /// What opens a file of the first type of `lineage` (the type and those
    /// it descends from, nearest first: `mime::lineage`), as the desktop
    /// chooses it (the XDG MIME Applications specification, as gio and
    /// `xdg-open` read it): the default the lists name for the nearest type
    /// that has one, so a C source opens where plain text does unless it
    /// has its own; else, most lists naming no default for most types, the
    /// first application associated with the nearest type that has one,
    /// added by a list or declared by its entry, and not removed.
    pub fn default_for_lineage(&self, lineage: &[Mime]) -> Option<&Application> {
        lineage
            .iter()
            .find_map(|m| self.default_for(m.as_str()))
            .or_else(|| {
                lineage
                    .iter()
                    .find_map(|m| self.for_type(m.as_str()).into_iter().next())
            })
    }

    /// The applications associated with `mime` itself: its default, then
    /// the lists' additions, then the entries that declare it, without the
    /// lists' removals. Each once.
    pub fn for_type(&self, mime: &str) -> Vec<&Application> {
        let associations = self.lists.get(mime);
        let removed = |id: &str| associations.is_some_and(|a| a.removed.contains(id));
        let added = associations
            .iter()
            .flat_map(|a| &a.added)
            .filter_map(|id| self.get(id));
        let declared = self
            .declared
            .get(mime)
            .into_iter()
            .flatten()
            .map(|&i| &self.apps[i])
            .filter(|app| !removed(&app.id));
        let mut out: Vec<&Application> = Vec::new();
        for app in self
            .default_for(mime)
            .into_iter()
            .chain(added)
            .chain(declared)
        {
            if !out.iter().any(|a| a.id == app.id) {
                out.push(app);
            }
        }
        out
    }
}

/// A type as the catalog keeps it: by its canonical name, as GIO keeps
/// it. An editor's entry, or a default the user saved, under a name that
/// became an alias (`application/x-shellscript`, since shared-mime-info
/// 2.5.1) still applies to its files, which are typed by the new one.
fn canonical(mime: &str) -> String {
    crate::mime::unalias(&crate::mime::Mime(mime.to_owned())).0
}

/// Every `*.desktop` under `dir`, with its desktop file ID.
fn collect_entries(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // Follow links to entries (NixOS profiles are made of them), not to
        // directories, which could loop.
        if kind.is_dir() {
            collect_entries(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "desktop")
            && let Ok(relative) = path.strip_prefix(root)
            && let Some(relative) = relative.to_str()
        {
            out.push((relative.replace('/', "-"), path));
        }
    }
}

/// The user's language, as desktop entries localize keys: `Name[fr_FR]`,
/// then `Name[fr]`.
#[derive(Clone, Debug, Default)]
pub(crate) struct Locale {
    /// Most specific first.
    keys: Vec<String>,
}

impl Locale {
    fn from_env() -> Self {
        let value = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|v| std::env::var(v).ok().filter(|v| !v.is_empty()))
            .unwrap_or_default();
        Self::parse(&value)
    }

    /// `lang_COUNTRY.ENCODING@MODIFIER`, per the desktop entry spec.
    fn parse(value: &str) -> Self {
        let (rest, modifier) = value
            .split_once('@')
            .map_or((value, None), |(r, m)| (r, Some(m)));
        let rest = rest.split('.').next().unwrap_or_default();
        let (lang, country) = rest
            .split_once('_')
            .map_or((rest, None), |(l, c)| (l, Some(c)));
        if lang.is_empty() || lang == "C" || lang == "POSIX" {
            return Locale::default();
        }
        let mut keys = Vec::new();
        if let (Some(c), Some(m)) = (country, modifier) {
            keys.push(format!("{lang}_{c}@{m}"));
        }
        if let Some(c) = country {
            keys.push(format!("{lang}_{c}"));
        }
        if let Some(m) = modifier {
            keys.push(format!("{lang}@{m}"));
        }
        keys.push(lang.to_owned());
        Locale { keys }
    }
}

/// The `[Desktop Entry]` group of an application. `None` for another type,
/// a `Hidden` entry, one without `Exec`, or one whose `TryExec` is not
/// installed.
pub(crate) fn parse_entry(
    id: &str,
    path: &Path,
    text: &str,
    locale: &Locale,
    desktops: &[String],
) -> Option<Application> {
    let mut values: HashMap<&str, &str> = HashMap::new();
    for (group, key, value) in keyfile::entries(text) {
        if group == "Desktop Entry" {
            values.entry(key).or_insert(value);
        }
    }
    let localized = |key: &str| {
        locale
            .keys
            .iter()
            .find_map(|l| values.get(format!("{key}[{l}]").as_str()))
            .or_else(|| values.get(key))
            .map(|v| keyfile::unescape(v))
            .filter(|v| !v.is_empty())
    };
    if values.get("Type").copied() != Some("Application")
        || values.get("Hidden").is_some_and(|v| keyfile::is_true(v))
    {
        return None;
    }
    let exec = keyfile::unescape(values.get("Exec")?);
    if exec.trim().is_empty() {
        return None;
    }
    if let Some(try_exec) = values.get("TryExec").map(|v| keyfile::unescape(v))
        && !try_exec.is_empty()
        && bibli_core::os_path::find_program(&try_exec).is_none()
    {
        return None;
    }
    let only = values.get("OnlyShowIn").map(|v| keyfile::list(v));
    let not = values
        .get("NotShowIn")
        .map(|v| keyfile::list(v))
        .unwrap_or_default();
    let here = |names: &[String]| desktops.iter().any(|d| names.contains(d));
    let shown = !values.get("NoDisplay").is_some_and(|v| keyfile::is_true(v))
        && only.is_none_or(|only| here(&only))
        && !here(&not);
    Some(Application {
        id: id.to_owned(),
        name: localized("Name").unwrap_or_else(|| id.trim_end_matches(".desktop").to_owned()),
        generic_name: localized("GenericName"),
        icon: localized("Icon"),
        exec,
        terminal: values.get("Terminal").is_some_and(|v| keyfile::is_true(v)),
        working_dir: values
            .get("Path")
            .map(|p| PathBuf::from(keyfile::unescape(p)))
            .filter(|p| p.is_absolute()),
        mime_types: values
            .get("MimeType")
            .map(|v| keyfile::list(v))
            .unwrap_or_default(),
        shown,
        source: path.to_path_buf(),
    })
}

/// What one run of an application is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Takes {
    /// `%F` or `%U`: every file in one run.
    Many,
    /// `%f` or `%u`, or no field code at all: one run per file.
    One,
}

impl Application {
    fn takes(&self) -> Takes {
        let many = keyfile::exec_words(&self.exec)
            .iter()
            .any(|(w, quoted)| !quoted && (w == "%F" || w == "%U"));
        if many { Takes::Many } else { Takes::One }
    }

    /// The command lines that open `files`: one when the application takes
    /// several files at once, else one per file. Paths go in as they are,
    /// bytes and all; `%u` and `%U` give `file://` URIs. An `Exec` without
    /// a field code gets the file appended, as the desktop does, so a
    /// program that says nothing about files still receives the one chosen.
    pub fn command_lines(&self, files: &[PathBuf]) -> Vec<Vec<OsString>> {
        match self.takes() {
            Takes::Many => vec![self.expand(files)],
            Takes::One if files.is_empty() => vec![self.expand(&[])],
            Takes::One => files
                .iter()
                .map(|f| self.expand(std::slice::from_ref(f)))
                .collect(),
        }
    }

    fn expand(&self, files: &[PathBuf]) -> Vec<OsString> {
        let mut argv: Vec<OsString> = Vec::new();
        let mut used_files = false;
        for (word, quoted) in keyfile::exec_words(&self.exec) {
            if quoted {
                argv.push(word.replace("%%", "%").into());
                continue;
            }
            match word.as_str() {
                "%F" | "%U" => {
                    used_files = true;
                    argv.extend(files.iter().map(|f| file_arg(f, word == "%U")));
                    continue;
                }
                "%i" => {
                    if let Some(icon) = &self.icon {
                        argv.push("--icon".into());
                        argv.push(icon.into());
                    }
                    continue;
                }
                _ => {}
            }
            let mut out = OsString::new();
            let mut literal = String::new();
            let mut chars = word.chars();
            while let Some(c) = chars.next() {
                if c != '%' {
                    literal.push(c);
                    continue;
                }
                match chars.next() {
                    Some(code @ ('f' | 'u')) => {
                        used_files = true;
                        out.push(std::mem::take(&mut literal));
                        if let Some(file) = files.first() {
                            out.push(file_arg(file, code == 'u'));
                        }
                    }
                    Some('c') => literal.push_str(&self.name),
                    Some('k') => {
                        out.push(std::mem::take(&mut literal));
                        out.push(&self.source);
                    }
                    Some('%') => literal.push('%'),
                    // Deprecated codes, and the ones consumed above when
                    // not a whole argument, expand to nothing.
                    _ => {}
                }
            }
            out.push(literal);
            if !out.is_empty() {
                argv.push(out);
            }
        }
        if !used_files {
            argv.extend(files.iter().map(|f| f.as_os_str().to_owned()));
        }
        argv
    }

    /// Open `files` with this application, detached. A terminal
    /// application runs in the user's terminal. The working directory is
    /// the entry's `Path`, else the folder of the first file.
    ///
    /// # Errors
    /// When the program cannot be started, or the entry's command line is
    /// empty.
    pub fn launch(&self, files: &[PathBuf]) -> anyhow::Result<()> {
        let dir = self
            .working_dir
            .clone()
            .or_else(|| {
                files
                    .first()
                    .and_then(|f| f.parent())
                    .map(Path::to_path_buf)
            })
            .or_else(bibli_core::os_path::home)
            .unwrap_or_else(|| PathBuf::from("/"));
        for argv in self.command_lines(files) {
            let Some((program, args)) = argv.split_first() else {
                anyhow::bail!("{} has no command to run", self.name);
            };
            if self.terminal {
                crate::open::open_terminal_watched(&dir, &argv, |_| {})?;
                continue;
            }
            let mut command = std::process::Command::new(program);
            command
                .args(args)
                .current_dir(&dir)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            crate::process::spawn_detached(&mut command)
                .map_err(|e| anyhow::anyhow!("launch {}: {e}", self.name))?;
        }
        Ok(())
    }
}

fn file_arg(path: &Path, uri: bool) -> OsString {
    if uri {
        crate::uri::file_uri(path).into()
    } else {
        path.as_os_str().to_owned()
    }
}

/// The type `entry`, at `path`, is opened as: what it is when that is no
/// file (a folder is `inode/directory`, a pipe `inode/fifo`, a link that
/// leads nowhere `inode/symlink`), never read; else what [`mime::identify`]
/// says of what it opens (a link's target, by the target's name), and
/// `application/octet-stream` when nothing does. Reads the head of the
/// file: call from a worker.
pub fn type_of(entry: &bibli_core::Entry, path: &Path) -> Mime {
    if let Some(mime) = entry.inode_type() {
        return Mime(mime.to_owned());
    }
    entry
        .content(path)
        .and_then(|content| mime::identify(entry.type_name(), &content.path))
        .unwrap_or_else(|| Mime("application/octet-stream".into()))
}

// ------------------------------------------------------ choosing a default

/// What one type's lines say in one `mimeapps.list`, as data: what a
/// command writes, and what [`edit_associations`] hands back to take it
/// back. Each is the value after `=` (`org.gnome.Loupe.desktop;`), `None`
/// for no line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssociationEdit {
    pub file: PathBuf,
    pub mime: String,
    /// `[Default Applications]`.
    pub default: Option<String>,
    /// `[Added Associations]`.
    pub added: Option<String>,
    /// `[Removed Associations]`.
    pub removed: Option<String>,
}

/// The lists in `$XDG_CONFIG_HOME`, the only ones a default is written to:
/// the desktop's own first, then `mimeapps.list`.
pub fn user_lists() -> Vec<PathBuf> {
    Sources::from_env().user_lists
}

const DEFAULTS: &str = "Default Applications";
const ADDED: &str = "Added Associations";
const REMOVED: &str = "Removed Associations";

impl AssociationEdit {
    /// The edit that makes `app` the default for `mime` in the user's
    /// lists: in the one of `$XDG_CONFIG_HOME` that already names a default
    /// for it (a desktop's own list is read first and would win), else in
    /// `mimeapps.list`. As the desktop does, the application is also added
    /// to the type's associations and taken out of its removals, so it is
    /// recommended for the type from then on. Reads the lists: call from a
    /// worker.
    ///
    /// # Errors
    /// When there is no configuration directory, or a list cannot be read.
    pub fn make_default(mime: &str, app: &str) -> anyhow::Result<Self> {
        Self::make_default_in(&user_lists(), mime, app)
    }

    /// [`AssociationEdit::make_default`] among explicit `lists`, most
    /// important first.
    ///
    /// # Errors
    /// When there is no `mimeapps.list` among them, or one cannot be read.
    pub fn make_default_in(lists: &[PathBuf], mime: &str, app: &str) -> anyhow::Result<Self> {
        let mut texts = Vec::new();
        for list in lists {
            let text = match std::fs::read_to_string(list) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(anyhow::anyhow!("{}: {e}", list.display())),
            };
            texts.push(text);
        }
        let index = texts
            .iter()
            .position(|text| line(text, DEFAULTS, mime).is_some())
            .or_else(|| {
                lists
                    .iter()
                    .position(|l| l.file_name().is_some_and(|n| n == "mimeapps.list"))
            })
            .ok_or_else(|| anyhow::anyhow!("no configuration directory"))?;
        Ok(Self::making_default(
            lists[index].clone(),
            &texts[index],
            mime,
            app,
        ))
    }

    /// [`AssociationEdit::make_default`] in `file`, whose text is `text`.
    fn making_default(file: PathBuf, text: &str, mime: &str, app: &str) -> Self {
        let ids = |value: Option<String>| -> Vec<String> {
            value.as_deref().map(keyfile::list).unwrap_or_default()
        };
        let joined = |ids: Vec<String>| {
            (!ids.is_empty()).then(|| {
                ids.iter()
                    .flat_map(|id| [id.as_str(), ";"])
                    .collect::<String>()
            })
        };
        let mut added = ids(line(text, ADDED, mime));
        added.retain(|id| id != app);
        added.insert(0, app.to_owned());
        let mut removed = ids(line(text, REMOVED, mime));
        removed.retain(|id| id != app);
        AssociationEdit {
            file,
            mime: mime.to_owned(),
            default: Some(format!("{app};")),
            added: joined(added),
            removed: joined(removed),
        }
    }
}

/// The value of `key` in `group`, the first if written twice.
fn line(text: &str, group: &str, key: &str) -> Option<String> {
    keyfile::entries(text)
        .find(|&(g, k, _)| g == group && k == key)
        .map(|(_, _, v)| v.trim_end().to_owned())
}

/// Set `key` in `group` to `value`, or remove it for `None`. Other lines,
/// comments and blank lines stay as written; a missing group is added at
/// the end.
fn set_line(lines: &mut Vec<String>, group: &str, key: &str, value: Option<&str>) {
    let header = format!("[{group}]");
    let is_header = |l: &str| l.trim_start().starts_with('[');
    let Some(start) = lines.iter().position(|l| l.trim() == header) else {
        if let Some(value) = value {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.push(format!("{key}={value}"));
        }
        return;
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| is_header(l))
        .map_or(lines.len(), |i| start + 1 + i);
    let existing = (start + 1..end).find(|&i| {
        let l = lines[i].trim_start();
        !l.starts_with('#') && l.split_once('=').is_some_and(|(k, _)| k.trim_end() == key)
    });
    match (existing, value) {
        (Some(i), Some(value)) => lines[i] = format!("{key}={value}"),
        (Some(i), None) => {
            lines.remove(i);
        }
        (None, Some(value)) => {
            // After the group's last line, before the blank lines that
            // separate it from the next.
            let at = (start + 1..end)
                .rev()
                .find(|&i| !lines[i].trim().is_empty())
                .map_or(start + 1, |i| i + 1);
            lines.insert(at, format!("{key}={value}"));
        }
        (None, None) => {}
    }
}

/// Apply `edit` to the text of a `mimeapps.list`: the new text and the edit
/// that reverts it, or `None` when the lines already say that.
fn edit_list_text(text: &str, edit: &AssociationEdit) -> Option<(String, AssociationEdit)> {
    let groups = [
        (DEFAULTS, &edit.default),
        (ADDED, &edit.added),
        (REMOVED, &edit.removed),
    ];
    let before: Vec<Option<String>> = groups
        .iter()
        .map(|(group, _)| line(text, group, &edit.mime))
        .collect();
    if groups
        .iter()
        .zip(&before)
        .all(|((_, want), have)| *want == have)
    {
        return None;
    }
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    for (group, value) in groups {
        set_line(&mut lines, group, &edit.mime, value.as_deref());
    }
    let mut updated = lines.join("\n");
    updated.push('\n');
    let mut before = before.into_iter();
    let inverse = AssociationEdit {
        file: edit.file.clone(),
        mime: edit.mime.clone(),
        default: before.next().flatten(),
        added: before.next().flatten(),
        removed: before.next().flatten(),
    };
    Some((updated, inverse))
}

/// Write `edit` into its list. Returns the edit that reverts it, or `None`
/// when the list already said that. A link is followed and a read-only list
/// refused ([`crate::file::rewrite`]). Writes a file: call from a worker.
///
/// # Errors
/// When the list cannot be read or written.
pub fn edit_associations(edit: &AssociationEdit) -> anyhow::Result<Option<AssociationEdit>> {
    crate::file::rewrite(&edit.file, "the list of default applications", |text| {
        edit_list_text(text, edit)
    })
}

#[cfg(test)]
// Helpers of the tests that run on Linux alone (ADR-035).
#[cfg_attr(not(target_os = "linux"), allow(dead_code, unused_imports))]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, text: &str) {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn app(exec: &str) -> Application {
        Application {
            id: "x.desktop".into(),
            name: "X".into(),
            generic_name: None,
            icon: Some("x-icon".into()),
            exec: exec.into(),
            terminal: false,
            working_dir: None,
            mime_types: Vec::new(),
            shown: true,
            source: "/apps/x.desktop".into(),
        }
    }

    fn lines(app: &Application, files: &[&str]) -> Vec<Vec<String>> {
        let files: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
        app.command_lines(&files)
            .into_iter()
            .map(|argv| argv.into_iter().map(|a| a.into_string().unwrap()).collect())
            .collect()
    }

    // Unix's paths.
    #[cfg(unix)]
    #[test]
    fn field_codes_expand_per_the_spec() {
        let files = ["/d/it's a b.txt", "/d/ünï.txt"];
        assert_eq!(
            lines(&app("viewer --new %F"), &files),
            [vec!["viewer", "--new", "/d/it's a b.txt", "/d/ünï.txt"]],
            "%F: one run with every file"
        );
        assert_eq!(
            lines(&app("viewer %u"), &files),
            [
                vec!["viewer", "file:///d/it's%20a%20b.txt"],
                vec!["viewer", "file:///d/%C3%BCn%C3%AF.txt"]
            ],
            "%u: one run per file, as a URI"
        );
        assert_eq!(
            lines(&app("game"), &files[..1]),
            [vec!["game", "/d/it's a b.txt"]],
            "no field code: the file is appended"
        );
        assert_eq!(
            lines(&app("tool --title=%c %i --file=%f %d %%"), &files[..1]),
            [vec![
                "tool",
                "--title=X",
                "--icon",
                "x-icon",
                "--file=/d/it's a b.txt",
                "%"
            ]],
        );
        assert_eq!(
            lines(&app(r#"sh -c "echo \"%f\" \$HOME" %f"#), &files[..1]),
            [vec!["sh", "-c", r#"echo "%f" $HOME"#, "/d/it's a b.txt"]],
            "quoted arguments keep their characters and are not expanded"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_reaches_the_program_as_it_is() {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::OsStr::from_bytes(b"/d/caf\xe9.txt");
        let argv = app("viewer %f").command_lines(&[PathBuf::from(name)]);
        assert_eq!(argv[0][1].as_bytes(), name.as_bytes());
    }

    /// A profile switched to another store path changes no time (the
    /// store dates every folder 1970), only where the link leads: the
    /// catalog is read again all the same.
    #[cfg(unix)]
    #[test]
    fn a_profile_switched_to_another_folder_changes_the_stamp() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("store-a"), tmp.path().join("store-b"));
        for dir in [&a, &b] {
            std::fs::create_dir(dir).unwrap();
            std::fs::File::open(dir)
                .unwrap()
                .set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1))
                .unwrap();
        }
        let profile = tmp.path().join("applications");
        std::os::unix::fs::symlink(&a, &profile).unwrap();
        let before = Stamp::of(std::slice::from_ref(&profile), &[]);
        std::fs::remove_file(&profile).unwrap();
        std::os::unix::fs::symlink(&b, &profile).unwrap();
        assert_ne!(Stamp::of(std::slice::from_ref(&profile), &[]), before);
    }

    #[test]
    fn locales_fall_back_from_the_most_specific() {
        assert_eq!(
            Locale::parse("fr_FR.UTF-8@euro").keys,
            ["fr_FR@euro", "fr_FR", "fr@euro", "fr"]
        );
        assert!(Locale::parse("C.UTF-8").keys.is_empty());
    }

    // The free desktops' files (step E, ADR-035).
    #[cfg(target_os = "linux")]
    #[test]
    fn catalog_reads_entries_and_lists_in_precedence() {
        let tmp = tempfile::tempdir().unwrap();
        let user = tmp.path().join("user/applications");
        let system = tmp.path().join("system/applications");
        let entry = |name: &str, mime: &str, extra: &str| {
            format!(
                "[Desktop Entry]\nType=Application\nName={name}\nName[fr]={name} (fr)\nExec={name} %F\nMimeType={mime}\n{extra}\n[Desktop Action new]\nName=Other\nExec=other\n"
            )
        };
        write(
            &system,
            "viewer.desktop",
            &entry("viewer", "image/png;", ""),
        );
        write(
            &system,
            "editor.desktop",
            &entry("editor", "image/png;text/plain;", ""),
        );
        write(&system, "gone.desktop", &entry("gone", "image/png;", ""));
        write(
            &user,
            "gone.desktop",
            "[Desktop Entry]\nType=Application\nHidden=true\n",
        );
        write(
            &system,
            "hidden-helper.desktop",
            &entry("helper", "image/png;", "NoDisplay=true"),
        );
        write(
            &system,
            "missing.desktop",
            &entry("missing", "image/png;", "TryExec=/nonexistent/bin"),
        );
        write(&system, "kde/sub.desktop", &entry("sub", "text/plain;", ""));
        write(
            &system,
            "link.desktop",
            "[Desktop Entry]\nType=Link\nURL=https://example.org\n",
        );
        let user_list = tmp.path().join("config/mimeapps.list");
        write(
            tmp.path(),
            "config/mimeapps.list",
            "[Default Applications]\nimage/png=absent.desktop;editor.desktop;\n\n[Removed Associations]\nimage/png=viewer.desktop;\n",
        );
        let system_list = system.join("mimeapps.list");
        write(
            &system,
            "mimeapps.list",
            "[Default Applications]\nimage/png=viewer.desktop\n[Added Associations]\nimage/png=kde-sub.desktop;viewer.desktop;\n",
        );
        let catalog = Catalog::read(
            &[user, system],
            &[user_list, system_list],
            &Locale::parse("fr_FR.UTF-8"),
            &[],
        );
        let ids =
            |apps: Vec<&Application>| apps.into_iter().map(|a| a.id.clone()).collect::<Vec<_>>();
        assert!(
            catalog.get("gone.desktop").is_none(),
            "hidden by the user's entry"
        );
        assert!(
            catalog.get("missing.desktop").is_none(),
            "TryExec not installed"
        );
        assert!(catalog.get("link.desktop").is_none(), "not an application");
        assert_eq!(catalog.get("kde-sub.desktop").unwrap().name, "sub (fr)");
        assert!(!catalog.get("hidden-helper.desktop").unwrap().shown);
        assert_eq!(
            catalog.get("viewer.desktop").unwrap().exec,
            "viewer %F",
            "the action's Exec does not replace the entry's"
        );
        assert_eq!(
            catalog.default_for("image/png").unwrap().id,
            "editor.desktop",
            "the first installed default of the most important list"
        );
        assert_eq!(
            ids(catalog.for_type("image/png")),
            ["editor.desktop", "kde-sub.desktop", "hidden-helper.desktop"],
            "default, added, then declared; the user's removal wins over the system's addition"
        );
        assert_eq!(
            ids(catalog.for_type("text/plain")),
            ["editor.desktop", "kde-sub.desktop"]
        );
        let lineage = [Mime("text/x-csrc".into()), Mime("text/plain".into())];
        assert_eq!(
            catalog.default_for_lineage(&lineage).unwrap().id,
            "editor.desktop",
            "no default named: the first application of the nearest type that has one"
        );
    }

    #[test]
    fn the_default_is_the_one_named_else_the_first_associated() {
        let tmp = tempfile::tempdir().unwrap();
        let apps = tmp.path().join("applications");
        let entry = |name: &str, mime: &str| {
            format!(
                "[Desktop Entry]\nType=Application\nName={name}\nExec={name} %F\nMimeType={mime}\n"
            )
        };
        write(&apps, "aviewer.desktop", &entry("aviewer", "image/png;"));
        write(&apps, "gallery.desktop", &entry("gallery", "image/png;"));
        write(
            &apps,
            "hex.desktop",
            &entry("hex", "application/octet-stream;"),
        );
        write(&apps, "cc.desktop", &entry("cc", "text/x-csrc;"));
        write(&apps, "notes.desktop", &entry("notes", "text/plain;"));
        write(&apps, "player.desktop", &entry("player", ""));
        let list = tmp.path().join("mimeapps.list");
        write(
            tmp.path(),
            "mimeapps.list",
            "[Added Associations]\nimage/png=gallery.desktop;\nvideo/mp4=player.desktop;\n\n\
             [Default Applications]\ntext/plain=notes.desktop;\n",
        );
        let catalog = Catalog::from_paths(&[apps], &[list]);
        let default = |types: &[&str]| {
            let lineage: Vec<Mime> = types.iter().map(|t| Mime((*t).into())).collect();
            catalog.default_for_lineage(&lineage).map(|a| a.id.clone())
        };
        assert_eq!(
            default(&["image/png", "application/octet-stream"]).as_deref(),
            Some("gallery.desktop"),
            "no default named: the list's addition before the entries that declare the type"
        );
        assert_eq!(
            default(&["video/mp4", "application/octet-stream"]).as_deref(),
            Some("player.desktop"),
            "added by a list only"
        );
        assert_eq!(
            default(&["text/x-csrc", "text/plain", "application/octet-stream"]).as_deref(),
            Some("notes.desktop"),
            "a parent's named default before the type's own associations"
        );
        assert_eq!(
            default(&["application/x-thing", "application/octet-stream"]).as_deref(),
            Some("hex.desktop"),
            "what opens any stream of bytes, as the desktop would"
        );
        assert_eq!(default(&["inode/directory"]), None, "nothing opens it");
    }

    #[test]
    fn a_default_is_written_among_the_lines_others_wrote_and_undone_exactly() {
        let text = "# managed by hand\n[Default Applications]\ntext/plain=editor.desktop\nimage/png=old.desktop;\n\n[Added Associations]\nimage/png=other.desktop;viewer.desktop;\n\n[Removed Associations]\nimage/png=viewer.desktop;gone.desktop;\n";
        let edit =
            AssociationEdit::making_default("/list".into(), text, "image/png", "viewer.desktop");
        assert_eq!(edit.default.as_deref(), Some("viewer.desktop;"));
        assert_eq!(
            edit.added.as_deref(),
            Some("viewer.desktop;other.desktop;"),
            "added first, once"
        );
        assert_eq!(
            edit.removed.as_deref(),
            Some("gone.desktop;"),
            "no longer removed"
        );
        let (updated, inverse) = edit_list_text(text, &edit).unwrap();
        assert_eq!(
            updated,
            "# managed by hand\n[Default Applications]\ntext/plain=editor.desktop\nimage/png=viewer.desktop;\n\n[Added Associations]\nimage/png=viewer.desktop;other.desktop;\n\n[Removed Associations]\nimage/png=gone.desktop;\n"
        );
        assert!(
            edit_list_text(&updated, &edit).is_none(),
            "applying again changes nothing"
        );
        let (restored, redo) = edit_list_text(&updated, &inverse).unwrap();
        assert_eq!(restored, text);
        assert_eq!(redo, edit);
    }

    #[test]
    fn a_first_default_creates_its_groups_and_its_undo_removes_the_lines() {
        let text = "[Added Associations]\ntext/plain=editor.desktop;\n";
        let edit = AssociationEdit::making_default(
            "/list".into(),
            text,
            "inode/directory",
            "files.desktop",
        );
        let (updated, inverse) = edit_list_text(text, &edit).unwrap();
        assert_eq!(
            updated,
            "[Added Associations]\ntext/plain=editor.desktop;\ninode/directory=files.desktop;\n\n[Default Applications]\ninode/directory=files.desktop;\n"
        );
        assert_eq!(inverse.default, None);
        let (restored, _) = edit_list_text(&updated, &inverse).unwrap();
        assert_eq!(
            restored,
            "[Added Associations]\ntext/plain=editor.desktop;\n\n[Default Applications]\n",
            "the lines go; an empty group is harmless"
        );
        let (empty, _) = edit_list_text("", &edit).unwrap();
        assert_eq!(
            empty,
            "[Default Applications]\ninode/directory=files.desktop;\n\n[Added Associations]\ninode/directory=files.desktop;\n"
        );
    }

    #[test]
    fn a_default_written_to_a_file_is_what_the_catalog_reads() {
        let tmp = tempfile::tempdir().unwrap();
        let apps = tmp.path().join("applications");
        write(
            &apps,
            "viewer.desktop",
            "[Desktop Entry]\nType=Application\nName=Viewer\nExec=viewer %f\n",
        );
        let list = tmp.path().join("config/mimeapps.list");
        let edit = AssociationEdit::making_default(list.clone(), "", "image/png", "viewer.desktop");
        let inverse = edit_associations(&edit).unwrap().unwrap();
        let catalog = Catalog::from_paths(std::slice::from_ref(&apps), std::slice::from_ref(&list));
        assert_eq!(
            catalog.default_for("image/png").unwrap().id,
            "viewer.desktop"
        );
        assert_eq!(catalog.for_type("image/png")[0].id, "viewer.desktop");
        edit_associations(&inverse).unwrap().unwrap();
        let catalog = Catalog::from_paths(&[apps], &[list]);
        assert!(catalog.default_for("image/png").is_none());
    }
}
