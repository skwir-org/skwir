//! Tree-sitter grammars found (ADR-110): where Skwir, the tree-sitter CLI,
//! Zed, Helix, Neovim, the system and Emacs keep them, each with the
//! queries that color it and the files it colors.
//!
//! A grammar is a library (a shared library, or a WebAssembly module Zed
//! builds) and queries (`highlights.scm` and its kind). Sources come in
//! this order, the first holding a library for a language giving it:
//! Skwir's own folder, the CLI's, Zed's, Helix's, Neovim's, the system's,
//! Emacs's. A library without queries borrows those another source holds
//! for its language, the grammar's own repository's first (Skwir's, the
//! CLI's, the system's), then Helix's, Zed's, Neovim's. What files a
//! language colors is said by `languages.toml` in Skwir's folder first,
//! then its own source, another source, bat's grammar of that name, and
//! last its name taken as an extension.
//!
//! Finding reads folders and small files: a worker's work, at start and
//! when the settings open, as bat's themes are.
use parking_lot::RwLock;
use skwir_core::syntax::Grammar;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Far more than a grammar's description or a query holds.
const LIMIT: u64 = 4 << 20;
/// Grammars found at most, every source together.
const MOST: usize = 2000;
/// How many languages a query may inherit, through each other.
const INHERITS: usize = 8;

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

    /// Whose queries come from the grammar's own repository, so are
    /// borrowed first.
    fn upstream(self) -> bool {
        matches!(self, Source::Skwir | Source::TreeSitter | Source::System)
    }
}

/// A language's grammar as found: what the helper loads, where it came
/// from, the files it colors, and what makes it unusable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub grammar: Grammar,
    pub source: Source,
    /// Where its queries came from, when another source than its own.
    pub queries_from: Option<Source>,
    /// Extensions (without their dot) and whole names it colors.
    pub file_types: Vec<String>,
    /// What a file's first line says when its name says nothing.
    pub first_line: Option<String>,
    pub problem: Option<String>,
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
    /// Folders holding `libtree-sitter-<lang>` libraries: Emacs's own,
    /// then the system's library folders.
    pub emacs: Vec<PathBuf>,
}

/// The grammars found, as last found; `None` before they are.
static FOUND: RwLock<Option<Arc<[Found]>>> = RwLock::new(None);

/// The grammars found, as last found. Finding them again (`find`) is a
/// worker's work.
pub fn found() -> Option<Arc<[Found]>> {
    FOUND.read().clone()
}

/// Find the grammars again where this system keeps them, keep them for
/// `found`, and answer them.
pub fn find() -> Arc<[Found]> {
    let found: Arc<[Found]> = find_in(&Roots::here(), &bat_file_types()).into();
    *FOUND.write() = Some(Arc::clone(&found));
    found
}

/// The grammar that colors a file named `name`: by its whole name, then
/// its extensions, longest first (`d.ts` before `ts`), then its first
/// line when its name says nothing. Usable ones only.
pub fn for_file<'a>(found: &'a [Found], name: &str, first_line: Option<&str>) -> Option<&'a Found> {
    let usable = || found.iter().filter(|found| found.problem.is_none());
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
    found
        .iter()
        .find(|found| found.grammar.name.eq_ignore_ascii_case(name))
}

/// What bat's grammars say each language colors, by its name in lower
/// case: the files a grammar found without file types borrows.
fn bat_file_types() -> HashMap<String, Vec<String>> {
    let set = crate::highlight::syntaxes();
    set.syntaxes()
        .iter()
        .map(|syntax| (syntax.name.to_lowercase(), syntax.file_extensions.clone()))
        .collect()
}

impl Roots {
    /// Where this system keeps them.
    pub fn here() -> Roots {
        let config = config_home();
        let data = skwir_core::xdg::data_home();
        let home = home();
        let mut roots = Roots {
            skwir: skwir_core::folders::data().map(|dir| dir.join("grammars")),
            ..Roots::default()
        };
        // The CLI's own: `$TREE_SITTER_DIR`, else its configuration folder.
        roots.tree_sitter_config = std::env::var_os("TREE_SITTER_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| config.as_ref().map(|dir| dir.join("tree-sitter")))
            .map(|dir| dir.join("config.json"));
        roots.tree_sitter_libraries = std::env::var_os("TREE_SITTER_LIBDIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| skwir_core::xdg::cache_home().map(|dir| dir.join("tree-sitter/lib")));
        roots.zed = zed_folders(data.as_deref(), home.as_deref());
        roots.helix_languages = config.as_ref().map(|dir| dir.join("helix/languages.toml"));
        roots.helix = std::env::var_os("HELIX_RUNTIME")
            .map(PathBuf::from)
            .into_iter()
            .chain(config.as_ref().map(|dir| dir.join("helix/runtime")))
            .chain(program_prefix("hx").map(|prefix| prefix.join("lib/runtime")))
            .chain(program_prefix("helix").map(|prefix| prefix.join("lib/runtime")))
            .chain(
                [
                    "/usr/lib/helix/runtime",
                    "/usr/share/helix/runtime",
                    "/usr/lib/hx/runtime",
                    "/usr/local/lib/helix/runtime",
                ]
                .map(PathBuf::from),
            )
            .collect();
        let nvim = program_prefix("nvim");
        roots.neovim_parsers = data
            .iter()
            .flat_map(|data| {
                let lazy = data.join("nvim/lazy/nvim-treesitter");
                [data.join("nvim/site/parser"), lazy.join("parser")]
            })
            .chain(nvim.iter().map(|prefix| prefix.join("lib/nvim/parser")))
            .chain(
                [
                    "/usr/lib/nvim/parser",
                    "/usr/lib64/nvim/parser",
                    "/usr/local/lib/nvim/parser",
                ]
                .map(PathBuf::from),
            )
            .collect();
        roots.neovim_queries = data
            .iter()
            .flat_map(|data| {
                let lazy = data.join("nvim/lazy/nvim-treesitter");
                [
                    data.join("nvim/site/queries"),
                    lazy.join("queries"),
                    lazy.join("runtime/queries"),
                ]
            })
            .chain(
                nvim.iter()
                    .map(|prefix| prefix.join("share/nvim/runtime/queries")),
            )
            .chain(["/usr/share/nvim/runtime/queries"].map(PathBuf::from))
            .collect();
        roots.system_parsers = [
            "/usr/lib/tree_sitter",
            "/usr/lib64/tree_sitter",
            "/usr/local/lib/tree_sitter",
        ]
        .map(PathBuf::from)
        .into();
        roots.system_queries = [
            "/usr/share/tree-sitter/queries",
            "/usr/local/share/tree-sitter/queries",
        ]
        .map(PathBuf::from)
        .into();
        roots.emacs = home
            .iter()
            .map(|home| home.join(".emacs.d/tree-sitter"))
            .chain(config.iter().map(|dir| dir.join("emacs/tree-sitter")))
            .chain(
                [
                    "/usr/lib",
                    "/usr/lib64",
                    "/usr/local/lib",
                    "/usr/lib/x86_64-linux-gnu",
                    "/usr/lib/aarch64-linux-gnu",
                    "/opt/homebrew/lib",
                ]
                .map(PathBuf::from),
            )
            .collect();
        roots
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
}

/// The configuration folder editors use: XDG's on Linux and the BSDs,
/// Application Support on macOS, the roaming `AppData` on Windows.
fn config_home() -> Option<PathBuf> {
    #[cfg(any(target_os = "macos", windows))]
    return dirs::config_dir();
    #[cfg(not(any(target_os = "macos", windows)))]
    return skwir_core::xdg::config_home();
}

/// Zed's installed extensions: in its data folder, its Flatpak's on Linux.
fn zed_folders(data: Option<&Path>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = Vec::new();
    #[cfg(target_os = "macos")]
    folders.extend(dirs::config_dir().map(|dir| dir.join("Zed/extensions/installed")));
    #[cfg(windows)]
    folders.extend(dirs::data_local_dir().map(|dir| dir.join("Zed/extensions/installed")));
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        folders.extend(data.map(|dir| dir.join("zed/extensions/installed")));
        folders.extend(
            home.map(|home| home.join(".var/app/dev.zed.Zed/data/zed/extensions/installed")),
        );
    }
    let _ = (data, home);
    folders
}

/// Where a program on `PATH` is installed: the folder above its `bin`, its
/// links followed (a Nix profile's leads into its store path).
fn program_prefix(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .and_then(|path| std::fs::canonicalize(path).ok())
        .and_then(|path| path.parent()?.parent().map(Path::to_path_buf))
}

/// A library and its queries as one source holds them, before they are
/// merged by language.
#[derive(Clone, Debug, Default)]
struct Candidate {
    source: Option<Source>,
    name: String,
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
}

/// The grammars found in `roots`, merged by language, in the order of
/// their names. `bat` names what bat's grammars color, by language.
fn find_in(roots: &Roots, bat: &HashMap<String, Vec<String>>) -> Vec<Found> {
    let mut candidates = Vec::new();
    if let Some(dir) = &roots.skwir {
        for repository in sorted(dir) {
            candidates.extend(repository_grammars(Source::Skwir, &repository, None));
        }
    }
    if let Some(config) = &roots.tree_sitter_config {
        for dir in parser_directories(config) {
            for repository in sorted(&dir) {
                candidates.extend(repository_grammars(
                    Source::TreeSitter,
                    &repository,
                    roots.tree_sitter_libraries.as_deref(),
                ));
            }
        }
    }
    for dir in &roots.zed {
        for extension in sorted(dir) {
            candidates.extend(zed_grammars(&extension));
        }
    }
    candidates.extend(helix_grammars(roots));
    candidates.extend(parser_folder_grammars(
        Source::Neovim,
        &roots.neovim_parsers,
        &roots.neovim_queries,
    ));
    candidates.extend(parser_folder_grammars(
        Source::System,
        &roots.system_parsers,
        &roots.system_queries,
    ));
    candidates.extend(emacs_grammars(&roots.emacs));
    candidates.truncate(MOST);
    let mapped = roots
        .skwir
        .as_ref()
        .map(|dir| languages_toml(&dir.join("languages.toml")))
        .unwrap_or_default();
    merge(candidates, &mapped, bat)
}

/// One grammar per language: the first candidate with a library gives it;
/// queries its own, else borrowed; file types as the module says.
fn merge(
    candidates: Vec<Candidate>,
    mapped: &HashMap<String, Vec<String>>,
    bat: &HashMap<String, Vec<String>>,
) -> Vec<Found> {
    let mut names: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, Vec<Candidate>> = HashMap::new();
    for candidate in candidates {
        let key = candidate.name.to_lowercase();
        if !by_name.contains_key(&key) {
            names.push(key.clone());
        }
        by_name.entry(key).or_default().push(candidate);
    }
    names.sort();
    let mut found = Vec::new();
    for key in names {
        let candidates = &by_name[&key];
        let Some(own) = candidates
            .iter()
            .find(|c| c.library.is_some())
            .or_else(|| candidates.first())
        else {
            continue;
        };
        let source = own.source.unwrap_or(Source::Skwir);
        let (queries, queries_from) = if own.queries.is_empty() {
            let mut lenders: Vec<&Candidate> = candidates
                .iter()
                .filter(|c| !c.queries.is_empty())
                .collect();
            lenders.sort_by_key(|c| {
                let source = c.source.unwrap_or(Source::Skwir);
                (!source.upstream(), lender_rank(source))
            });
            match lenders.first() {
                Some(lender) => (lender.queries.clone(), lender.source),
                None => (Queries::default(), None),
            }
        } else {
            (own.queries.clone(), None)
        };
        let file_types = mapped
            .get(&key)
            .cloned()
            .filter(|types| !types.is_empty())
            .or_else(|| Some(own.file_types.clone()).filter(|types| !types.is_empty()))
            .or_else(|| {
                candidates
                    .iter()
                    .map(|c| c.file_types.clone())
                    .find(|types| !types.is_empty())
            })
            .or_else(|| bat.get(&key).cloned().filter(|types| !types.is_empty()))
            .unwrap_or_else(|| vec![key.clone()]);
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
                (Some(_), true) => Some("no highlights query found for its language".to_owned()),
                (Some(_), false) => None,
            });
        found.push(Found {
            grammar: Grammar {
                name: own.name.clone(),
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
                injection_regex,
            },
            source,
            queries_from: queries_from.filter(|&from| from != source),
            file_types,
            first_line,
            problem,
        });
    }
    found
}

/// Whose queries are borrowed before whose, once the grammars' own
/// repositories' are: Helix's are close to them, Zed's next, Neovim's use
/// conditions only Neovim knows.
fn lender_rank(source: Source) -> u8 {
    match source {
        Source::Skwir => 0,
        Source::TreeSitter => 1,
        Source::System => 2,
        Source::Helix => 3,
        Source::Zed => 4,
        Source::Neovim => 5,
        Source::Emacs => 6,
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

/// Helix's grammars: `grammars/<lang>.so` of each runtime, queries in its
/// `queries/<lang>/` (`; inherits:` followed), file types from the user's
/// `languages.toml`.
fn helix_grammars(roots: &Roots) -> Vec<Candidate> {
    let languages = roots
        .helix_languages
        .as_deref()
        .map(helix_languages)
        .unwrap_or_default();
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for runtime in &roots.helix {
        let queries_dir = runtime.join("queries");
        let mut names: Vec<String> = sorted(&runtime.join("grammars"))
            .into_iter()
            .filter(|path| is_library(path))
            .filter_map(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
            .collect();
        // Queries alone are lent to libraries found elsewhere.
        names.extend(
            sorted(&queries_dir)
                .into_iter()
                .filter_map(|path| Some(path.file_name()?.to_string_lossy().into_owned())),
        );
        names.sort();
        names.dedup();
        for name in names {
            if !seen.insert(name.clone()) {
                continue;
            }
            let (file_types, grammar) = languages.get(&name).cloned().unwrap_or_default();
            candidates.push(Candidate {
                source: Some(Source::Helix),
                name: grammar.unwrap_or_else(|| name.clone()),
                library: library_in(&runtime.join("grammars"), &name, false),
                symbol: None,
                queries: inherited_queries(std::slice::from_ref(&queries_dir), &name),
                file_types,
                first_line: None,
                injection_regex: None,
                problem: None,
            });
        }
    }
    candidates
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
                    ..Candidate::default()
                });
            }
        }
    }
    candidates
}

/// Emacs's grammars: `libtree-sitter-<lang>` libraries, no queries.
fn emacs_grammars(folders: &[PathBuf]) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for dir in folders {
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
            // `libtree-sitter-c.so`, not `.so.15` nor the runtime itself.
            let Some(name) = [".so", ".dylib", ".dll"]
                .iter()
                .find_map(|ending| rest.strip_suffix(ending))
            else {
                continue;
            };
            if name.is_empty() || !path.is_file() || !seen.insert(name.to_owned()) {
                continue;
            }
            candidates.push(Candidate {
                source: Some(Source::Emacs),
                name: name.to_owned(),
                library: Some(path.clone()),
                ..Candidate::default()
            });
        }
    }
    candidates
}

/// A language's queries in the first of `folders` holding them, a query
/// it inherits (`; inherits: a,b`, Helix's and Neovim's way) coming first,
/// and one that `; extends` it in later folders after.
fn inherited_queries(folders: &[PathBuf], name: &str) -> Queries {
    let kind = |file: &str| -> Vec<PathBuf> {
        let mut files = Vec::new();
        query_files(folders, name, file, &mut files, 0);
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
    depth: usize,
) {
    if depth > INHERITS {
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
                query_files(folders, &parent, file, files, depth + 1);
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
        let root = tmp.path();
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
        // Neovim's: a library whose queries only Helix and the system have.
        write(&root.join("nvim/parser/lua.so"), "");
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
        // Emacs's: a library alone, and the runtime itself, never a grammar.
        write(&root.join("emacs/libtree-sitter-zig.so"), "");
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
            emacs: vec![root.join("emacs")],
        };
        let bat = HashMap::from([("zig".to_owned(), vec!["zig".to_owned(), "zon".to_owned()])]);
        let found = find_in(&roots, &bat);
        assert_eq!(
            names(&found),
            [
                ("c", Source::Neovim, None),
                ("gleam", Source::Skwir, None),
                ("lua", Source::Neovim, Some(Source::System)),
                ("markup", Source::Helix, None),
                ("nix", Source::Zed, None),
                ("odin", Source::TreeSitter, None),
                ("typst", Source::Helix, None),
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
        // Usable or not, and why.
        assert!(
            get("zig")
                .problem
                .as_deref()
                .unwrap()
                .contains("highlights")
        );
        assert!(
            get("markup")
                .problem
                .as_deref()
                .unwrap()
                .contains("library")
        );
        assert!(get("c").problem.is_some(), "Neovim's queries, no library");
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
        assert!(
            for_file(&found, "build.zig", None).is_none(),
            "zig has no queries"
        );
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
            file_types: vec!["fish".into()],
            first_line: Some("^#!.*\\bfish\\b".into()),
            problem: None,
        }];
        assert!(for_file(&found, "setup", Some("#!/usr/bin/env fish")).is_some());
        assert!(for_file(&found, "setup", Some("#!/bin/sh")).is_none());
        // A name with an extension stays the name's (AGENTS.md).
        assert!(for_file(&found, "setup.txt", Some("#!/usr/bin/env fish")).is_none());
        assert!(for_file(&found, "config.FISH", None).is_some());
    }
}
