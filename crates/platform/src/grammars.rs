//! Tree-sitter grammars found (ADR-110): where Skwir, the tree-sitter CLI,
//! Zed, Helix, Neovim, the system and Emacs keep them, each with the
//! queries that color it and the files it colors.
//!
//! A grammar is a library (a shared library, or a WebAssembly module Zed
//! builds) and queries (`highlights.scm` and its kind). Sources come in
//! this order: Skwir's own folder, the CLI's, Zed's, Helix's, Neovim's,
//! the system's (its parser folders, and `libtree-sitter-<lang>` in its
//! library folders), Emacs's (its own folders). A source is named by the
//! folder a grammar was found in, never by how its library is named.
//! Each source's grammar of a language is kept (`all`); the one used
//! (`found`) is the source the Reading page chose for the language
//! (`choose`), else the first usable. A grammar is whole (one source's
//! library and queries) or combined by one rule (ADR-110, amended): a
//! library without queries takes those of its grammar's own repository
//! (Skwir's folder, the CLI's) or the system's, never an editor's, each
//! tried in turn, and is checked to fit before the Reading page offers
//! it (`check_combined`), named then by the queries that color it. A
//! whole grammar colors with its own queries alone. A part is never
//! listed alone. A language is known
//! by its key (`key_of`), as its library's symbol names it. What files a
//! language colors is said by `languages.toml` in Skwir's folder first,
//! then its own source, another source, bat's grammar of that name, and
//! last its name taken as an extension. Where they were looked for, and
//! what was nearly found, is kept beside them (`report`).
//!
//! Finding reads folders and small files: a worker's work, at start and
//! when the settings open, as bat's themes are.
use parking_lot::RwLock;
use skwir_core::syntax::{self, Grammar};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Far more than a grammar's description or a query holds.
const LIMIT: u64 = 4 << 20;
/// Grammars found at most, every source together.
const MOST: usize = 2000;
/// How many languages a query may inherit, through each other.
const INHERITS: usize = 16;

/// Where a grammar was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Skwir,
    TreeSitter,
    Zed,
    Helix,
    Neovim,
    System,
    Emacs,
}

impl Source {
    /// Every source, in the order they come.
    pub const ALL: [Source; 7] = [
        Source::Skwir,
        Source::TreeSitter,
        Source::Zed,
        Source::Helix,
        Source::Neovim,
        Source::System,
        Source::Emacs,
    ];

    /// How the settings name it (`reading.grammar_for`).
    pub fn key(self) -> &'static str {
        match self {
            Source::Skwir => "skwir",
            Source::TreeSitter => "tree-sitter",
            Source::Zed => "zed",
            Source::Helix => "helix",
            Source::Neovim => "neovim",
            Source::System => "system",
            Source::Emacs => "emacs",
        }
    }

    /// The source the settings name `key`.
    pub fn of_key(key: &str) -> Option<Source> {
        Source::ALL.into_iter().find(|source| source.key() == key)
    }

    /// Its name at the head of a choice ("Zed", "Skwir's folder").
    pub fn short(self) -> &'static str {
        match self {
            Source::Skwir => "Skwir's folder",
            Source::TreeSitter => "tree-sitter CLI",
            Source::Zed => "Zed",
            Source::Helix => "Helix",
            Source::Neovim => "Neovim",
            Source::System => "System",
            Source::Emacs => "Emacs",
        }
    }

    /// Its name, as the Reading page says it.
    pub fn label(self) -> &'static str {
        match self {
            Source::Skwir => "Skwir",
            Source::TreeSitter => "the tree-sitter CLI",
            Source::Zed => "Zed",
            Source::Helix => "Helix",
            Source::Neovim => "Neovim",
            Source::System => "the system",
            Source::Emacs => "Emacs",
        }
    }

    /// Its library, as a combined grammar names it after its queries'
    /// source ("tree-sitter CLI, with Emacs's library", ADR-110).
    pub fn library(self) -> &'static str {
        match self {
            Source::Skwir => "a library in Skwir's folder",
            Source::TreeSitter => "the tree-sitter CLI's library",
            Source::Zed => "Zed's library",
            Source::Helix => "Helix's library",
            Source::Neovim => "Neovim's library",
            Source::System => "the system's library",
            Source::Emacs => "Emacs's library",
        }
    }

    /// Whose queries may color another source's library: the grammar's
    /// own repository's (Skwir's folder, the CLI's) and the system's, as
    /// its package ships them; never an editor's (ADR-110, amended).
    fn lends(self) -> bool {
        matches!(self, Source::Skwir | Source::TreeSitter | Source::System)
    }
}

/// A language's grammar as found: what the helper loads, where it came
/// from, the files it colors, and what makes it unusable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub grammar: Grammar,
    pub source: Source,
    /// Where its queries came from, when another source than its own,
    /// and the folder they lie in (the grammar's repository).
    pub queries_from: Option<Source>,
    pub queries_at: Option<PathBuf>,
    /// Where each set of `grammar.lent` came from, and its folder: what a
    /// combined grammar is named by once its check says that set colors
    /// it (`colored_by`).
    pub lent_from: Vec<(Source, Option<PathBuf>)>,
    /// Extensions (without their dot) and whole names it colors.
    pub file_types: Vec<String>,
    /// What a file's first line says when its name says nothing.
    pub first_line: Option<String>,
    pub problem: Option<String>,
    /// A grammar combined from two sources, its library and its queries,
    /// not yet checked to fit (`check_combined`): colors as it is, offered
    /// on the Reading page once checked.
    pub checking: bool,
    /// Why its check could not be made (the helper did not start, did not
    /// answer, ended): no answer about the grammar, so never kept. It
    /// waits, asked again once the grammars are found again (`find`).
    pub unchecked: Option<String>,
}

impl Found {
    /// Whether it waits for a check that may be asked now.
    pub fn waits(&self) -> bool {
        self.checking && self.unchecked.is_none()
    }
}

/// The folders each source keeps grammars in, as found on this system;
/// tests make their own.
#[derive(Clone, Debug, Default)]
pub struct Roots {
    /// Skwir's own: grammar repositories, and `languages.toml`.
    pub skwir: Option<PathBuf>,
    /// The tree-sitter CLI's configuration (`config.json`) and the folder
    /// of the libraries it builds.
    pub tree_sitter_config: Option<PathBuf>,
    pub tree_sitter_libraries: Option<PathBuf>,
    /// Zed's installed extensions.
    pub zed: Vec<PathBuf>,
    /// Helix's runtime folders (`grammars/`, `queries/`), and its user
    /// `languages.toml`.
    pub helix: Vec<PathBuf>,
    pub helix_languages: Option<PathBuf>,
    /// Neovim's parser folders, and its query folders, in its order.
    pub neovim_parsers: Vec<PathBuf>,
    pub neovim_queries: Vec<PathBuf>,
    /// The system's parser folders (`<lang>.so`) and query folders
    /// (`<lang>/highlights.scm`).
    pub system_parsers: Vec<PathBuf>,
    pub system_queries: Vec<PathBuf>,
    /// The system's library folders, holding `libtree-sitter-<lang>`
    /// libraries a package installed: the system's grammars, its queries
    /// those of `system_queries` (Emacs looks there too, but did not put
    /// them there).
    pub system_libraries: Vec<PathBuf>,
    /// Emacs's own folders of `libtree-sitter-<lang>` libraries, where it
    /// builds them (`treesit-install-language-grammar`).
    pub emacs: Vec<PathBuf>,
}

/// The grammars as last found, the choices they were picked with, and the
/// pick: one lock, so a pick is always of the grammars and the choices
/// kept beside it.
struct State {
    /// Every source's grammar of each language; `None` before they are
    /// found.
    all: Option<Arc<[Found]>>,
    /// The folders their files lie in, as they really are: what the helper
    /// reads (`Found` files, `confine`).
    folders: Vec<PathBuf>,
    /// The languages whose grammar is chosen from a source.
    chosen: BTreeMap<String, Source>,
    /// The grammar used for each language, picked from `all` as `chosen`
    /// says; `None` before the grammars are found.
    found: Option<Arc<[Found]>>,
    /// Counts the picks that changed `found` (`generation`).
    generation: u64,
    /// Where they were looked for, as last found.
    report: Option<Arc<Report>>,
    /// What each combined grammar's check answered, by its files as they
    /// were (`fit_key`). Kept for the run.
    fits: Option<HashMap<FitKey, Fit>>,
}

/// What a combined grammar's check answered (`check_combined`): the
/// helper's answer about these files, kept for the run.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Fit {
    /// It fits: the set of its queries at this place of
    /// `Grammar::query_sets` colors it.
    Colors(usize),
    /// Its library cannot be used here (it does not load, its ABI is not
    /// one this tree-sitter reads): why, as the helper says.
    Library(String),
    /// None of its highlights queries compiles against its library: why
    /// the first does not.
    Queries(String),
}

/// What a check came to: an answer about the grammar, kept, or the
/// helper's own failure, which says nothing of it and is never kept.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Answer {
    Fit(Fit),
    Failed(String),
}

impl Answer {
    /// What the helper's check answered (`tree_sitter::Helper::check`).
    fn of(checked: anyhow::Result<crate::tree_sitter::Checked>) -> Answer {
        let checked = match checked {
            Ok(checked) => checked,
            Err(error) => return Answer::Failed(format!("{error:#}")),
        };
        Answer::Fit(match (checked.refused, checked.colorless) {
            (Some(why), _) => Fit::Library(why),
            // An older helper says no set: its own, the first.
            (None, None) => Fit::Colors(checked.colored_by.unwrap_or(0)),
            (None, Some(colorless)) => Fit::Queries(
                checked
                    .problems
                    .into_iter()
                    .find(|problem| problem.starts_with("highlights"))
                    .unwrap_or(colorless),
            ),
        })
    }
}

static STATE: RwLock<State> = RwLock::new(State {
    all: None,
    folders: Vec::new(),
    chosen: BTreeMap::new(),
    found: None,
    generation: 0,
    report: None,
    fits: None,
});

/// Whether a check of the combined grammars is asked and not done
/// (`take_waiting`, `Asked`): one asked at a time.
static CHECK_ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The grammar used for each language, as last found and chosen. Finding
/// them again (`find`) is a worker's work.
pub fn found() -> Option<Arc<[Found]>> {
    STATE.read().found.clone()
}

/// Every source's grammar of each language, by language, in the order the
/// sources come: what the Reading page offers to choose from.
pub fn all() -> Option<Arc<[Found]>> {
    STATE.read().all.clone()
}

/// How many times the grammars used changed: found, found again
/// differently, or picked otherwise. What was colored with others is
/// colored again when it changed (a preview's `Version`).
pub fn generation() -> u64 {
    STATE.read().generation
}

/// Find the grammars again where this system keeps them, keep them for
/// `all`, and pick each language's with the choices last given to `sync`:
/// a choice made while they are found is the one used. The grammars used.
/// A combined grammar whose check could not be made waits again.
pub fn find() -> Arc<[Found]> {
    let (mut all, folders, report) = find_all(&Roots::here(), &bat_file_types());
    // A combined grammar checked before with these very files is not
    // checked again: their keys read here, the lock not held.
    let keys = waiting_keys(&all);
    let mut state = STATE.write();
    let fits = state.fits.get_or_insert_with(HashMap::new);
    fitted(&mut all, &keys, fits, &HashMap::new());
    state.all = Some(all.into());
    state.folders = folders;
    state.report = Some(Arc::new(report));
    repick(&mut state)
}

/// What a combined grammar's files were when checked: each one's path,
/// identity and change time. The same files unchanged fit as they did.
type FitKey = Vec<(
    PathBuf,
    Option<skwir_core::FileId>,
    Option<std::time::SystemTime>,
)>;

fn fit_key(grammar: &Grammar) -> FitKey {
    grammar
        .files()
        .map(|file| {
            let meta = std::fs::metadata(file).ok();
            let identity = meta
                .as_ref()
                .and_then(|meta| skwir_core::fs::stat::identity(file, meta));
            let changed = meta
                .as_ref()
                .and_then(|meta| skwir_core::fs::stat::changed(file, meta));
            (file.clone(), identity, changed)
        })
        .collect()
}

/// The keys of `all`'s grammars waiting for their check, by place.
fn waiting_keys(all: &[Found]) -> Vec<(usize, FitKey)> {
    all.iter()
        .enumerate()
        .filter(|(_, found)| found.checking)
        .map(|(at, found)| (at, fit_key(&found.grammar)))
        .collect()
}

/// The checks `fits` remembers, and those that could not be made
/// (`failed`, this check's), applied to `all`'s grammars at `keys`: one
/// that fits is checked, named by the set of queries that colors it; one
/// refused cannot be used, saying why in words of its parts; one whose
/// check failed waits, saying why. Whether any changed.
fn fitted(
    all: &mut [Found],
    keys: &[(usize, FitKey)],
    fits: &HashMap<FitKey, Fit>,
    failed: &HashMap<FitKey, String>,
) -> bool {
    let mut any = false;
    for (at, key) in keys {
        let Some(found) = all.get_mut(*at) else {
            continue;
        };
        if let Some(fit) = fits.get(key) {
            found.checking = false;
            found.unchecked = None;
            match fit {
                Fit::Colors(set) => colored_by(found, *set),
                Fit::Library(why) => found.problem = Some(library_refused(found, why)),
                Fit::Queries(why) => found.problem = Some(queries_refused(found, why)),
            }
            any = true;
        } else if let Some(why) = failed.get(key) {
            found.unchecked = Some(home_in(why));
            any = true;
        }
    }
    any
}

/// Why a combined grammar's library cannot be used, the helper's reason
/// with its paths from `~/`: "Emacs's library cannot be used (its ABI is
/// 16, …)". Its queries are not the matter.
fn library_refused(found: &Found, why: &str) -> String {
    format!(
        "{} cannot be used ({})",
        found.source.library(),
        home_in(why)
    )
}

/// Why none of a combined grammar's queries colors its library, and what
/// makes them fit, by whose they are: a repository's is checked out at
/// the revision the library was built from; the system's are for its own
/// library, so the repository goes in Skwir's folder.
fn queries_refused(found: &Found, why: &str) -> String {
    let library = found.source.library();
    let why = home_in(why);
    match found.queries_from {
        Some(Source::Skwir | Source::TreeSitter) => format!(
            "its repository's queries do not fit {library} ({why}): check the repository out at the revision its library was built from"
        ),
        _ => format!(
            "the system's queries do not fit {library} ({why}): put the grammar's repository, at the revision its library was built from, in Skwir's grammars folder"
        ),
    }
}

/// `found` colored by the set of its queries at `set` of
/// `Grammar::query_sets`, as its check says: that set first, named by
/// where it came from, and those before it, whose highlights do not
/// compile against its library, left out. Its own first set: as it is.
fn colored_by(found: &mut Found, set: usize) {
    let Some(lent) = set.checked_sub(1) else {
        return;
    };
    if lent >= found.grammar.lent.len() || lent >= found.lent_from.len() {
        return;
    }
    let after = found.grammar.lent.split_off(lent + 1);
    let after_from = found.lent_from.split_off(lent + 1);
    let (Some(queries), Some((from, folder))) = (found.grammar.lent.pop(), found.lent_from.pop())
    else {
        return;
    };
    let grammar = &mut found.grammar;
    grammar.highlights = queries.highlights;
    grammar.injections = queries.injections;
    grammar.locals = queries.locals;
    grammar.folds = queries.folds;
    grammar.outline = queries.outline;
    grammar.lent = after;
    found.lent_from = after_from;
    let combined = from != found.source;
    found.queries_from = combined.then_some(from);
    found.queries_at = folder.filter(|_| combined);
}

/// Whether combined grammars wait for a check that may be asked now.
pub fn waiting() -> bool {
    STATE
        .read()
        .all
        .as_ref()
        .is_some_and(|all| all.iter().any(Found::waits))
}

/// A check of the combined grammars asked (`take_waiting`): the next may
/// be asked once this is dropped, whether its job ran, never started (its
/// token cancelled) or panicked.
pub struct Asked(());

impl Drop for Asked {
    fn drop(&mut self) {
        CHECK_ASKED.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// Whether combined grammars wait for their check and none is asked: the
/// one caller handed `Asked` asks it (`check_combined`), holding it until
/// it is done.
pub fn take_waiting() -> Option<Asked> {
    let asked = waiting()
        && CHECK_ASKED
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_ok();
    asked.then_some(Asked(()))
}

/// Check each combined grammar waiting (ADR-110, amended): its library
/// loaded and its queries compiled in a helper of its own
/// (`tree_sitter::checking`), each told under a name of its own so two
/// sources' grammars of a language are checked apart; what it answers
/// kept for the run, and applied. A worker's work, asked when the
/// Reading page's Languages is first shown or `ui.languages` is asked
/// off the window's thread; one check at a time, a second checking what
/// the first left. Whether any grammar changed.
pub fn check_combined(cancel: &skwir_core::CancelToken) -> bool {
    static ONE: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    let _one = ONE.lock();
    check_waiting(crate::tree_sitter::checking(), cancel)
}

fn check_waiting(helper: &crate::tree_sitter::Helper, cancel: &skwir_core::CancelToken) -> bool {
    let (waiting, folders) = {
        let state = STATE.read();
        let waiting: Vec<Found> = state
            .all
            .iter()
            .flat_map(|all| all.iter())
            .filter(|found| found.waits())
            .cloned()
            .collect();
        (waiting, state.folders.clone())
    };
    if waiting.is_empty() {
        return false;
    }
    // Each one's files as they are before the helper reads them: one
    // written again during the check is checked again, its answer never
    // kept under the new file's key.
    let keys: Vec<FitKey> = waiting
        .iter()
        .map(|found| fit_key(&found.grammar))
        .collect();
    let apart = |found: &Found| format!("{}@{}", found.grammar.name, found.source.key());
    let told: Vec<Grammar> = waiting
        .iter()
        .map(|found| Grammar {
            name: apart(found),
            ..found.grammar.clone()
        })
        .collect();
    // Told afresh: a helper still running from the last check keeps what
    // it loaded, and these files may have been written again since.
    helper.set_grammars_afresh(told.clone(), folders_read(&told, &folders));
    let mut fits = Vec::new();
    let mut failed = HashMap::new();
    for (found, key) in waiting.iter().zip(keys) {
        if cancel.is_cancelled() {
            break;
        }
        let checked = helper.check(&apart(found), cancel);
        if checked.is_err() && cancel.is_cancelled() {
            break;
        }
        match Answer::of(checked) {
            Answer::Fit(fit) => fits.push((key, fit)),
            Answer::Failed(why) => {
                failed.insert(key, why);
            }
        }
    }
    if fits.is_empty() && failed.is_empty() {
        return false;
    }
    let all = {
        let mut state = STATE.write();
        state.fits.get_or_insert_with(HashMap::new).extend(fits);
        state.all.clone()
    };
    let Some(all) = all else { return false };
    // Applied to the grammars as they stand: their keys read without the
    // lock; found again meanwhile, `find` applied what was kept, and what
    // failed is asked again.
    let keys = waiting_keys(&all);
    let mut state = STATE.write();
    if !state.all.as_ref().is_some_and(|now| Arc::ptr_eq(now, &all)) {
        return true;
    }
    let mut checked = all.to_vec();
    let fits = state.fits.get_or_insert_with(HashMap::new);
    if !fitted(&mut checked, &keys, fits, &failed) {
        return false;
    }
    state.all = Some(checked.into());
    repick(&mut state);
    true
}

/// Where the grammars were looked for when last found, and what was
/// nearly found: the Reading page's list under Languages.
pub fn report() -> Option<Arc<Report>> {
    STATE.read().report.clone()
}

/// The grammars used for these choices, by language in lower case (a
/// language's grammar from the source it names where that one is usable,
/// any other's the first usable): those kept when the choices are the
/// ones kept, else picked again now and told the helper every caller
/// shares (`tree_sitter::shared`: a grammar colors what it injects there
/// too). Telling it starts nothing: a helper told others is let go of,
/// and the next text opened starts one told these. Empty before the
/// grammars are found.
pub fn sync(chosen: &BTreeMap<String, Source>) -> Arc<[Found]> {
    {
        let state = STATE.read();
        if state.chosen == *chosen
            && let Some(found) = &state.found
        {
            return Arc::clone(found);
        }
        if state.all.is_none() && state.chosen == *chosen {
            return Arc::default();
        }
    }
    let mut state = STATE.write();
    if state.chosen != *chosen {
        state.chosen.clone_from(chosen);
        if state.all.is_some() {
            return repick(&mut state);
        }
    }
    state.found.clone().unwrap_or_default()
}

/// Pick the grammars used from those found as the choices kept say, and
/// tell the shared helper, under the lock: two picks never tell it out of
/// order. Telling it only compares and swaps what it keeps.
fn repick(state: &mut State) -> Arc<[Found]> {
    let all = state.all.clone().unwrap_or_default();
    let found: Arc<[Found]> = pick(&all, &state.chosen).into();
    // Never found and found nothing are alike: nothing is colored again;
    // nor when only what the Reading page says changed (a check answered
    // that a grammar coloring meanwhile fits).
    if !colors_alike(state.found.as_deref().unwrap_or_default(), &found) {
        state.generation += 1;
    }
    state.found = Some(Arc::clone(&found));
    let told: Vec<Grammar> = found
        .iter()
        .filter(|found| found.problem.is_none())
        .map(|found| found.grammar.clone())
        .collect();
    let folders = folders_read(&told, &state.folders);
    crate::tree_sitter::shared().set_grammars_reading(told, folders);
    found
}

/// Whether two picks color texts alike: the same grammars, from the same
/// sources, for the same files, each usable or not. What only the Reading
/// page says (a check's state, a reason's words) colors nothing again.
fn colors_alike(before: &[Found], now: &[Found]) -> bool {
    fn colors(found: &Found) -> (&Grammar, Source, &[String], Option<&str>, bool) {
        (
            &found.grammar,
            found.source,
            &found.file_types,
            found.first_line.as_deref(),
            found.problem.is_none(),
        )
    }
    before.len() == now.len() && before.iter().zip(now).all(|(a, b)| colors(a) == colors(b))
}

/// The folders a helper told `grammars` reads: of `folders` (where the
/// grammars found lie), those holding one of their files, and the folder
/// of a file that lies in none of them (a link an editor's or the
/// system's folder holds, `confine`).
fn folders_read(grammars: &[Grammar], folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut read: Vec<PathBuf> = Vec::new();
    let mut seen = HashSet::new();
    for file in grammars.iter().flat_map(Grammar::files) {
        let folder = folders
            .iter()
            .find(|folder| file.starts_with(folder))
            .cloned()
            .or_else(|| file.parent().map(Path::to_owned));
        if let Some(folder) = folder
            && seen.insert(folder.clone())
        {
            read.push(folder);
        }
    }
    read
}

/// One grammar per language of `all` (by language, sources in order): the
/// source `chosen` names for it where usable, else the first usable, else
/// the first, which says why it cannot be.
fn pick(all: &[Found], chosen: &BTreeMap<String, Source>) -> Vec<Found> {
    let mut picked = Vec::new();
    let mut at = 0;
    while at < all.len() {
        let language = key_of(&all[at].grammar.name);
        let end = all[at..]
            .iter()
            .position(|found| key_of(&found.grammar.name) != language)
            .map_or(all.len(), |n| at + n);
        let alternatives = &all[at..end];
        let usable = |found: &&Found| found.problem.is_none();
        let wanted = chosen.get(&language).and_then(|source| {
            alternatives
                .iter()
                .filter(usable)
                .find(|found| found.source == *source)
        });
        if let Some(found) = wanted
            .or_else(|| alternatives.iter().find(usable))
            .or_else(|| alternatives.first())
        {
            picked.push(found.clone());
        }
        at = end;
    }
    picked
}

/// The grammar that colors a file named `name`: by its whole name, then
/// its extensions, longest first (`d.ts` before `ts`), then its first
/// line when its name says nothing. Usable ones only.
pub fn for_file<'a>(found: &'a [Found], name: &str, first_line: Option<&str>) -> Option<&'a Found> {
    matching(found, name, first_line, true)
}

/// `for_file` among every grammar of `found`, those that cannot be used
/// too: the language a file is of, whether a grammar of it colors or not
/// (the key a language's choice is kept by).
pub fn for_file_any<'a>(
    found: &'a [Found],
    name: &str,
    first_line: Option<&str>,
) -> Option<&'a Found> {
    matching(found, name, first_line, false)
}

/// `for_file`'s rules, among usable grammars only or every one.
fn matching<'a>(
    found: &'a [Found],
    name: &str,
    first_line: Option<&str>,
    usable_only: bool,
) -> Option<&'a Found> {
    let usable = || {
        found
            .iter()
            .filter(move |found| !usable_only || found.problem.is_none())
    };
    let lower = name.to_lowercase();
    if let Some(found) =
        usable().find(|found| found.file_types.iter().any(|t| t.to_lowercase() == lower))
    {
        return Some(found);
    }
    let extensions = lower
        .match_indices('.')
        .map(|(at, _)| &lower[at + 1..])
        .filter(|extension| !extension.is_empty());
    for extension in extensions {
        if let Some(found) = usable().find(|found| {
            found
                .file_types
                .iter()
                .any(|t| t.trim_start_matches('.').to_lowercase() == extension)
        }) {
            return Some(found);
        }
    }
    if name.contains('.') {
        return None;
    }
    let line = first_line?;
    usable().find(|found| {
        found
            .first_line
            .as_deref()
            .and_then(|pattern| regex::Regex::new(pattern).ok())
            .is_some_and(|pattern| pattern.is_match(line))
    })
}

/// The grammar named `name` (as injections and the helper name it).
pub fn named<'a>(found: &'a [Found], name: &str) -> Option<&'a Found> {
    let key = key_of(name);
    found
        .iter()
        .find(|found| key_of(&found.grammar.name) == key)
}

/// What bat's grammars say each language colors, by its name in lower
/// case: the files a grammar found without file types borrows. Read from
/// what bat's set holds as kept (`added::table_now`), never the set: a
/// start finds grammars without reading bat's.
fn bat_file_types() -> HashMap<String, Vec<String>> {
    crate::highlight::added::table_now().file_types()
}

impl Roots {
    /// Where this system keeps them.
    pub fn here() -> Roots {
        let config = editor_config();
        let home = home();
        let packaged = Packaged::here();
        let mut roots = Roots {
            skwir: folder(),
            ..Roots::default()
        };
        // The CLI's own: `$TREE_SITTER_DIR`, else its configuration folder
        // (macOS's Application Support before 0.23, `~/.tree-sitter` before
        // that), the first that holds one.
        roots.tree_sitter_config = if let Some(dir) = std::env::var_os("TREE_SITTER_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
        {
            Some(dir.join("config.json"))
        } else {
            let places: Vec<PathBuf> = config
                .iter()
                .map(|dir| dir.join("tree-sitter"))
                .chain(legacy_config().map(|dir| dir.join("tree-sitter")))
                .chain(home.iter().map(|home| home.join(".tree-sitter")))
                .map(|dir| dir.join("config.json"))
                .collect();
            places
                .iter()
                .find(|path| path.is_file())
                .or(places.first())
                .cloned()
        };
        roots.tree_sitter_libraries = std::env::var_os("TREE_SITTER_LIBDIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| editor_cache().map(|dir| dir.join("tree-sitter/lib")));
        roots.zed = zed_folders(home.as_deref());
        roots.helix_languages = config.as_ref().map(|dir| dir.join("helix/languages.toml"));
        // In Helix's order: the user's runtime, `$HELIX_RUNTIME`, then the
        // one installed with it (beside `hx`, in its prefix's `lib` or
        // `libexec`, Homebrew's), then the distributions' folders.
        let installed = |name: &str| -> Vec<PathBuf> {
            program(name)
                .and_then(|exe| exe.parent().map(Path::to_owned))
                .map(|bin| {
                    let mut runtimes = vec![bin.join("runtime")];
                    if let Some(prefix) = bin.parent() {
                        runtimes.push(prefix.join("lib/runtime"));
                        runtimes.push(prefix.join("libexec/runtime"));
                        runtimes.push(prefix.join("share/helix/runtime"));
                    }
                    runtimes
                })
                .unwrap_or_default()
        };
        roots.helix = config
            .iter()
            .map(|dir| dir.join("helix/runtime"))
            .chain(
                std::env::var_os("HELIX_RUNTIME")
                    .map(PathBuf::from)
                    .filter(|dir| dir.is_absolute()),
            )
            .chain(installed("hx"))
            .chain(installed("helix"))
            .chain(packaged.helix)
            .collect();
        let nvim = program("nvim").and_then(|exe| Some(exe.parent()?.parent()?.to_owned()));
        let nvim_data = neovim_data();
        roots.neovim_parsers = nvim_data
            .iter()
            .flat_map(|data| {
                let lazy = data.join("lazy/nvim-treesitter");
                [data.join("site/parser"), lazy.join("parser")]
            })
            .chain(nvim.iter().map(|prefix| prefix.join("lib/nvim/parser")))
            .chain(packaged.neovim_parsers)
            .collect();
        roots.neovim_queries = nvim_data
            .iter()
            .flat_map(|data| {
                let lazy = data.join("lazy/nvim-treesitter");
                [
                    data.join("site/queries"),
                    lazy.join("queries"),
                    lazy.join("runtime/queries"),
                ]
            })
            .chain(
                nvim.iter()
                    .map(|prefix| prefix.join("share/nvim/runtime/queries")),
            )
            .chain(packaged.neovim_queries)
            .collect();
        roots.system_parsers = packaged.system_parsers;
        roots.system_queries = packaged.system_queries;
        roots.system_libraries = packaged.system_libraries;
        roots.emacs = home
            .iter()
            .map(|home| home.join(".emacs.d/tree-sitter"))
            .chain(legacy_config().map(|dir| dir.join(".emacs.d/tree-sitter")))
            .chain(config.iter().map(|dir| dir.join("emacs/tree-sitter")))
            .collect();
        roots
    }
}

/// The folders a system's packages keep grammars and editors' runtimes
/// in, beside those found from where a program is (`Roots::here`).
#[derive(Default)]
struct Packaged {
    helix: Vec<PathBuf>,
    neovim_parsers: Vec<PathBuf>,
    neovim_queries: Vec<PathBuf>,
    system_parsers: Vec<PathBuf>,
    system_queries: Vec<PathBuf>,
    system_libraries: Vec<PathBuf>,
}

impl Packaged {
    /// The distributions' folders on Linux and the BSDs: Arch's
    /// `/usr/lib/tree_sitter` and `libtree-sitter-<lang>` libraries, Debian's
    /// multiarch ones, each editor's runtime as packaged.
    #[cfg(not(any(target_os = "macos", windows)))]
    fn here() -> Packaged {
        let paths = |paths: &[&str]| paths.iter().map(PathBuf::from).collect();
        Packaged {
            helix: paths(&[
                "/usr/lib/helix/runtime",
                "/usr/share/helix/runtime",
                "/usr/lib/hx/runtime",
                "/usr/local/lib/helix/runtime",
            ]),
            neovim_parsers: paths(&[
                "/usr/lib/nvim/parser",
                "/usr/lib64/nvim/parser",
                "/usr/local/lib/nvim/parser",
            ]),
            neovim_queries: paths(&["/usr/share/nvim/runtime/queries"]),
            system_parsers: paths(&[
                "/usr/lib/tree_sitter",
                "/usr/lib64/tree_sitter",
                "/usr/local/lib/tree_sitter",
            ]),
            system_queries: paths(&[
                "/usr/share/tree-sitter/queries",
                "/usr/local/share/tree-sitter/queries",
            ]),
            system_libraries: paths(&[
                "/usr/lib",
                "/usr/lib64",
                "/usr/local/lib",
                "/usr/lib/x86_64-linux-gnu",
                "/usr/lib/aarch64-linux-gnu",
            ]),
        }
    }

    /// Homebrew's on macOS (Apple silicon's prefix, then Intel's): its
    /// libraries and their queries. Helix and Neovim are found from their
    /// program's prefix, the system keeps no grammar of its own.
    #[cfg(target_os = "macos")]
    fn here() -> Packaged {
        let paths = |paths: &[&str]| paths.iter().map(PathBuf::from).collect();
        Packaged {
            system_queries: paths(&[
                "/opt/homebrew/share/tree-sitter/queries",
                "/usr/local/share/tree-sitter/queries",
            ]),
            system_libraries: paths(&["/opt/homebrew/lib", "/usr/local/lib"]),
            ..Packaged::default()
        }
    }

    /// None on Windows: each program keeps its grammars in its own folders,
    /// found from the user's (`Roots::here`) and its program's.
    #[cfg(windows)]
    fn here() -> Packaged {
        Packaged::default()
    }
}

/// Skwir's grammars folder: grammars' repositories, built or not (an
/// unbuilt one lends its queries to a library found elsewhere), and
/// `languages.toml`.
pub fn folder() -> Option<PathBuf> {
    skwir_core::folders::data().map(|dir| dir.join("grammars"))
}

/// The home folder, as the rest of Skwir reads it.
fn home() -> Option<PathBuf> {
    skwir_core::os_path::home()
}

/// A text the helper wrote (naming files by their whole path), its paths
/// from `~` (`os_path::from_home_in`).
fn home_in(text: &str) -> String {
    skwir_core::os_path::from_home_in(text)
}

/// The configuration folder Helix, the tree-sitter CLI and Emacs use: XDG's
/// on Linux, the BSDs and macOS (`~/.config`, as they ask `etcetera`), the
/// roaming `AppData` on Windows.
fn editor_config() -> Option<PathBuf> {
    #[cfg(windows)]
    return dirs::config_dir();
    #[cfg(not(windows))]
    return skwir_core::xdg::config_home();
}

/// The folder the tree-sitter CLI keeps the libraries it builds under:
/// XDG's cache on Linux and macOS, the local `AppData` on Windows.
fn editor_cache() -> Option<PathBuf> {
    #[cfg(windows)]
    return dirs::cache_dir();
    #[cfg(not(windows))]
    return skwir_core::xdg::cache_home();
}

/// The system's own configuration folder where it is not XDG's: macOS's
/// Application Support (the CLI's before 0.23), Windows' roaming `AppData`
/// (Emacs's home without `HOME`).
fn legacy_config() -> Option<PathBuf> {
    #[cfg(any(target_os = "macos", windows))]
    return dirs::config_dir();
    #[cfg(not(any(target_os = "macos", windows)))]
    return None;
}

/// Neovim's data folder (`stdpath('data')`): `nvim` in XDG's data folder,
/// `nvim-data` in the local `AppData` on Windows.
fn neovim_data() -> Option<PathBuf> {
    #[cfg(windows)]
    return dirs::data_local_dir().map(|dir| dir.join("nvim-data"));
    #[cfg(not(windows))]
    return skwir_core::xdg::data_home().map(|dir| dir.join("nvim"));
}

/// Zed's installed extensions: in its data folder, its Flatpak's on Linux.
fn zed_folders(home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    #[cfg(target_os = "macos")]
    folders.extend(dirs::config_dir().map(|dir| dir.join("Zed/extensions/installed")));
    #[cfg(windows)]
    folders.extend(dirs::data_local_dir().map(|dir| dir.join("Zed/extensions/installed")));
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        folders
            .extend(skwir_core::xdg::data_home().map(|dir| dir.join("zed/extensions/installed")));
        folders.extend(
            home.map(|home| home.join(".var/app/dev.zed.Zed/data/zed/extensions/installed")),
        );
    }
    let _ = home;
    folders
}

/// A program on `PATH`, its links followed (a Nix profile's leads into its
/// store path, Homebrew's into its cellar): `name`, `name.exe` on Windows.
fn program(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let file = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    std::env::split_paths(&path)
        .map(|dir| dir.join(&file))
        .find(|path| path.is_file())
        .and_then(|path| canonical(&path))
}

/// A library and its queries as one source holds them, before they are
/// merged by language.
#[derive(Clone, Debug, Default)]
struct Candidate {
    source: Option<Source>,
    name: String,
    /// The folder it was found as: a repository's own, a language's
    /// queries folder; what a grammar its queries color names them by.
    home: Option<PathBuf>,
    library: Option<PathBuf>,
    symbol: Option<String>,
    queries: Queries,
    file_types: Vec<String>,
    first_line: Option<String>,
    injection_regex: Option<String>,
    problem: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Queries {
    highlights: Vec<PathBuf>,
    injections: Vec<PathBuf>,
    locals: Vec<PathBuf>,
    folds: Vec<PathBuf>,
    outline: Vec<PathBuf>,
}

impl Queries {
    fn is_empty(&self) -> bool {
        self.highlights.is_empty()
    }

    fn kinds_mut(&mut self) -> [&mut Vec<PathBuf>; 5] {
        [
            &mut self.highlights,
            &mut self.injections,
            &mut self.locals,
            &mut self.folds,
            &mut self.outline,
        ]
    }
}

impl From<Queries> for syntax::Queries {
    fn from(queries: Queries) -> Self {
        syntax::Queries {
            highlights: queries.highlights,
            injections: queries.injections,
            locals: queries.locals,
            folds: queries.folds,
            outline: queries.outline,
        }
    }
}

/// The grammars found in `roots`, merged by language, in the order of
/// their names, and the folders their files lie in, as they really are
/// (`confine`). `bat` names what bat's grammars color, by language.
#[cfg(test)]
fn find_in(roots: &Roots, bat: &HashMap<String, Vec<String>>) -> (Vec<Found>, Vec<PathBuf>) {
    let (all, folders, _) = find_all(roots, bat);
    (all, folders)
}

/// `find_in`, and the report of where they were looked for.
fn find_all(
    roots: &Roots,
    bat: &HashMap<String, Vec<String>>,
) -> (Vec<Found>, Vec<PathBuf>, Report) {
    let mut confined = Confined::default();
    let mut candidates = Vec::new();
    if let Some(dir) = &roots.skwir {
        for repository in sorted(dir) {
            candidates.extend(confined.keep(
                repository_grammars(Source::Skwir, &repository, None),
                &[repository.as_path()],
                Kept::Inside,
            ));
        }
    }
    if let Some(config) = &roots.tree_sitter_config {
        let libraries = roots.tree_sitter_libraries.as_deref();
        for dir in parser_directories(config) {
            for repository in sorted(&dir) {
                let folders: Vec<&Path> = std::iter::once(repository.as_path())
                    .chain(libraries)
                    .collect();
                candidates.extend(confined.keep(
                    repository_grammars(Source::TreeSitter, &repository, libraries),
                    &folders,
                    Kept::Inside,
                ));
            }
        }
    }
    for dir in &roots.zed {
        for extension in sorted(dir) {
            candidates.extend(confined.keep(
                zed_grammars(&extension),
                &[extension.as_path()],
                Kept::Inside,
            ));
        }
    }
    candidates.extend(confined.keep(
        helix_grammars(roots),
        &listed(&[&roots.helix]),
        Kept::Listed,
    ));
    candidates.extend(confined.keep(
        parser_folder_grammars(Source::Neovim, &roots.neovim_parsers, &roots.neovim_queries),
        &listed(&[&roots.neovim_parsers, &roots.neovim_queries]),
        Kept::Listed,
    ));
    candidates.extend(confined.keep(
        parser_folder_grammars(Source::System, &roots.system_parsers, &roots.system_queries),
        &listed(&[&roots.system_parsers, &roots.system_queries]),
        Kept::Listed,
    ));
    candidates.extend(confined.keep(
        library_folder_grammars(
            Source::System,
            &roots.system_libraries,
            &roots.system_queries,
        ),
        &listed(&[&roots.system_libraries, &roots.system_queries]),
        Kept::Listed,
    ));
    candidates.extend(confined.keep(
        library_folder_grammars(Source::Emacs, &roots.emacs, &[]),
        &listed(&[&roots.emacs]),
        Kept::Listed,
    ));
    candidates.truncate(MOST);
    let mapped = roots
        .skwir
        .as_ref()
        .map(|dir| languages_toml(&dir.join("languages.toml")))
        .unwrap_or_default();
    let all = merge(candidates, &mapped, bat);
    let report = report_of(roots, &confined.held, &all);
    (all, confined.folders, report)
}

/// Where grammars were looked for (ADR-110, amended): what was nearly
/// found, with what to do, and each folder read with what it held.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub nearly: Vec<Nearly>,
    pub looked: Vec<Looked>,
}

/// A folder read for grammars, and the languages it held a part or the
/// whole of; one that is not there held nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Looked {
    pub source: Source,
    pub folder: PathBuf,
    pub there: bool,
    pub languages: Vec<String>,
}

/// What was nearly found: the file or folder it is about, and what is
/// wrong with it and what to do, said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Nearly {
    pub source: Source,
    pub what: PathBuf,
    pub said: String,
}

/// Every folder each source is read in, in the sources' order: the CLI's
/// parser directories its configuration names and its library cache.
/// A folder that is another one of its source (`/usr/lib64`, a link to
/// `/usr/lib`) is that one.
fn folders_looked(roots: &Roots) -> Vec<(Source, PathBuf)> {
    let mut looked: Vec<(Source, PathBuf)> = Vec::new();
    let mut seen: HashSet<(Source, PathBuf)> = HashSet::new();
    let mut add = |source: Source, folders: &[PathBuf]| {
        for folder in folders {
            let real = canonical(folder).unwrap_or_else(|| folder.clone());
            if seen.insert((source, real)) {
                looked.push((source, folder.clone()));
            }
        }
    };
    add(Source::Skwir, roots.skwir.as_slice());
    if let Some(config) = &roots.tree_sitter_config {
        add(Source::TreeSitter, &parser_directories(config));
        add(Source::TreeSitter, roots.tree_sitter_libraries.as_slice());
    }
    add(Source::Zed, &roots.zed);
    add(Source::Helix, &roots.helix);
    add(Source::Neovim, &roots.neovim_parsers);
    add(Source::Neovim, &roots.neovim_queries);
    add(Source::System, &roots.system_parsers);
    add(Source::System, &roots.system_queries);
    add(Source::System, &roots.system_libraries);
    add(Source::Emacs, &roots.emacs);
    looked
}

/// The report of where grammars were looked for in `roots`, of the
/// candidates `held` as found and the grammars they made, `all`.
fn report_of(roots: &Roots, held: &[Held], all: &[Found]) -> Report {
    let mut nearly = Vec::new();
    // The CLI's configuration, the folders it names, and a repository a
    // folder too deep in one (or in Skwir's folder).
    if let Some(config) = &roots.tree_sitter_config
        && config.is_file()
        && let Err(error) = read_text(config)
            .ok_or_else(|| "cannot be read".to_owned())
            .and_then(|text| {
                serde_json::from_str::<serde_json::Value>(&text).map_err(|e| e.to_string())
            })
    {
        nearly.push(Nearly {
            source: Source::TreeSitter,
            what: config.clone(),
            said: format!(
                "its configuration does not read ({error}): none of its parser directories is read"
            ),
        });
    }
    let parser_dirs = roots
        .tree_sitter_config
        .as_deref()
        .map(parser_directories)
        .unwrap_or_default();
    for dir in &parser_dirs {
        if !dir.is_dir() {
            nearly.push(Nearly {
                source: Source::TreeSitter,
                what: dir.clone(),
                said: "a parser directory its configuration names, not there".to_owned(),
            });
        }
    }
    let nested = parser_dirs
        .iter()
        .map(|dir| (Source::TreeSitter, dir))
        .chain(roots.skwir.iter().map(|dir| (Source::Skwir, dir)));
    for (source, dir) in nested {
        for (folder, repository) in nested_repositories(dir) {
            let said = match source {
                Source::Skwir => format!(
                    "one folder too deep: Skwir reads only the folders directly in it ({}); move or link the repository there",
                    shown(dir)
                ),
                _ => format!(
                    "one folder too deep: it reads only the folders directly in {}; add {} to its parser-directories, or link the repository into Skwir's grammars folder",
                    shown(dir),
                    shown(&folder)
                ),
            };
            nearly.push(Nearly {
                source,
                what: repository,
                said,
            });
        }
    }
    // A grammar found that cannot be used: a library with nothing to
    // color with, a repository put in Skwir's or the CLI's folders with
    // no library anywhere, a library outside its folder. An editor's
    // queries of a language it has no parser of (Helix's `markup`,
    // Neovim's queries of parsers not installed) are its own business:
    // never listed (`merge`), so never said.
    let mut held_as: HashMap<(Source, String), &Held> = HashMap::new();
    for held in held {
        held_as
            .entry((held.source, key_of(&held.name)))
            .or_insert(held);
    }
    for found in all.iter().filter(|found| found.problem.is_some()) {
        let what = Some(found.grammar.library.clone())
            .filter(|library| !library.as_os_str().is_empty())
            .or_else(|| {
                held_as
                    .get(&(found.source, key_of(&found.grammar.name)))
                    .and_then(|held| held.home.clone().or_else(|| held.at.first().cloned()))
            })
            .unwrap_or_default();
        nearly.push(Nearly {
            source: found.source,
            what,
            said: found.problem.clone().unwrap_or_default(),
        });
    }
    let folders = folders_looked(roots);
    // Each file is the deepest folder's of its source that holds it: the
    // system's `/usr/lib/tree_sitter` gives its own, not `/usr/lib`.
    let mut languages: Vec<Vec<String>> = vec![Vec::new(); folders.len()];
    for held in held {
        for at in &held.at {
            let deepest = folders
                .iter()
                .enumerate()
                .filter(|(_, (source, folder))| *source == held.source && at.starts_with(folder))
                .max_by_key(|(_, (_, folder))| folder.components().count());
            if let Some((index, _)) = deepest {
                languages[index].push(held.name.clone());
            }
        }
    }
    let looked = folders
        .into_iter()
        .zip(languages)
        .map(|((source, folder), mut languages)| {
            languages.sort();
            languages.dedup();
            Looked {
                source,
                there: folder.is_dir(),
                folder,
                languages,
            }
        })
        .collect();
    Report { nearly, looked }
}

/// The repositories one folder too deep in `dir`: in a folder of it that
/// is not one itself, named `tree-sitter-*` as grammars' repositories are,
/// each with that folder. Bounded, as finding is.
fn nested_repositories(dir: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut nested = Vec::new();
    for folder in sorted(dir).into_iter().take(MOST) {
        if !folder.is_dir() || is_repository(&folder) {
            continue;
        }
        for inner in sorted(&folder) {
            let named = inner
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("tree-sitter-"));
            if named && is_repository(&inner) {
                nested.push((folder.clone(), inner));
            }
        }
        if nested.len() >= 64 {
            break;
        }
    }
    nested
}

/// Whether `dir` is a grammar's repository: what `tree-sitter init` makes
/// (`tree-sitter.json`), else a grammar's source (`grammar.js`).
fn is_repository(dir: &Path) -> bool {
    dir.join("tree-sitter.json").is_file() || dir.join("grammar.js").is_file()
}

/// `path` as a reason says it: from `~` where it is under the home
/// folder (`os_path::from_home`, as the settings say paths).
fn shown(path: &Path) -> String {
    skwir_core::os_path::from_home(path)
}

/// Every folder of `folders`, as one list.
fn listed<'a>(folders: &[&'a [PathBuf]]) -> Vec<&'a Path> {
    folders
        .iter()
        .flat_map(|folders| folders.iter().map(PathBuf::as_path))
        .collect()
}

/// How a source's files are kept to its folders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kept {
    /// Named by a description (a repository's `tree-sitter.json`, an
    /// extension's `config.toml`), which may say any path and hold links:
    /// each file really lies in the source's folders.
    Inside,
    /// Listed in an editor's or the system's folders, which its package
    /// manager fills: a link there is followed wherever it leads (NixOS's
    /// into its store, Arch's Neovim parsers into `/usr/lib/tree_sitter`),
    /// but a name a file gives (`; inherits: ../x`) never leaves them.
    Listed,
}

/// The folders grammars were found in, as they really are: what a helper
/// told them reads, and nothing else (`tree_sitter::Helper`, ADR-110);
/// and what each candidate was, as found, before its files are named by
/// their real paths: what the folders looked in held (`Report`).
#[derive(Default)]
struct Confined {
    folders: Vec<PathBuf>,
    held: Vec<Held>,
}

/// A candidate as found: its source, its language, its files where it was
/// found (a link an editor's folder holds, not where it leads), and the
/// folder it was found as.
struct Held {
    source: Source,
    name: String,
    at: Vec<PathBuf>,
    home: Option<PathBuf>,
}

impl Confined {
    /// `candidates` with each file named by its real path, and those that
    /// do not lie in `folders` (as `kept` says) left out: a library left
    /// out makes its grammar unusable, said in its problem. A file in the
    /// system's folders, which an isolated helper reads anyway, is kept.
    fn keep(
        &mut self,
        candidates: Vec<Candidate>,
        folders: &[&Path],
        kept: Kept,
    ) -> Vec<Candidate> {
        if candidates.is_empty() {
            return candidates;
        }
        self.held.extend(candidates.iter().map(|candidate| {
            Held {
                source: candidate.source.unwrap_or(Source::Skwir),
                name: candidate.name.clone(),
                at: candidate
                    .library
                    .iter()
                    .chain(&candidate.home)
                    .chain(candidate.queries.highlights.last())
                    .cloned()
                    .collect(),
                home: candidate.home.clone(),
            }
        }));
        let home = home().and_then(|home| canonical(&home));
        let roots: Vec<(&Path, PathBuf)> = folders
            .iter()
            .filter_map(|&found| Some((found, canonical(found)?)))
            // A folder holding the home folder would hand the helper all of
            // it (a repository linked to `~` or `/`).
            .filter(|(_, real)| !home.as_ref().is_some_and(|home| home.starts_with(real)))
            .collect();
        for (_, real) in &roots {
            if !self.folders.contains(real) {
                self.folders.push(real.clone());
            }
        }
        let inside = |file: &Path| -> Option<PathBuf> {
            let real = canonical(file)?;
            let kept_here = roots.iter().any(|(found, root)| {
                real.starts_with(root)
                    || (kept == Kept::Listed
                        && file.starts_with(found)
                        && file.components().all(|part| {
                            !matches!(
                                part,
                                std::path::Component::ParentDir | std::path::Component::CurDir
                            )
                        }))
            });
            (kept_here || crate::sandbox::reads_anyway(&real)).then_some(real)
        };
        candidates
            .into_iter()
            .map(|mut candidate| {
                if let Some(library) = candidate.library.take() {
                    match inside(&library) {
                        Some(real) => candidate.library = Some(real),
                        None => {
                            candidate.problem.get_or_insert_with(|| {
                                format!("its library {} lies outside its folder", library.display())
                            });
                        }
                    }
                }
                for files in candidate.queries.kinds_mut() {
                    *files = files.iter().filter_map(|file| inside(file)).collect();
                }
                candidate
            })
            .collect()
    }
}

/// `path` as it really is, its links followed (`os_path::canonical`);
/// `None` when it is not there.
fn canonical(path: &Path) -> Option<PathBuf> {
    skwir_core::os_path::canonical(path).ok()
}

/// Each source's grammar of each language, by language, sources in order
/// (the first candidate of each source): queries its own, else borrowed;
/// file types as the module says, the same for every source's.
fn merge(
    candidates: Vec<Candidate>,
    mapped: &HashMap<String, Vec<String>>,
    bat: &HashMap<String, Vec<String>>,
) -> Vec<Found> {
    // A grammar is known by the symbol its library exports, `-` and `_`
    // alike: Emacs's `c-sharp` and Neovim's `c_sharp` are one language.
    let mapped: HashMap<String, &Vec<String>> = mapped
        .iter()
        .map(|(name, types)| (key_of(name), types))
        .collect();
    let bat: HashMap<String, &Vec<String>> = bat
        .iter()
        .map(|(name, types)| (key_of(name), types))
        .collect();
    let mut names: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, Vec<Candidate>> = HashMap::new();
    for candidate in candidates {
        let key = key_of(&candidate.name);
        if !by_name.contains_key(&key) {
            names.push(key.clone());
        }
        by_name.entry(key).or_default().push(candidate);
    }
    names.sort();
    let mut found = Vec::new();
    for key in names {
        let candidates = &by_name[&key];
        // What a language colors, the same whoever's grammar colors it;
        // its name, the first's (each library keeps its own symbol).
        let first = candidates
            .iter()
            .find(|c| c.library.is_some())
            .or_else(|| candidates.first());
        let name = first.map_or_else(|| key.clone(), |first| first.name.clone());
        let file_types = mapped
            .get(&key)
            .map(|types| (*types).clone())
            .filter(|types| !types.is_empty())
            .or_else(|| {
                first
                    .map(|own| own.file_types.clone())
                    .filter(|types| !types.is_empty())
            })
            .or_else(|| {
                candidates
                    .iter()
                    .map(|c| c.file_types.clone())
                    .find(|types| !types.is_empty())
            })
            .or_else(|| {
                bat.get(&key)
                    .map(|types| (*types).clone())
                    .filter(|types| !types.is_empty())
            })
            .unwrap_or_else(|| vec![name.clone()]);
        let mut sources = HashSet::new();
        let with_library: HashSet<Source> = candidates
            .iter()
            .filter(|c| c.library.is_some())
            .map(|c| c.source.unwrap_or(Source::Skwir))
            .collect();
        for own in candidates {
            // One grammar a source: its first. Queries alone are a part,
            // never listed (ADR-110, amended): lent where they may be,
            // and said only of a repository the user put in Skwir's or
            // the CLI's folders when no source has a library of it, to
            // say it is not built. A library left out (it lies outside
            // its folder) is said where its source has no other. A
            // source's candidate without a library never hides one of the
            // same source that has one.
            let source = own.source.unwrap_or(Source::Skwir);
            let first_of_all = first.is_some_and(|first| std::ptr::eq(first, own));
            let repository = matches!(source, Source::Skwir | Source::TreeSitter);
            let left_out = own.problem.is_some() && !with_library.contains(&source);
            let said = own.library.is_some() || (first_of_all && repository) || left_out;
            if !said || !sources.insert(source) {
                continue;
            }
            found.push(alternative(own, candidates, &file_types, &name));
        }
    }
    found
}

/// `own`, a source's grammar of a language whose grammars are
/// `candidates`, as found, named `name`: whole, its own queries; a part
/// (a library without queries), those its grammar's own repository or the
/// system lends; its first line and injections, else another's; what
/// makes it unusable.
fn alternative(
    own: &Candidate,
    candidates: &[Candidate],
    file_types: &[String],
    name: &str,
) -> Found {
    let source = own.source.unwrap_or(Source::Skwir);
    // Whole, it colors with its own queries alone. A library without
    // queries takes those of its grammar's own repository (Skwir's folder,
    // the CLI's) and the system's, by rank: the helper tries them in turn
    // where one does not compile, and its check says which colors it
    // (`colored_by`). An editor's are written for its own revision of the
    // grammar and its own conditions: never lent (ADR-110, amended).
    let sets: Vec<&Candidate> = if own.queries.is_empty() {
        let mut lenders: Vec<&Candidate> = candidates
            .iter()
            .filter(|c| {
                !c.queries.is_empty()
                    && !std::ptr::eq(*c, own)
                    && c.source.unwrap_or(Source::Skwir).lends()
            })
            .collect();
        lenders.sort_by_key(|c| lender_rank(c.source.unwrap_or(Source::Skwir)));
        // The same queries found twice (the system's, through its parser
        // and its library folders) are one set.
        let mut seen: Vec<&Queries> = Vec::new();
        lenders.retain(|c| {
            let new = !seen.contains(&&c.queries);
            seen.push(&c.queries);
            new
        });
        lenders
    } else {
        vec![own]
    };
    // An editor that holds queries of it, not lent: said.
    let unlent = candidates
        .iter()
        .filter(|c| !c.queries.is_empty() && !std::ptr::eq(*c, own))
        .filter_map(|c| c.source)
        .find(|source| !source.lends());
    let first = sets.first().copied();
    let queries = first.map(|set| set.queries.clone()).unwrap_or_default();
    // Lent: by whom, and from which folder (its home, else the folder of
    // its highlights).
    let folder_of = |set: &Candidate| {
        set.home.clone().or_else(|| {
            set.queries
                .highlights
                .last()
                .and_then(|file| file.parent().map(Path::to_owned))
        })
    };
    let lender = first.filter(|set| !std::ptr::eq(*set, own));
    let queries_from = lender.and_then(|lender| lender.source);
    let combined = queries_from.is_some_and(|from| from != source);
    // The sets tried after the first: only a combined grammar's, each
    // that lends; one source's library and queries are one grammar.
    let after: &[&Candidate] = if combined { &sets[1..] } else { &[] };
    let lent: Vec<syntax::Queries> = after.iter().map(|set| set.queries.clone().into()).collect();
    let lent_from = after
        .iter()
        .map(|set| (set.source.unwrap_or(Source::Skwir), folder_of(set)))
        .collect();
    let first_line = own
        .first_line
        .clone()
        .or_else(|| candidates.iter().find_map(|c| c.first_line.clone()));
    let injection_regex = own
        .injection_regex
        .clone()
        .or_else(|| candidates.iter().find_map(|c| c.injection_regex.clone()));
    let problem = own
        .problem
        .clone()
        .or_else(|| match (&own.library, queries.is_empty()) {
            (None, _) => Some("no library: build the grammar (`tree-sitter build`)".to_owned()),
            (Some(_), true) => Some(no_highlights(source, unlent)),
            (Some(_), false) => None,
        });
    Found {
        grammar: Grammar {
            name: name.to_owned(),
            library: own.library.clone().unwrap_or_default(),
            symbol: own
                .symbol
                .clone()
                .unwrap_or_else(|| format!("tree_sitter_{}", own.name.replace('-', "_"))),
            highlights: queries.highlights,
            injections: queries.injections,
            locals: queries.locals,
            folds: queries.folds,
            outline: queries.outline,
            lent,
            injection_regex,
        },
        source,
        queries_from: queries_from.filter(|_| combined),
        queries_at: lender.and_then(folder_of).filter(|_| combined),
        lent_from,
        file_types: file_types.to_vec(),
        first_line,
        checking: combined && problem.is_none(),
        unchecked: None,
        problem,
    }
}

/// Why a library nothing lends highlights to colors nothing, and what
/// makes it color: the grammar's repository in Skwir's folder, whose
/// queries color it, built or not. Emacs keeps none (its modes color in
/// Lisp); a repository of Skwir's or the CLI's has none of its own; an
/// editor's queries of it, `unlent`, are not lent to another's library.
fn no_highlights(source: Source, unlent: Option<Source>) -> String {
    const PUT: &str =
        "put the grammar's repository in Skwir's grammars folder (it need not be built)";
    match (source, unlent) {
        (Source::Emacs, _) => format!("Emacs keeps no colors with its grammars: {PUT}"),
        (Source::Skwir | Source::TreeSitter, _) => {
            "no highlights query in its repository, nor in the system's".to_owned()
        }
        (_, Some(editor)) => format!(
            "no queries installed with it, and {}'s are not lent to another's library: {PUT}",
            editor.label()
        ),
        (_, None) => format!("no highlights query found for its language: {PUT}"),
    }
}

/// The key a language is known by: its name in lower case, `-` read as
/// `_`, as the symbol its library exports names it (`tree_sitter_c_sharp`
/// for Emacs's `c-sharp` and Neovim's `c_sharp`). Choices are kept by it.
pub fn key_of(name: &str) -> String {
    name.to_lowercase().replace('-', "_")
}

/// Whose queries are borrowed before whose, of those that lend
/// (`Source::lends`): the user's repository in Skwir's folder, then the
/// CLI's, then the system's.
fn lender_rank(source: Source) -> u8 {
    match source {
        Source::Skwir => 0,
        Source::TreeSitter => 1,
        _ => 2,
    }
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

fn read_text(path: &Path) -> Option<String> {
    let bytes = crate::file::read_regular(path, LIMIT).ok()?;
    String::from_utf8(bytes).ok()
}

/// Whether `name` can be a grammar's: what its library's symbol is made
/// of (`tree_sitter_<name>`, `-` read as `_`). A version a library's file
/// name carries (`libtree-sitter-c.0.dylib`, `c.0.24.so`) makes none.
fn is_grammar_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// A library's file name ends so on this system.
fn is_library(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase());
    name.is_some_and(|name| {
        [".so", ".dylib", ".dll", ".wasm"]
            .iter()
            .any(|ending| name.ends_with(ending))
    })
}

/// The grammars a repository (`tree-sitter build` run in it) describes in
/// its `tree-sitter.json`, else its `package.json`, each with the library
/// built beside it (or, for the CLI's, in `libraries`).
fn repository_grammars(
    source: Source,
    repository: &Path,
    libraries: Option<&Path>,
) -> Vec<Candidate> {
    let described = read_text(&repository.join("tree-sitter.json"))
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json.get("grammars").cloned())
        .or_else(|| {
            read_text(&repository.join("package.json"))
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|json| json.get("tree-sitter").cloned())
        });
    let Some(serde_json::Value::Array(grammars)) = described else {
        return Vec::new();
    };
    let folder_name = repository
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    grammars
        .iter()
        .take(64)
        .filter_map(|grammar| {
            let text = |key: &str| grammar.get(key).and_then(|v| v.as_str()).map(str::to_owned);
            let paths = |key: &str, default: &str| -> Vec<PathBuf> {
                match grammar.get(key) {
                    Some(serde_json::Value::String(path)) => vec![repository.join(path)],
                    Some(serde_json::Value::Array(paths)) => paths
                        .iter()
                        .filter_map(|p| p.as_str())
                        .map(|p| repository.join(p))
                        .collect(),
                    _ => Some(repository.join(default))
                        .filter(|path| path.is_file())
                        .into_iter()
                        .collect(),
                }
            };
            let name = text("name").or_else(|| {
                text("scope")
                    .and_then(|scope| scope.rsplit('.').next().map(str::to_owned))
                    .or_else(|| folder_name.strip_prefix("tree-sitter-").map(str::to_owned))
            })?;
            let at = repository.join(text("path").unwrap_or_else(|| ".".to_owned()));
            let library = library_in(&at, &name, true)
                .or_else(|| libraries.and_then(|dir| library_in(dir, &name, false)));
            Some(Candidate {
                source: Some(source),
                name: name.clone(),
                home: Some(repository.to_owned()),
                library,
                symbol: None,
                queries: Queries {
                    highlights: paths("highlights", "queries/highlights.scm"),
                    injections: paths("injections", "queries/injections.scm"),
                    locals: paths("locals", "queries/locals.scm"),
                    folds: paths("folds", "queries/folds.scm"),
                    outline: paths("tags", "queries/tags.scm"),
                },
                file_types: grammar
                    .get("file-types")
                    .and_then(|v| v.as_array())
                    .map(|types| {
                        types
                            .iter()
                            .filter_map(|t| t.as_str())
                            .map(|t| t.trim_start_matches('.').to_owned())
                            .collect()
                    })
                    .unwrap_or_default(),
                first_line: text("first-line-regex"),
                injection_regex: text("injection-regex"),
                problem: None,
            })
        })
        .collect()
}

/// The library built for `name` in `dir`: as `tree-sitter build` names it
/// (`<name>.so`, `.dylib`, `.dll`, `tree-sitter-<name>.wasm`), as Emacs's
/// (`libtree-sitter-<name>`), or, in a grammar's own repository
/// (`alone`), the folder's only library: a folder of several languages'
/// libraries never gives one another's.
fn library_in(dir: &Path, name: &str, alone: bool) -> Option<PathBuf> {
    let underscored = name.replace('-', "_");
    let mut wanted: Vec<String> = Vec::new();
    for base in [name, underscored.as_str()] {
        for ending in ["so", "dylib", "dll"] {
            wanted.push(format!("{base}.{ending}"));
            wanted.push(format!("libtree-sitter-{base}.{ending}"));
        }
        wanted.push(format!("tree-sitter-{base}.wasm"));
        wanted.push(format!("{base}.wasm"));
    }
    wanted.push("parser.so".to_owned());
    wanted.push("parser.dylib".to_owned());
    wanted.push("parser.dll".to_owned());
    if let Some(path) = wanted
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
    {
        return Some(path);
    }
    if !alone {
        return None;
    }
    let libraries: Vec<PathBuf> = sorted(dir)
        .into_iter()
        .filter(|path| is_library(path) && path.is_file())
        .collect();
    (libraries.len() == 1)
        .then(|| libraries.into_iter().next())
        .flatten()
}

/// The folders the CLI's `config.json` names in `parser-directories`, `~`
/// read as the home folder.
fn parser_directories(config: &Path) -> Vec<PathBuf> {
    let Some(json) =
        read_text(config).and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    else {
        return Vec::new();
    };
    json.get("parser-directories")
        .and_then(|v| v.as_array())
        .map(|dirs| {
            dirs.iter()
                .filter_map(|dir| dir.as_str())
                .filter_map(|dir| match dir.strip_prefix("~/") {
                    Some(rest) => home().map(|home| home.join(rest)),
                    None => Some(PathBuf::from(dir)),
                })
                .filter(|dir| dir.is_absolute())
                .collect()
        })
        .unwrap_or_default()
}

/// A Zed extension's grammars: `grammars/<grammar>.wasm`, each language of
/// `languages/<dir>/config.toml` naming its grammar, the files it colors
/// and its first line, its queries beside it.
fn zed_grammars(extension: &Path) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    for language in sorted(&extension.join("languages")) {
        let Some(config) = read_text(&language.join("config.toml"))
            .and_then(|text| text.parse::<toml::Table>().ok())
        else {
            continue;
        };
        let text = |key: &str| config.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        let Some(grammar) = text("grammar") else {
            continue;
        };
        let library = Some(extension.join("grammars").join(format!("{grammar}.wasm")))
            .filter(|path| path.is_file());
        let query = |name: &str| -> Vec<PathBuf> {
            Some(language.join(name))
                .filter(|path| path.is_file())
                .into_iter()
                .collect()
        };
        let language_name = text("name").unwrap_or_else(|| grammar.clone());
        candidates.push(Candidate {
            source: Some(Source::Zed),
            name: grammar.clone(),
            home: Some(language.clone()),
            library,
            symbol: None,
            queries: Queries {
                highlights: query("highlights.scm"),
                injections: query("injections.scm"),
                locals: query("locals.scm"),
                folds: query("folds.scm"),
                outline: query("outline.scm"),
            },
            file_types: config
                .get("path_suffixes")
                .and_then(|v| v.as_array())
                .map(|types| {
                    types
                        .iter()
                        .filter_map(|t| t.as_str())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            first_line: text("first_line_pattern"),
            // Zed names an injected language by its language's name, any case.
            injection_regex: Some(format!(
                "^(?i:{}|{})$",
                regex::escape(&language_name),
                regex::escape(&grammar)
            )),
            problem: None,
        });
    }
    candidates
}

/// Helix's grammars, its runtimes resolved as Helix resolves them: a
/// language's library (`grammars/<grammar>.so`) from the first runtime
/// holding it, each of its queries (`queries/<lang>/`) from the first
/// runtime holding that file, `; inherits:` followed through every runtime;
/// file types from the user's `languages.toml`.
fn helix_grammars(roots: &Roots) -> Vec<Candidate> {
    let languages = roots
        .helix_languages
        .as_deref()
        .map(helix_languages)
        .unwrap_or_default();
    let query_folders: Vec<PathBuf> = roots
        .helix
        .iter()
        .map(|runtime| runtime.join("queries"))
        .collect();
    let mut names: Vec<String> = Vec::new();
    for runtime in &roots.helix {
        names.extend(
            sorted(&runtime.join("grammars"))
                .into_iter()
                .filter(|path| is_library(path))
                .filter_map(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
                .filter(|name| is_grammar_name(name)),
        );
        // Queries alone are lent to libraries found elsewhere.
        names.extend(
            sorted(&runtime.join("queries"))
                .into_iter()
                .filter_map(|path| Some(path.file_name()?.to_string_lossy().into_owned())),
        );
    }
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|name| {
            let (file_types, grammar) = languages.get(&name).cloned().unwrap_or_default();
            // A library is named by its grammar, which a language may name.
            let library = grammar
                .iter()
                .chain(std::iter::once(&name))
                .find_map(|grammar| {
                    roots
                        .helix
                        .iter()
                        .find_map(|runtime| library_in(&runtime.join("grammars"), grammar, false))
                });
            Candidate {
                source: Some(Source::Helix),
                name: grammar.unwrap_or_else(|| name.clone()),
                home: None,
                library,
                symbol: None,
                queries: inherited_queries(&query_folders, &name),
                file_types,
                first_line: None,
                injection_regex: None,
                problem: None,
            }
        })
        .collect()
}

/// Helix's `languages.toml`: each language's file types (globs left out)
/// and the grammar it names, by its name.
fn helix_languages(path: &Path) -> HashMap<String, (Vec<String>, Option<String>)> {
    let Some(table) = read_text(path).and_then(|text| text.parse::<toml::Table>().ok()) else {
        return HashMap::new();
    };
    table
        .get("language")
        .and_then(|v| v.as_array())
        .map(|languages| {
            languages
                .iter()
                .filter_map(|language| {
                    let name = language.get("name")?.as_str()?.to_owned();
                    let types = language
                        .get("file-types")
                        .and_then(|v| v.as_array())
                        .map(|types| {
                            types
                                .iter()
                                .filter_map(|t| t.as_str())
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    let grammar = language
                        .get("grammar")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned);
                    Some((name, (types, grammar)))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Libraries `<lang>.so` in `parsers` folders (Neovim's, the system's),
/// queries in `<lang>/` of the `queries` folders.
fn parser_folder_grammars(
    source: Source,
    parsers: &[PathBuf],
    queries: &[PathBuf],
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for dir in parsers {
        for path in sorted(dir) {
            if !is_library(&path) || !path.is_file() {
                continue;
            }
            let Some(name) = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .filter(|name| is_grammar_name(name))
            else {
                continue;
            };
            if !seen.insert(name.clone()) {
                continue;
            }
            candidates.push(Candidate {
                source: Some(source),
                queries: inherited_queries(queries, &name),
                name,
                library: Some(path),
                ..Candidate::default()
            });
        }
    }
    // Queries alone, lent to libraries found elsewhere.
    for dir in queries {
        for path in sorted(dir) {
            let Some(name) = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            if path.is_dir() && seen.insert(name.clone()) {
                candidates.push(Candidate {
                    source: Some(source),
                    queries: inherited_queries(queries, &name),
                    name,
                    home: Some(path),
                    ..Candidate::default()
                });
            }
        }
    }
    candidates
}

/// `libtree-sitter-<lang>` libraries in `folders`, as Emacs builds them
/// in its own (no queries: its modes color in Lisp) and as a package puts
/// them in the system's library folders (their queries those `queries`
/// holds of their language, the system's).
fn library_folder_grammars(
    source: Source,
    folders: &[PathBuf],
    queries: &[PathBuf],
) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    // A folder that is another (`/usr/lib64`, a link to `/usr/lib`) is
    // listed once: the system's library folders hold thousands of files.
    let mut listed = HashSet::new();
    for dir in folders {
        if !listed.insert(canonical(dir).unwrap_or_else(|| dir.clone())) {
            continue;
        }
        for path in sorted(dir) {
            let Some(file) = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            let Some(rest) = file.strip_prefix("libtree-sitter-") else {
                continue;
            };
            // `libtree-sitter-c.so`, not `.so.15`, `.0.dylib` nor the
            // runtime itself.
            let Some(name) = [".so", ".dylib", ".dll"]
                .iter()
                .find_map(|ending| rest.strip_suffix(ending))
            else {
                continue;
            };
            if !is_grammar_name(name) || !path.is_file() || !seen.insert(name.to_owned()) {
                continue;
            }
            candidates.push(Candidate {
                source: Some(source),
                name: name.to_owned(),
                library: Some(path.clone()),
                queries: inherited_queries(queries, name),
                ..Candidate::default()
            });
        }
    }
    candidates
}

/// A language's queries in the first of `folders` holding them, a query
/// it inherits (`; inherits: a,b`, Helix's and Neovim's way) coming first,
/// and one that `; extends` it in later folders after. A language is read
/// once however many inherit it: a query inherited twice (`tsx` inherits
/// `typescript` and `jsx`, both `ecma`) comes once, where first met, and
/// a cycle ends.
fn inherited_queries(folders: &[PathBuf], name: &str) -> Queries {
    let kind = |file: &str| -> Vec<PathBuf> {
        let mut files = Vec::new();
        query_files(folders, name, file, &mut files, &mut HashSet::new());
        files
    };
    Queries {
        highlights: kind("highlights.scm"),
        injections: kind("injections.scm"),
        locals: kind("locals.scm"),
        folds: kind("folds.scm"),
        outline: [kind("tags.scm"), kind("outline.scm")]
            .into_iter()
            .find(|files| !files.is_empty())
            .unwrap_or_default(),
    }
}

fn query_files(
    folders: &[PathBuf],
    name: &str,
    file: &str,
    files: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
) {
    if seen.len() > INHERITS || !seen.insert(name.to_lowercase()) {
        return;
    }
    let mut base: Option<PathBuf> = None;
    let mut extending = Vec::new();
    for folder in folders {
        let path = folder.join(name).join(file);
        let Some(text) = read_text(&path) else {
            continue;
        };
        let extends = text.lines().take(5).any(|line| line.trim() == "; extends");
        if extends {
            extending.push(path);
        } else if base.is_none() {
            for parent in inherits(&text) {
                query_files(folders, &parent, file, files, seen);
            }
            base = Some(path);
        }
    }
    files.extend(base);
    files.extend(extending);
}

/// The languages a query's `; inherits: a,b` names, in its first lines.
fn inherits(text: &str) -> Vec<String> {
    text.lines()
        .take(5)
        .find_map(|line| line.trim().strip_prefix("; inherits:"))
        .map(|names| {
            names
                .split(',')
                .map(|name| {
                    name.trim()
                        .trim_matches(|c| c == '(' || c == ')')
                        .to_owned()
                })
                .filter(|name| !name.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// `languages.toml` in Skwir's grammars folder: each language's files,
/// `typst = ["typ"]`, at its top or under `[languages]`.
fn languages_toml(path: &Path) -> HashMap<String, Vec<String>> {
    let Some(table) = read_text(path).and_then(|text| text.parse::<toml::Table>().ok()) else {
        return HashMap::new();
    };
    let table = table
        .get("languages")
        .and_then(|v| v.as_table())
        .cloned()
        .unwrap_or(table);
    table
        .iter()
        .filter_map(|(name, types)| {
            let types: Vec<String> = types
                .as_array()?
                .iter()
                .filter_map(|t| t.as_str())
                .map(|t| t.trim_start_matches('.').to_owned())
                .collect();
            Some((name.to_lowercase(), types))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn names(found: &[Found]) -> Vec<(&str, Source, Option<Source>)> {
        found
            .iter()
            .map(|f| (f.grammar.name.as_str(), f.source, f.queries_from))
            .collect()
    }

    /// Every source at once, each as its owner lays it out.
    #[test]
    fn grammars_are_found_where_each_source_keeps_them() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        // Skwir's own: a repository `tree-sitter build` ran in.
        write(
            &root.join("skwir/tree-sitter-gleam/tree-sitter.json"),
            r#"{"grammars":[{"name":"gleam","scope":"source.gleam","path":".",
                "file-types":["gleam"],"injection-regex":"^gleam$",
                "highlights":"queries/highlights.scm"}]}"#,
        );
        write(
            &root.join("skwir/tree-sitter-gleam/queries/highlights.scm"),
            "(identifier) @variable",
        );
        write(&root.join("skwir/tree-sitter-gleam/gleam.so"), "");
        write(
            &root.join("skwir/languages.toml"),
            "[languages]\ntypst = [\"typ\"]\n",
        );
        // The CLI's: a parser directory, its library in the CLI's cache.
        write(
            &root.join("ts-config/config.json"),
            &format!(
                r#"{{"parser-directories":["{}"]}}"#,
                root.join("src").display()
            ),
        );
        write(
            &root.join("src/tree-sitter-odin/tree-sitter.json"),
            r#"{"grammars":[{"name":"odin","file-types":["odin"]}]}"#,
        );
        write(
            &root.join("src/tree-sitter-odin/queries/highlights.scm"),
            "(identifier) @variable",
        );
        write(&root.join("ts-lib/odin.so"), "");
        // Zed's: a WebAssembly grammar and its language's config.
        let nix = root.join("zed/nix");
        write(&nix.join("grammars/nix.wasm"), "");
        write(
            &nix.join("languages/nix/config.toml"),
            "name = \"Nix\"\ngrammar = \"nix\"\npath_suffixes = [\"nix\"]\n",
        );
        write(
            &nix.join("languages/nix/highlights.scm"),
            "(comment) @comment",
        );
        write(&nix.join("languages/nix/outline.scm"), "(binding) @item");
        // Helix's: a library, queries inheriting another's.
        write(&root.join("helix/grammars/typst.so"), "");
        write(
            &root.join("helix/queries/typst/highlights.scm"),
            "; inherits: markup\n(text) @string",
        );
        write(
            &root.join("helix/queries/markup/highlights.scm"),
            "(heading) @markup.heading",
        );
        // Neovim's: a library whose queries only Helix and the system have,
        // and one of a language Zed has too, its queries Zed's.
        write(&root.join("nvim/parser/lua.so"), "");
        write(&root.join("nvim/parser/nix.so"), "");
        // Neovim's typst whole, beside Helix's.
        write(&root.join("nvim/parser/typst.so"), "");
        write(
            &root.join("nvim-queries/typst/highlights.scm"),
            "(heading) @markup.heading",
        );
        write(
            &root.join("nvim-queries/c/highlights.scm"),
            "(comment) @comment",
        );
        // The system's: queries for Lua, the grammar's own.
        write(
            &root.join("system-queries/lua/highlights.scm"),
            "(comment) @comment",
        );
        write(
            &root.join("helix/queries/lua/highlights.scm"),
            "(comment) @comment.helix",
        );
        // The system's library folders: a package's library, with the
        // system's queries of it, and one without; the system's either way,
        // named as Emacs names its own.
        write(&root.join("system-lib/libtree-sitter-json.so"), "");
        write(
            &root.join("system-queries/json/highlights.scm"),
            "(string) @string",
        );
        write(&root.join("system-lib/libtree-sitter-yaml.so"), "");
        // Emacs's: a library alone, and the runtime itself, never a grammar.
        write(&root.join("emacs/libtree-sitter-zig.so"), "");
        // An Emacs library whose grammar's repository, never built, is in
        // Skwir's folder: its queries color Emacs's library.
        write(&root.join("emacs/libtree-sitter-heex.so"), "");
        write(
            &root.join("skwir/tree-sitter-heex/tree-sitter.json"),
            r#"{"grammars":[{"name":"heex","file-types":["heex"]}]}"#,
        );
        write(
            &root.join("skwir/tree-sitter-heex/queries/highlights.scm"),
            "(comment) @comment",
        );
        write(&root.join("emacs/libtree-sitter.so.0.25"), "");
        write(&root.join("emacs/libtree-sitter-c.so.15"), "");
        let roots = Roots {
            skwir: Some(root.join("skwir")),
            tree_sitter_config: Some(root.join("ts-config/config.json")),
            tree_sitter_libraries: Some(root.join("ts-lib")),
            zed: vec![root.join("zed")],
            helix: vec![root.join("helix")],
            helix_languages: None,
            neovim_parsers: vec![root.join("nvim/parser")],
            neovim_queries: vec![root.join("nvim-queries")],
            system_parsers: vec![root.join("system-parsers")],
            system_queries: vec![root.join("system-queries")],
            system_libraries: vec![root.join("system-lib")],
            emacs: vec![root.join("emacs")],
        };
        let bat = HashMap::from([("zig".to_owned(), vec!["zig".to_owned(), "zon".to_owned()])]);
        let (all, folders) = find_in(&roots, &bat);
        assert!(folders.contains(&root.join("skwir/tree-sitter-gleam")));
        assert!(folders.contains(&root.join("helix")));
        // Each source's grammar of a language is kept, in their order; an
        // editor's queries never color another's library (ADR-110,
        // amended): Neovim's nix parser is not colored by Zed's queries.
        let of = |name: &str| -> Vec<(Source, Option<Source>, bool)> {
            all.iter()
                .filter(|found| found.grammar.name == name)
                .map(|found| (found.source, found.queries_from, found.problem.is_none()))
                .collect()
        };
        assert_eq!(
            of("nix"),
            [(Source::Zed, None, true), (Source::Neovim, None, false)]
        );
        let neovim_nix = all
            .iter()
            .find(|found| found.grammar.name == "nix" && found.source == Source::Neovim)
            .unwrap();
        assert!(
            neovim_nix
                .problem
                .as_deref()
                .unwrap()
                .contains("Zed's are not lent"),
            "{:?}",
            neovim_nix.problem
        );
        assert_eq!(
            of("typst"),
            [(Source::Helix, None, true), (Source::Neovim, None, true)]
        );
        // The first is used, unless another is chosen.
        let chosen = pick(
            &all,
            &BTreeMap::from([("typst".to_owned(), Source::Neovim)]),
        );
        let typst = named(&chosen, "typst").unwrap();
        assert_eq!(typst.source, Source::Neovim);
        assert!(typst.grammar.library.ends_with("nvim/parser/typst.so"));
        assert_eq!(
            typst.file_types,
            ["typ"],
            "a language colors the same files"
        );
        // A source chosen that has none of it, or none usable: the first.
        let none = pick(&all, &BTreeMap::from([("nix".to_owned(), Source::Helix)]));
        assert_eq!(named(&none, "nix").unwrap().source, Source::Zed);
        let unusable = pick(&all, &BTreeMap::from([("nix".to_owned(), Source::Neovim)]));
        assert_eq!(named(&unusable, "nix").unwrap().source, Source::Zed);
        let found = pick(&all, &BTreeMap::new());
        assert_eq!(
            names(&found),
            [
                ("gleam", Source::Skwir, None),
                ("heex", Source::Emacs, Some(Source::Skwir)),
                ("json", Source::System, None),
                ("lua", Source::Neovim, Some(Source::System)),
                ("nix", Source::Zed, None),
                ("odin", Source::TreeSitter, None),
                ("typst", Source::Helix, None),
                ("yaml", Source::System, None),
                ("zig", Source::Emacs, None),
            ]
        );
        let get = |name: &str| named(&found, name).unwrap();
        assert_eq!(
            get("gleam").grammar.injection_regex.as_deref(),
            Some("^gleam$")
        );
        assert_eq!(get("odin").grammar.library, root.join("ts-lib/odin.so"));
        assert!(get("nix").grammar.library.ends_with("grammars/nix.wasm"));
        assert_eq!(get("nix").grammar.outline.len(), 1);
        // Inherited first, then its own.
        assert_eq!(
            get("typst").grammar.highlights,
            [
                root.join("helix/queries/markup/highlights.scm"),
                root.join("helix/queries/typst/highlights.scm")
            ]
        );
        assert_eq!(get("typst").file_types, ["typ"], "languages.toml says");
        assert_eq!(get("zig").file_types, ["zig", "zon"], "bat's say");
        assert_eq!(get("odin").file_types, ["odin"]);
        assert_eq!(get("lua").file_types, ["lua"], "its name");
        // Usable or not, and why: Emacs's library alone says where its
        // colors would come from.
        assert!(
            get("zig")
                .problem
                .as_deref()
                .unwrap()
                .starts_with("Emacs keeps no colors")
        );
        // Its repository unbuilt in Skwir's folder lends its queries: one
        // row, Emacs's library, the repository's colors.
        let heex = get("heex");
        assert!(heex.problem.is_none(), "{:?}", heex.problem);
        assert_eq!(
            heex.grammar.library,
            root.join("emacs/libtree-sitter-heex.so")
        );
        assert_eq!(
            heex.grammar.highlights,
            [root.join("skwir/tree-sitter-heex/queries/highlights.scm")]
        );
        assert_eq!(heex.file_types, ["heex"], "its repository says");
        assert_eq!(
            heex.queries_at.as_deref(),
            Some(root.join("skwir/tree-sitter-heex").as_path()),
            "named by its repository"
        );
        assert_eq!(
            all.iter()
                .filter(|found| found.grammar.name == "heex")
                .count(),
            1,
            "the unbuilt repository has no row of its own"
        );
        // Queries alone are a part, never listed: Neovim's of a parser
        // not installed, Helix's of a language no grammar is of.
        assert!(named(&all, "c").is_none(), "Neovim's queries, no library");
        assert!(named(&all, "markup").is_none(), "Helix's base queries");
        // Combined grammars wait for their check; whole ones do not. The
        // system's queries lend, Helix's do not.
        assert!(heex.checking);
        assert!(get("lua").checking);
        assert_eq!(
            get("lua").grammar.highlights,
            [root.join("system-queries/lua/highlights.scm")]
        );
        assert!(get("lua").grammar.lent.is_empty(), "Helix's not lent");
        assert!(!get("nix").checking && !get("typst").checking);
        // A library in the system's folders is the system's, its queries
        // the system's of it.
        let json = get("json");
        assert!(json.problem.is_none(), "{:?}", json.problem);
        assert_eq!(
            json.grammar.library,
            root.join("system-lib/libtree-sitter-json.so")
        );
        assert_eq!(
            json.grammar.highlights,
            [root.join("system-queries/json/highlights.scm")]
        );
        assert!(
            get("yaml")
                .problem
                .as_deref()
                .unwrap()
                .starts_with("no highlights query found"),
            "the system's, not Emacs's"
        );
        assert!(get("gleam").problem.is_none());
        // Files to grammars.
        assert_eq!(
            for_file(&found, "main.gleam", None).unwrap().grammar.name,
            "gleam"
        );
        assert_eq!(
            for_file(&found, "paper.typ", None).unwrap().grammar.name,
            "typst"
        );
        assert_eq!(
            for_file(&found, "page.heex", None).unwrap().grammar.name,
            "heex"
        );
        assert!(
            for_file(&found, "build.zig", None).is_none(),
            "zig has no queries"
        );
    }

    /// A repository's description names files anywhere, and a repository
    /// may hold links: a grammar keeps only the files that really lie in
    /// its repository, and the helper reads its folders alone. A link in an
    /// editor's folder is followed (NixOS's parsers lead into its store).
    #[cfg(unix)]
    #[test]
    fn a_grammar_keeps_only_files_in_its_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        write(&root.join("private/secret.scm"), "(identifier) @variable");
        write(&root.join("private/elsewhere.so"), "");
        let repository = root.join("skwir/tree-sitter-hostile");
        write(
            &repository.join("tree-sitter.json"),
            &format!(
                r#"{{"grammars":[
                    {{"name":"hostile","file-types":["hst"],
                      "highlights":["{}","../../private/secret.scm","queries/linked.scm",
                                    "queries/highlights.scm"]}},
                    {{"name":"away","path":"{}","file-types":["away"]}}]}}"#,
                root.join("private/secret.scm").display(),
                root.join("private").display(),
            ),
        );
        write(&repository.join("hostile.so"), "");
        write(
            &repository.join("queries/highlights.scm"),
            "(identifier) @variable",
        );
        std::os::unix::fs::symlink(
            root.join("private/secret.scm"),
            repository.join("queries/linked.scm"),
        )
        .unwrap();
        // Neovim's parser folder holds a link to a library elsewhere.
        write(&root.join("store/lua.so"), "");
        std::fs::create_dir_all(root.join("nvim/parser")).unwrap();
        std::os::unix::fs::symlink(root.join("store/lua.so"), root.join("nvim/parser/lua.so"))
            .unwrap();
        write(
            &root.join("nvim/queries/lua/highlights.scm"),
            "(comment) @comment",
        );
        let roots = Roots {
            skwir: Some(root.join("skwir")),
            neovim_parsers: vec![root.join("nvim/parser")],
            neovim_queries: vec![root.join("nvim/queries")],
            ..Roots::default()
        };
        let (all, folders) = find_in(&roots, &HashMap::new());
        let get = |name: &str| named(&all, name).unwrap();
        assert_eq!(
            get("hostile").grammar.highlights,
            [repository.join("queries/highlights.scm")],
            "only what lies in the repository"
        );
        assert!(get("hostile").problem.is_none());
        let away = get("away");
        assert!(
            away.problem.as_deref().unwrap().contains("outside"),
            "{away:?}"
        );
        assert!(away.grammar.library.as_os_str().is_empty());
        assert_eq!(
            get("lua").grammar.library,
            root.join("store/lua.so"),
            "a link in an editor's folder, followed"
        );
        assert!(
            !folders
                .iter()
                .any(|folder| folder.starts_with(root.join("private")))
        );
        let told: Vec<Grammar> = all.iter().map(|found| found.grammar.clone()).collect();
        let read = folders_read(&told, &folders);
        assert!(read.contains(&repository), "{read:?}");
        assert!(read.contains(&root.join("store")), "{read:?}");
        assert!(
            !read
                .iter()
                .any(|folder| folder.starts_with(root.join("private"))),
            "{read:?}"
        );
    }

    /// Helix's runtimes as Helix reads them: the user's overrides one query
    /// of a language whose library and other queries the system's holds.
    #[test]
    fn helix_runtimes_are_read_file_by_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        let (user, system) = (root.join("user"), root.join("system"));
        write(
            &user.join("queries/rust/highlights.scm"),
            "; inherits: base\n(identifier) @variable",
        );
        write(&system.join("grammars/rust.so"), "");
        write(
            &system.join("queries/rust/highlights.scm"),
            "(comment) @comment",
        );
        write(
            &system.join("queries/rust/injections.scm"),
            "(macro) @injection.content",
        );
        write(
            &system.join("queries/base/highlights.scm"),
            "(string) @string",
        );
        let roots = Roots {
            helix: vec![user.clone(), system.clone()],
            ..Roots::default()
        };
        let (all, _) = find_in(&roots, &HashMap::new());
        let rust = named(&all, "rust").unwrap();
        assert_eq!(rust.grammar.library, system.join("grammars/rust.so"));
        assert_eq!(
            rust.grammar.highlights,
            [
                system.join("queries/base/highlights.scm"),
                user.join("queries/rust/highlights.scm")
            ],
            "the user's, inheriting the system's"
        );
        assert_eq!(
            rust.grammar.injections,
            [system.join("queries/rust/injections.scm")]
        );
        assert!(rust.problem.is_none());
    }

    /// A query inherited twice comes once, and a cycle ends; a repository
    /// not built never hides a built one of the same source.
    #[test]
    fn inheritance_reads_each_language_once() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        let queries = root.join("queries");
        write(
            &queries.join("tsx/highlights.scm"),
            "; inherits: typescript,jsx\n(a) @b",
        );
        write(
            &queries.join("typescript/highlights.scm"),
            "; inherits: ecma\n(a) @b",
        );
        write(
            &queries.join("jsx/highlights.scm"),
            "; inherits: ecma\n(a) @b",
        );
        write(
            &queries.join("ecma/highlights.scm"),
            "; inherits: tsx\n(a) @b",
        );
        let files = inherited_queries(std::slice::from_ref(&queries), "tsx").highlights;
        assert_eq!(
            files,
            ["ecma", "typescript", "jsx", "tsx"]
                .map(|name| queries.join(name).join("highlights.scm"))
        );
        // Two repositories of one language in Skwir's folder: the first
        // not built.
        for (folder, built) in [("a-unbuilt", false), ("b-built", true)] {
            let repository = root.join("skwir").join(folder);
            write(
                &repository.join("tree-sitter.json"),
                r#"{"grammars":[{"name":"gleam","file-types":["gleam"]}]}"#,
            );
            write(&repository.join("queries/highlights.scm"), "(a) @b");
            if built {
                write(&repository.join("gleam.so"), "");
            }
        }
        let roots = Roots {
            skwir: Some(root.join("skwir")),
            ..Roots::default()
        };
        let (all, _) = find_in(&roots, &HashMap::new());
        let gleam: Vec<&Found> = all.iter().filter(|f| f.grammar.name == "gleam").collect();
        assert_eq!(gleam.len(), 1, "{gleam:?}");
        assert!(gleam[0].problem.is_none(), "{gleam:?}");
        assert!(
            gleam[0]
                .grammar
                .library
                .starts_with(root.join("skwir/b-built"))
        );
    }

    /// Where grammars were looked for: each folder with what it held, and
    /// first what was nearly found, with what to do; an editor's queries
    /// of a language it has no parser of are not.
    #[test]
    fn the_folders_looked_in_say_what_was_nearly_found() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        // A repository a folder too deep in Skwir's folder.
        write(
            &root.join("skwir/old/tree-sitter-foo/tree-sitter.json"),
            r#"{"grammars":[{"name":"foo"}]}"#,
        );
        // The CLI's parser directories: one there, one not; a repository
        // a folder too deep in the first, one not built beside it.
        let projects = root.join("projects");
        write(
            &root.join("ts-config/config.json"),
            &serde_json::json!({
                "parser-directories": [projects, root.join("gone")]
            })
            .to_string(),
        );
        write(
            &projects.join("gleam-mode/tree-sitter-gleam/grammar.js"),
            "",
        );
        write(
            &projects.join("tree-sitter-elm/tree-sitter.json"),
            r#"{"grammars":[{"name":"elm"}]}"#,
        );
        write(
            &projects.join("tree-sitter-elm/queries/highlights.scm"),
            "(comment) @comment",
        );
        // Emacs's library with nothing to color with; Helix's queries of a
        // language no parser is of.
        write(&root.join("emacs/libtree-sitter-quint.so"), "");
        write(
            &root.join("helix/queries/markup/highlights.scm"),
            "(heading) @markup.heading",
        );
        let roots = Roots {
            skwir: Some(root.join("skwir")),
            tree_sitter_config: Some(root.join("ts-config/config.json")),
            tree_sitter_libraries: Some(root.join("ts-lib")),
            helix: vec![root.join("helix")],
            system_libraries: vec![root.join("system-lib")],
            emacs: vec![root.join("emacs")],
            ..Roots::default()
        };
        let (_, _, report) = find_all(&roots, &HashMap::new());
        let nearly: Vec<(Source, &Path, &str)> = report
            .nearly
            .iter()
            .map(|n| (n.source, n.what.as_path(), n.said.as_str()))
            .collect();
        let said = |what: &Path| {
            let Some((_, _, said)) = nearly.iter().find(|(_, at, _)| *at == what) else {
                panic!("{} not said: {nearly:#?}", what.display())
            };
            *said
        };
        assert!(said(&root.join("gone")).contains("not there"));
        assert!(said(&projects.join("gleam-mode/tree-sitter-gleam")).contains("add "));
        assert!(said(&root.join("skwir/old/tree-sitter-foo")).starts_with("one folder too deep"));
        assert!(said(&projects.join("tree-sitter-elm")).starts_with("no library"));
        assert!(
            said(&root.join("emacs/libtree-sitter-quint.so")).starts_with("Emacs keeps no colors")
        );
        assert_eq!(nearly.len(), 5, "Helix's markup is not: {nearly:#?}");
        // Each folder, with what it held; one not there held nothing.
        let held = |folder: &Path| {
            report
                .looked
                .iter()
                .find(|looked| looked.folder == folder)
                .map(|looked| (looked.there, looked.languages.clone()))
                .unwrap()
        };
        assert_eq!(held(&projects), (true, vec!["elm".to_owned()]));
        assert_eq!(held(&root.join("gone")), (false, Vec::new()));
        assert_eq!(held(&root.join("emacs")), (true, vec!["quint".to_owned()]));
        assert_eq!(held(&root.join("helix")), (true, vec!["markup".to_owned()]));
        assert_eq!(held(&root.join("system-lib")), (false, Vec::new()));
        // A configuration that does not read says so.
        write(&root.join("ts-config/config.json"), "{ not json");
        let (_, _, report) = find_all(&roots, &HashMap::new());
        assert!(
            report
                .nearly
                .iter()
                .any(|n| n.what == root.join("ts-config/config.json")
                    && n.said.contains("does not read")),
            "{:#?}",
            report.nearly
        );
    }

    /// Each grammar is the deepest folder's of its source that holds it,
    /// a folder that is another (a link to it) is looked in once, and a
    /// library left out of its folder (a Zed extension's linked outside
    /// it) is listed and said.
    #[cfg(unix)]
    #[test]
    fn the_folders_looked_in_credit_the_folder_that_holds_each() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        let lib = root.join("usr/lib");
        write(&lib.join("tree_sitter/c.so"), "");
        write(&root.join("queries/c/highlights.scm"), "(a) @a");
        write(&lib.join("libtree-sitter-json.so"), "");
        write(&root.join("queries/json/highlights.scm"), "(a) @a");
        std::os::unix::fs::symlink(&lib, root.join("usr/lib64")).unwrap();
        // A Zed extension whose library is a link out of it.
        let nix = root.join("zed/nix");
        write(&root.join("elsewhere/nix.wasm"), "");
        std::fs::create_dir_all(nix.join("grammars")).unwrap();
        std::os::unix::fs::symlink(
            root.join("elsewhere/nix.wasm"),
            nix.join("grammars/nix.wasm"),
        )
        .unwrap();
        write(
            &nix.join("languages/nix/config.toml"),
            "name = \"Nix\"\ngrammar = \"nix\"\npath_suffixes = [\"nix\"]\n",
        );
        write(&nix.join("languages/nix/highlights.scm"), "(a) @a");
        let roots = Roots {
            zed: vec![root.join("zed")],
            system_parsers: vec![lib.join("tree_sitter")],
            system_queries: vec![root.join("queries")],
            system_libraries: vec![lib.clone(), root.join("usr/lib64")],
            ..Roots::default()
        };
        let (all, _, report) = find_all(&roots, &HashMap::new());
        let held = |folder: &Path| {
            report
                .looked
                .iter()
                .find(|looked| looked.folder == folder)
                .map(|looked| looked.languages.clone())
        };
        assert_eq!(held(&lib.join("tree_sitter")), Some(vec!["c".to_owned()]));
        assert_eq!(held(&lib), Some(vec!["json".to_owned()]), "not c");
        assert_eq!(held(&root.join("usr/lib64")), None, "the same folder");
        let nix = named(&all, "nix").unwrap();
        assert_eq!(nix.source, Source::Zed);
        assert!(
            nix.problem.as_deref().unwrap().contains("outside"),
            "{nix:?}"
        );
        assert!(
            report
                .nearly
                .iter()
                .any(|nearly| nearly.source == Source::Zed && nearly.said.contains("outside")),
            "{:#?}",
            report.nearly
        );
    }

    /// A grammar is known by the symbol its library exports: Emacs's
    /// `c-sharp` and Neovim's `c_sharp` are one language, named as the
    /// first is, each library keeping its own symbol.
    #[test]
    fn one_grammar_named_two_ways_is_one_language() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        write(&root.join("nvim/parser/c_sharp.so"), "");
        write(
            &root.join("nvim/queries/c_sharp/highlights.scm"),
            "(comment) @comment",
        );
        write(&root.join("emacs/libtree-sitter-c-sharp.so"), "");
        let roots = Roots {
            neovim_parsers: vec![root.join("nvim/parser")],
            neovim_queries: vec![root.join("nvim/queries")],
            emacs: vec![root.join("emacs")],
            ..Roots::default()
        };
        let (all, _) = find_in(&roots, &HashMap::new());
        let rows: Vec<(&str, Source, &str)> = all
            .iter()
            .map(|f| (f.grammar.name.as_str(), f.source, f.grammar.symbol.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("c_sharp", Source::Neovim, "tree_sitter_c_sharp"),
                ("c_sharp", Source::Emacs, "tree_sitter_c_sharp"),
            ]
        );
        assert_eq!(key_of("C-Sharp"), "c_sharp");
        assert_eq!(named(&all, "c-sharp").unwrap().source, Source::Neovim);
        // Chosen under either name, the same language.
        let chosen = pick(
            &all,
            &BTreeMap::from([("c_sharp".to_owned(), Source::Neovim)]),
        );
        assert_eq!(chosen.len(), 1);
    }

    /// A combined grammar of Emacs's library and a repository's queries,
    /// waiting for its check.
    fn combined(root: &Path) -> Found {
        Found {
            grammar: Grammar {
                name: "a".into(),
                library: root.join("lib/a.so"),
                highlights: vec![root.join("repo/queries/highlights.scm")],
                lent: vec![syntax::Queries {
                    highlights: vec![root.join("system/a/highlights.scm")],
                    ..syntax::Queries::default()
                }],
                ..Grammar::default()
            },
            source: Source::Emacs,
            queries_from: Some(Source::Skwir),
            queries_at: Some(root.join("repo")),
            lent_from: vec![(Source::System, Some(root.join("system/a")))],
            file_types: vec!["a".into()],
            first_line: None,
            problem: None,
            checking: true,
            unchecked: None,
        }
    }

    fn combined_files(root: &Path) {
        write(&root.join("lib/a.so"), "");
        write(&root.join("repo/queries/highlights.scm"), "(x) @x");
        write(&root.join("system/a/highlights.scm"), "(x) @x");
    }

    /// A combined grammar's check is kept by its files as they were: the
    /// same files fit as they did, a refusal makes it unusable saying why
    /// in words of the part refused, a file changed since is checked again.
    #[test]
    fn a_check_is_kept_by_the_files_it_checked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        combined_files(root);
        let mut all = vec![combined(root)];
        let keys = waiting_keys(&all);
        assert_eq!(keys.len(), 1);
        let none = HashMap::new();
        let mut fits = HashMap::new();
        assert!(
            !fitted(&mut all, &keys, &fits, &none),
            "never checked: waits"
        );
        assert!(all[0].waits());
        fits.insert(keys[0].1.clone(), Fit::Colors(0));
        assert!(fitted(&mut all, &keys, &fits, &none));
        assert!(!all[0].checking && all[0].problem.is_none(), "it fits");
        assert_eq!(all[0].queries_from, Some(Source::Skwir), "named as found");
        // Its queries refused: unusable, saying whose and what to do.
        let mut refused = vec![combined(root)];
        fits.insert(keys[0].1.clone(), Fit::Queries("no node `x`".into()));
        assert!(fitted(&mut refused, &keys, &fits, &none));
        let why = refused[0].problem.as_deref().unwrap();
        assert!(
            why.starts_with("its repository's queries do not fit Emacs's library (no node `x`)")
                && why.contains("revision its library was built from"),
            "{why}"
        );
        // Its library refused: said of the library, never of its queries,
        // nor under the name the helper was told.
        let mut library = vec![combined(root)];
        let abi = "its ABI is 16, and this tree-sitter reads 13 to 15";
        fits.insert(keys[0].1.clone(), Fit::Library(abi.into()));
        assert!(fitted(&mut library, &keys, &fits, &none));
        assert_eq!(
            library[0].problem.as_deref(),
            Some(format!("Emacs's library cannot be used ({abi})").as_str())
        );
        // Its queries written again: another key, checked again.
        std::fs::remove_file(root.join("repo/queries/highlights.scm")).unwrap();
        write(&root.join("repo/queries/highlights.scm"), "(y) @y");
        let mut again = vec![combined(root)];
        let changed = waiting_keys(&again);
        assert_ne!(changed[0].1, keys[0].1);
        assert!(!fitted(&mut again, &changed, &fits, &none));
        assert!(again[0].waits());
    }

    /// A check the helper could not make (it did not start) is no answer:
    /// never kept, the grammar waits saying why, not asked again until the
    /// grammars are found again, then asked again. One check is asked at a
    /// time, and may be asked again once its `Asked` is let go of.
    #[test]
    fn a_check_the_helper_failed_is_asked_again() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        combined_files(root);
        STATE.write().all = Some(vec![combined(root)].into());
        let asked = take_waiting().expect("one waits");
        assert!(take_waiting().is_none(), "one asked at a time");
        drop(asked);
        let asked = take_waiting().expect("asked again once let go of");
        let helper = crate::tree_sitter::with_program(root.join("no-such-helper"));
        let cancel = skwir_core::CancelToken::new();
        assert!(check_waiting(&helper, &cancel), "said, though not kept");
        drop(asked);
        let all = all().unwrap();
        assert!(all[0].checking && all[0].problem.is_none(), "{:?}", all[0]);
        assert!(all[0].unchecked.is_some(), "why it was not checked");
        assert!(!waiting(), "not asked again at once");
        assert!(take_waiting().is_none());
        assert!(STATE.read().fits.as_ref().is_none_or(HashMap::is_empty));
        // Found again: it waits again.
        let mut again = vec![combined(root)];
        let keys = waiting_keys(&again);
        let fits = STATE.read().fits.clone().unwrap_or_default();
        assert!(!fitted(&mut again, &keys, &fits, &HashMap::new()));
        STATE.write().all = Some(again.into());
        assert!(waiting());
        // Cancelled, nothing is said.
        cancel.cancel();
        assert!(!check_waiting(&helper, &cancel));
        assert!(all_waiting());
    }

    fn all_waiting() -> bool {
        all().unwrap().iter().all(Found::waits)
    }

    /// The check says which set of a combined grammar's queries colors it:
    /// named by that one, those before it left out. A whole grammar colors
    /// with its own queries alone, lent none.
    #[test]
    fn a_combined_grammar_is_named_by_the_queries_that_color_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        combined_files(root);
        let mut all = vec![combined(root)];
        let keys = waiting_keys(&all);
        let fits = HashMap::from([(keys[0].1.clone(), Fit::Colors(1))]);
        assert!(fitted(&mut all, &keys, &fits, &HashMap::new()));
        let found = &all[0];
        assert_eq!(found.queries_from, Some(Source::System));
        assert_eq!(found.queries_at, Some(root.join("system/a")));
        assert_eq!(
            found.grammar.highlights,
            [root.join("system/a/highlights.scm")]
        );
        assert!(found.grammar.lent.is_empty() && found.lent_from.is_empty());
        assert!(found.problem.is_none() && !found.checking);
        // From the sources: a library without queries takes each that
        // lends, in turn; one with its own, none.
        write(&root.join("emacs/libtree-sitter-heex.so"), "");
        write(&root.join("nvim/parser/heex.so"), "");
        write(&root.join("nvim/queries/heex/highlights.scm"), "(a) @a");
        write(
            &root.join("skwir/tree-sitter-heex/tree-sitter.json"),
            r#"{"grammars":[{"name":"heex"}]}"#,
        );
        write(
            &root.join("skwir/tree-sitter-heex/queries/highlights.scm"),
            "(b) @b",
        );
        write(&root.join("system-queries/heex/highlights.scm"), "(c) @c");
        let roots = Roots {
            skwir: Some(root.join("skwir")),
            neovim_parsers: vec![root.join("nvim/parser")],
            neovim_queries: vec![root.join("nvim/queries")],
            system_queries: vec![root.join("system-queries")],
            emacs: vec![root.join("emacs")],
            ..Roots::default()
        };
        let (all, _) = find_in(&roots, &HashMap::new());
        let of = |source: Source| all.iter().find(|found| found.source == source).unwrap();
        let neovim = of(Source::Neovim);
        assert!(!neovim.checking && neovim.grammar.lent.is_empty(), "whole");
        let emacs = of(Source::Emacs);
        assert!(emacs.checking);
        assert_eq!(emacs.queries_from, Some(Source::Skwir));
        assert_eq!(
            emacs.lent_from,
            [(Source::System, Some(root.join("system-queries/heex")))]
        );
        assert_eq!(emacs.grammar.lent.len(), 1);
    }

    /// What colors texts is compared, not what only the Reading page says:
    /// a check answered for a grammar coloring meanwhile colors nothing
    /// again.
    #[test]
    fn a_fitting_check_colors_nothing_again() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        let waiting = combined(root);
        let checked = Found {
            checking: false,
            ..waiting.clone()
        };
        assert!(colors_alike(
            std::slice::from_ref(&waiting),
            std::slice::from_ref(&checked)
        ));
        let refused = Found {
            problem: Some("no".into()),
            ..checked.clone()
        };
        assert!(!colors_alike(&[checked], &[refused]));
    }

    /// A version a library's file name carries makes no language of its
    /// own: neither `.so.0.24` nor macOS's `.0.dylib`, nor `c.0.so`.
    #[test]
    fn a_library_version_is_no_language() {
        let tmp = tempfile::tempdir().unwrap();
        let root = &canonical(tmp.path()).unwrap();
        for name in [
            "libtree-sitter-c.so",
            "libtree-sitter-c.so.0.24",
            "libtree-sitter-c.0.dylib",
            "libtree-sitter-c.0.24.dylib",
            "libtree-sitter-c_sharp.dylib",
        ] {
            write(&root.join("lib").join(name), "");
        }
        write(&root.join("parsers/lua.so"), "");
        write(&root.join("parsers/lua.0.so"), "");
        let found = library_folder_grammars(Source::System, &[root.join("lib")], &[]);
        let names: Vec<&str> = found.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["c", "c_sharp"]);
        let found = parser_folder_grammars(Source::Neovim, &[root.join("parsers")], &[]);
        let names: Vec<&str> = found.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["lua"]);
    }

    /// The grammars used follow the choices each caller reads, picked again
    /// only when they differ; each change counts in `generation`.
    #[test]
    fn the_pick_follows_the_choices_it_is_given() {
        let of = |source: Source| Found {
            grammar: Grammar {
                name: "nix".into(),
                ..Grammar::default()
            },
            source,
            queries_from: None,
            queries_at: None,
            lent_from: Vec::new(),
            file_types: vec!["nix".into()],
            first_line: None,
            problem: None,
            checking: false,
            unchecked: None,
        };
        // Before the grammars are found: none, the choice kept; found
        // none, nothing changed.
        let neovim = BTreeMap::from([("nix".to_owned(), Source::Neovim)]);
        assert!(sync(&neovim).is_empty());
        let never = generation();
        {
            let mut state = STATE.write();
            state.all = Some(Vec::new().into());
            repick(&mut state);
        }
        assert_eq!(generation(), never, "found nothing");
        {
            let mut state = STATE.write();
            state.all = Some(vec![of(Source::Zed), of(Source::Neovim)].into());
            repick(&mut state);
        }
        let found = generation();
        assert_eq!(sync(&neovim)[0].source, Source::Neovim, "chosen before");
        assert_eq!(generation(), found, "the same choices: nothing picked");
        assert_eq!(sync(&BTreeMap::new())[0].source, Source::Zed);
        assert_eq!(generation(), found + 1);
        assert_eq!(sync(&BTreeMap::new())[0].source, Source::Zed);
        assert_eq!(generation(), found + 1);
    }

    #[test]
    fn a_name_without_extension_is_told_by_its_first_line() {
        let found = vec![Found {
            grammar: Grammar {
                name: "fish".into(),
                ..Grammar::default()
            },
            source: Source::Skwir,
            queries_from: None,
            queries_at: None,
            lent_from: Vec::new(),
            file_types: vec!["fish".into()],
            first_line: Some("^#!.*\\bfish\\b".into()),
            problem: None,
            checking: false,
            unchecked: None,
        }];
        assert!(for_file(&found, "setup", Some("#!/usr/bin/env fish")).is_some());
        // Unusable, it is no grammar to color with, but still the file's.
        let broken = [Found {
            problem: Some("no library".into()),
            ..found[0].clone()
        }];
        assert!(for_file(&broken, "config.fish", None).is_none());
        assert!(for_file_any(&broken, "config.fish", None).is_some());
        assert!(for_file_any(&broken, "setup", Some("#!/usr/bin/env fish")).is_some());
        assert!(for_file(&found, "setup", Some("#!/bin/sh")).is_none());
        // A name with an extension stays the name's (AGENTS.md).
        assert!(for_file(&found, "setup.txt", Some("#!/usr/bin/env fish")).is_none());
        assert!(for_file(&found, "config.FISH", None).is_some());
    }
}
