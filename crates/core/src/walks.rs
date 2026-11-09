//! What a walk below a folder leaves out (ADR-125): counting a folder's
//! tags (the tags plugin), searching it (the query plugin), dating it (the
//! timeline). A hidden entry is walked like any other; whether it is shown
//! is the pane's (`ListingSpec::show_hidden`).
//!
//! - **The exclusions kept from searches** (`exclusions`, the Folders
//!   page): names anywhere, paths, globs, regexes, a name beside the file
//!   that says which tool made it, a folder holding a marker (a signed
//!   `CACHEDIR.TAG`).
//! - **Always:** a network mount (ADR-077, [`mounts_below`]) and a trash,
//!   whatever the rules say: the Trash is a place of its own.
//!
//! Only what is below a walk's root is left out ([`Below`]): a search or a
//! count asked of a folder the rules name walks it.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::exclusions::{Catalog, Matcher, NameMatch, Names, Role, Rule};
use crate::fs::reach;
use crate::settings::Settings;

pub use crate::exclusions::{CACHEDIR_TAG, is_cache, tags_a_cache};

/// Changed when what is always left out changes (the trash's rule): what
/// was counted under another is counted again.
const ALWAYS: u64 = 1;

/// What a walk leaves out, as the settings said it when read.
#[derive(Clone, Debug, Default)]
pub struct LeftOut {
    rule: Rule,
    trash: Option<PathBuf>,
    fingerprint: u64,
}

impl LeftOut {
    /// What the settings say now: the exclusions kept from searches.
    pub fn of(settings: &Settings) -> LeftOut {
        LeftOut::ruled(Catalog::of(settings).rule(Role::Searches))
    }

    /// What skwir's own rules leave out, as it ships them.
    pub fn shipped() -> LeftOut {
        LeftOut::ruled(Catalog::shipped().rule(Role::Searches))
    }

    /// Only these, names, paths (`~` the home folder) or patterns: for
    /// tests.
    pub fn listed<'a>(folders: impl IntoIterator<Item = &'a str>) -> LeftOut {
        let matchers: Vec<Matcher> = folders
            .into_iter()
            .map(|text| Matcher::Pattern {
                text: text.trim().to_owned(),
                beside: None,
                names: Names::Folders,
            })
            .collect();
        LeftOut::ruled(Rule::of(&matchers))
    }

    /// Nothing of the rules (`excluded:yes`): only what is always left
    /// out, a trash and a network mount.
    pub fn none() -> LeftOut {
        LeftOut::ruled(Rule::default())
    }

    /// What `rule` leaves out, and what is always left out.
    pub fn ruled(rule: Rule) -> LeftOut {
        let fingerprint = rule.fingerprint() ^ ALWAYS.rotate_left(32);
        LeftOut {
            rule,
            trash: crate::xdg::data_home().map(|data| data.join("Trash")),
            fingerprint,
        }
    }

    /// Which rules, and which version of what is always left out, these
    /// are: what was made of a walk under others is made again.
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// The rules kept from searches.
    pub fn rule(&self) -> &Rule {
        &self.rule
    }

    /// Whether the rules, or the rule for a trash, name the folder `dir` by
    /// its name or its path: nothing is read.
    pub fn names(&self, dir: &Path) -> bool {
        dir.file_name().is_some_and(is_trash)
            || self.trash.as_deref() == Some(dir)
            || self.rule.names(dir)
    }

    /// Whether the rules name the file `path`: nothing is read. A walk
    /// that knows the rules name no file asks nothing (`Rule::names_files`).
    pub fn names_file(&self, path: &Path) -> bool {
        self.rule.names_file(path)
    }

    /// Whether a name alone is left out, wherever it is.
    pub fn names_anywhere(&self, name: &OsStr) -> bool {
        is_trash(name) || self.rule.names_anywhere(name)
    }

    /// What a walk of `root` leaves out below it; a marker or a sibling
    /// looked up only when `probe` (never on a remote, where it would ask
    /// the server for every folder: `docs/invariants/remote.md`).
    pub fn below(self: &Arc<Self>, root: &Path, probe: bool) -> Below {
        Below {
            rule: Arc::clone(self),
            root: root.to_owned(),
            mounts: mounts_below(root).into_iter().collect(),
            probe,
            looked: Mutex::default(),
        }
    }
}

/// A trash, by its name: freedesktop's on a volume (`.Trash`,
/// `.Trash-1000`), a Mac's (`.Trash`, `.Trashes`), Windows' (`$RECYCLE.BIN`,
/// `RECYCLER`).
fn is_trash(name: &OsStr) -> bool {
    let name = name.as_encoded_bytes();
    name == b".Trash"
        || name == b".Trashes"
        || name.starts_with(b".Trash-")
        || name.eq_ignore_ascii_case(b"$RECYCLE.BIN")
        || name.eq_ignore_ascii_case(b"RECYCLER")
}

/// What one walk of a folder leaves out below it: shared by the walk's
/// threads, what a folder holds and beside it looked up once per folder.
pub struct Below {
    rule: Arc<LeftOut>,
    root: PathBuf,
    mounts: HashSet<PathBuf>,
    probe: bool,
    looked: Mutex<HashMap<PathBuf, bool>>,
}

impl Below {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn rule(&self) -> &LeftOut {
        &self.rule
    }

    /// Whether the walk enters `dir`, a folder it found below its root:
    /// not a network mount, not named by the rules, not holding a marker
    /// nor beside a sibling that says so. The root itself is always entered.
    pub fn enters(&self, dir: &Path) -> bool {
        !self.leaves_out(dir)
    }

    /// Whether `dir` is left out, the root never: [`enters`](Self::enters)
    /// turned round.
    pub fn leaves_out(&self, dir: &Path) -> bool {
        if dir == self.root || !dir.starts_with(&self.root) {
            return false;
        }
        self.named(dir) || (self.probe && self.looked_up(dir))
    }

    /// Whether the file `path` found below the root is left out.
    pub fn leaves_out_file(&self, path: &Path) -> bool {
        self.rule.rule().names_files() && path.starts_with(&self.root) && self.rule.names_file(path)
    }

    /// Whether the rules name any file: when not, a walk asks nothing of
    /// the files it finds.
    pub fn names_files(&self) -> bool {
        self.rule.rule().names_files()
    }

    /// [`leaves_out_file`](Self::leaves_out_file) of a path whose kind is
    /// not known (an index's answer): looked at only where its name says
    /// it may be.
    pub fn leaves_out_path(&self, path: &Path) -> bool {
        self.leaves_out_file(path) && !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
    }

    /// Whether the rules name `dir` (a mount, a trash, a name, a path, a
    /// pattern): what is known without reading.
    pub fn named(&self, dir: &Path) -> bool {
        dir != self.root && (self.mounts.contains(dir) || self.rule.names(dir))
    }

    /// Whether the walk looks up markers and siblings.
    pub fn probes(&self) -> bool {
        self.probe
    }

    /// Whether a beside rule leaves out the folder `dir`, its parent's
    /// names known (`siblings`: whether one is there): for a walk that
    /// reads them.
    pub fn beside_among(&self, dir: &Path, siblings: impl Fn(&NameMatch) -> bool) -> bool {
        dir != self.root
            && self.probe
            && dir
                .file_name()
                .is_some_and(|name| self.rule.rule().beside(name).any(siblings))
    }

    /// Whether a marker held by `dir` (its names known: `names`) leaves it
    /// out, a `CACHEDIR.TAG` read for its signature: for a walk that reads
    /// them.
    pub fn marked_among(&self, dir: &Path, names: impl Fn(&OsStr) -> bool) -> bool {
        dir != self.root && self.probe && self.rule.rule().marked_among(dir, names)
    }

    /// The topmost folder below the root, holding `path`, that the walk
    /// leaves out: what a path named without walking (an index's
    /// candidate, plocate's answer, a change arriving) would have been
    /// left out under. `path` itself is not asked: a folder left out is
    /// still found itself.
    pub fn stops_above(&self, path: &Path) -> Option<PathBuf> {
        let rest = path.strip_prefix(&self.root).ok()?;
        let mut at = self.root.clone();
        let mut components = rest.components().peekable();
        while let Some(component) = components.next() {
            if components.peek().is_none() {
                break;
            }
            at.push(component);
            if self.leaves_out(&at) {
                return Some(at);
            }
        }
        None
    }

    /// Whether a marker or a sibling leaves out `dir`, looked up once.
    fn looked_up(&self, dir: &Path) -> bool {
        let rule = self.rule.rule();
        if rule.markers().is_empty() && !rule.has_beside() {
            return false;
        }
        if let Some(known) = self.looked.lock().get(dir) {
            return *known;
        }
        let out = rule.marked(dir) || rule.beside_found(dir);
        self.looked.lock().insert(dir.to_owned(), out);
        out
    }
}

/// The network mounts below `root` (not `root` itself), from the mount
/// table in memory: what a walk of it never enters (ADR-077), and what an
/// index names under one is left out as the walk leaves it, before
/// anything there is read.
pub fn mounts_below(root: &Path) -> Vec<PathBuf> {
    reach::all()
        .iter()
        .filter(|r| r.point != root && r.point.starts_with(root))
        .map(|r| r.point.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(folders: &[&str]) -> Arc<LeftOut> {
        Arc::new(LeftOut::listed(folders.iter().copied()))
    }

    #[test]
    fn names_are_left_out_anywhere_and_paths_where_they_are() {
        let home = crate::os_path::home().unwrap();
        let left = rule(&[
            "node_modules",
            "~/.cargo/registry",
            "/srv/cache",
            "cmake-build-*",
        ]);
        assert!(left.names(&home.join("work/app's ü/node_modules")));
        assert!(left.names(&home.join(".cargo").join("registry")));
        assert!(!left.names(&home.join(".cargo")), "a path is one folder");
        assert!(left.names(Path::new("/srv/cache")));
        assert!(left.names(&home.join("x/cmake-build-debug")));
        assert!(!left.names(&home.join("node_modules.txt/x")));
        assert!(!left.names(&home.join(".config")), "hidden is not left out");
    }

    #[test]
    fn a_trash_is_always_left_out() {
        let left = Arc::new(LeftOut::none());
        for name in [".Trash", ".Trash-1000", ".Trashes", "$RECYCLE.BIN"] {
            assert!(left.names(&Path::new("/media/disk").join(name)), "{name}");
        }
        if let Some(data) = crate::xdg::data_home() {
            assert!(left.names(&data.join("Trash")));
        }
        assert!(!left.names(Path::new("/media/disk/Trash notes")));
    }

    #[test]
    fn only_what_is_below_the_root_is_left_out() {
        let left = rule(&["node_modules"]);
        let root = Path::new("/r/node_modules");
        let below = left.below(root, false);
        assert!(below.enters(root), "a root the rules name is walked");
        assert!(below.leaves_out(&root.join("a/node_modules")));
        assert!(!below.leaves_out(Path::new("/elsewhere/node_modules")));
        assert_eq!(
            below
                .stops_above(&root.join("a/node_modules/b/c.js"))
                .as_deref(),
            Some(root.join("a/node_modules").as_path())
        );
        assert_eq!(
            below.stops_above(&root.join("a/node_modules")),
            None,
            "found itself"
        );
        assert_eq!(below.stops_above(&root.join("a/b.js")), None);
    }

    #[test]
    fn a_cache_says_so_with_a_signed_tag_looked_up_once() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tmp.path().join("built");
        let plain = tmp.path().join("plain");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::create_dir_all(&plain).unwrap();
        std::fs::write(
            cache.join(CACHEDIR_TAG),
            b"Signature: 8a477f597d28d172789f06886806bc55\n# a tool\n",
        )
        .unwrap();
        std::fs::write(plain.join(CACHEDIR_TAG), b"Signature: none").unwrap();
        let left = Arc::new(LeftOut::ruled(Rule::of(&[Matcher::Holding(
            CACHEDIR_TAG.into(),
        )])));
        let below = left.below(tmp.path(), true);
        assert!(below.leaves_out(&cache));
        assert!(!below.leaves_out(&plain), "unsigned");
        std::fs::remove_file(cache.join(CACHEDIR_TAG)).unwrap();
        assert!(below.leaves_out(&cache), "looked up once a walk");
        assert!(
            !left.below(tmp.path(), false).leaves_out(&cache),
            "not probed"
        );
        assert!(!left.below(&cache, true).leaves_out(&cache), "its own root");
    }

    #[test]
    fn a_name_beside_the_file_of_its_tool_is_left_out_there_only() {
        let tmp = tempfile::tempdir().unwrap();
        let crate_dir = tmp.path().join("it's a crate");
        let other = tmp.path().join("shooting");
        for dir in [crate_dir.join("target"), other.join("target")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(crate_dir.join("Cargo.toml"), b"[package]").unwrap();
        let left = Arc::new(LeftOut::ruled(Rule::of(&[Matcher::Pattern {
            text: "target".into(),
            beside: Some("Cargo.toml".into()),
            names: Names::Folders,
        }])));
        let below = left.below(tmp.path(), true);
        assert!(below.leaves_out(&crate_dir.join("target")));
        assert!(
            !below.leaves_out(&other.join("target")),
            "a photographer's targets"
        );
        assert!(
            !left.names(&crate_dir.join("target")),
            "known only by looking"
        );
    }

    #[test]
    fn a_file_named_is_left_out_and_a_folder_of_that_name_is_not() {
        let left = Arc::new(LeftOut::ruled(Rule::of(&[Matcher::Pattern {
            text: "*.log".into(),
            beside: None,
            names: Names::Files,
        }])));
        let tmp = tempfile::tempdir().unwrap();
        let below = left.below(tmp.path(), false);
        assert!(below.names_files());
        assert!(below.leaves_out_file(&tmp.path().join("a's run.log")));
        assert!(!below.leaves_out_file(&tmp.path().join("run.txt")));
        assert!(
            !below.leaves_out(&tmp.path().join("a.log")),
            "folders: not it"
        );
        // A path whose kind is not known is looked at.
        std::fs::create_dir(tmp.path().join("dir.log")).unwrap();
        assert!(!below.leaves_out_path(&tmp.path().join("dir.log")));
        assert!(below.leaves_out_path(&tmp.path().join("gone.log")));
        // Rules that name only folders ask nothing of a file.
        assert!(
            !rule(&["node_modules"])
                .below(tmp.path(), false)
                .names_files()
        );
    }

    #[test]
    fn the_fingerprint_is_the_rules_whatever_their_order() {
        let a = LeftOut::listed(["node_modules", "~/.cargo"]);
        let b = LeftOut::listed(["~/.cargo", "node_modules", " node_modules "]);
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_ne!(
            a.fingerprint(),
            LeftOut::listed(["node_modules"]).fingerprint()
        );
        assert_ne!(a.fingerprint(), LeftOut::none().fingerprint());
    }
}
