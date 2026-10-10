use std::collections::HashSet;
use std::fmt;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::BufRead;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use glob::Pattern;
use mime::Mime;
use unicase::UniCase;

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum GlobType {
    Literal(String),
    Simple(String),
    Full(Pattern),
}

impl fmt::Debug for GlobType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GlobType::Literal(name) => write!(f, "Literal '{}'", name),
            GlobType::Simple(pattern) => write!(f, "Simple glob '*{}'", pattern),
            GlobType::Full(pattern) => write!(f, "Full glob '{}'", pattern),
        }
    }
}

impl GlobType {
    /// The length of the pattern as `Display` writes it, without writing it.
    fn len(&self) -> usize {
        match self {
            GlobType::Literal(str) | GlobType::Simple(str) => str.len(),
            GlobType::Full(pattern) => pattern.as_str().len(),
        }
    }
}

impl fmt::Display for GlobType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GlobType::Literal(str) => write!(f, "{}", str),
            GlobType::Simple(str) => write!(f, "{}", str),
            GlobType::Full(pattern) => write!(f, "{}", pattern),
        }
    }
}

fn determine_type(glob: &str) -> GlobType {
    let mut maybe_simple = false;

    for (idx, ch) in glob.bytes().enumerate() {
        if idx == 0 && ch == b'*' {
            maybe_simple = true;
        } else if ch == b'\\' || ch == b'[' || ch == b'*' || ch == b'?' {
            return GlobType::Full(Pattern::new(glob).unwrap());
        }
    }

    if maybe_simple {
        GlobType::Simple(glob[1..].to_string())
    } else {
        GlobType::Literal(glob.to_string())
    }
}

#[derive(Clone)]
pub struct Glob {
    glob: GlobType,
    weight: i32,
    case_sensitive: bool,
    mime_type: Mime,
}

impl PartialEq for Glob {
    fn eq(&self, other: &Glob) -> bool {
        self.glob == other.glob && self.mime_type == other.mime_type
    }
}

impl Eq for Glob {}

impl Hash for Glob {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.glob.hash(h);
        self.mime_type.hash(h)
    }
}

impl fmt::Debug for Glob {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Glob: {:?} {:?} (weight: {}, cs: {})",
            self.glob, self.mime_type, self.weight, self.case_sensitive
        )
    }
}

impl Glob {
    pub fn simple(mime_type: &Mime, glob: &str) -> Glob {
        Glob {
            mime_type: mime_type.clone(),
            glob: determine_type(glob),
            weight: 50,
            case_sensitive: false,
        }
    }

    pub fn with_weight(mime_type: &Mime, glob: &str, weight: i32) -> Glob {
        Glob {
            mime_type: mime_type.clone(),
            glob: determine_type(glob),
            weight,
            case_sensitive: false,
        }
    }

    pub fn new(mime_type: &Mime, glob: &str, weight: i32, cs: bool) -> Glob {
        Glob {
            mime_type: mime_type.clone(),
            glob: determine_type(glob),
            weight,
            case_sensitive: cs,
        }
    }

    pub fn from_v1_string(s: &str) -> Option<Glob> {
        let mut chunks = s.split(':').fuse();
        let mime_type = chunks.next().and_then(|s| Mime::from_str(s).ok())?;
        let glob = chunks.next().filter(|s| !s.is_empty())?;

        // The globs file is not extensible, so consume any
        // leftover tokens
        if chunks.next().is_some() {
            return None;
        }

        Some(Glob::new(&mime_type, glob, 50, false))
    }

    pub fn from_v2_string(s: &str) -> Option<Glob> {
        let mut chunks = s.split(':').fuse();

        let weight = chunks
            .next()
            .and_then(|v| v.parse::<i32>().ok())
            .filter(|n| *n >= 0)?;

        let mime_type = chunks.next().and_then(|s| Mime::from_str(s).ok())?;
        let glob = chunks.next()?;

        let mut case_sensitive = false;
        if let Some(flags) = chunks.next() {
            let flags_chunks = flags.split(',').collect::<Vec<&str>>();

            // Allow for extra flags
            if flags_chunks.iter().any(|&f| f == "cs") {
                case_sensitive = true;
            }
        }

        // Ignore any other token, for extensibility:
        //
        // https://specifications.freedesktop.org/shared-mime-info-spec/shared-mime-info-spec-latest.html#idm46152099256048

        Some(Glob::new(&mime_type, glob, weight, case_sensitive))
    }

    fn compare(&self, file_name: &str) -> bool {
        self.matches(file_name, &file_name.to_lowercase())
    }

    /// Whether the glob matches `file_name`, whose lowercase form is
    /// `lowercase`: computed once by the caller for every glob it tries,
    /// not once per glob.
    fn matches(&self, file_name: &str, lowercase: &str) -> bool {
        match &self.glob {
            GlobType::Literal(s) => {
                let a = UniCase::new(s);
                let b = UniCase::new(file_name);

                a == b
            }
            GlobType::Simple(s) => {
                file_name.ends_with(s.as_str())
                    || (!self.case_sensitive && lowercase.ends_with(s.as_str()))
            }
            GlobType::Full(p) => p.matches(file_name),
        }
    }
}

pub fn read_globs_v1_from_file<P: AsRef<Path>>(file_name: P) -> Option<Vec<Glob>> {
    let f = match File::open(file_name) {
        Ok(v) => v,
        Err(_) => return None,
    };

    let mut res = Vec::new();
    let file = BufReader::new(&f);
    for line in file.lines() {
        if line.is_err() {
            return None;
        }

        let line = line.unwrap();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        match Glob::from_v1_string(&line) {
            Some(v) => res.push(v),
            None => continue,
        }
    }

    Some(res)
}

pub fn read_globs_v2_from_file<P: AsRef<Path>>(file_name: P) -> Option<Vec<Glob>> {
    let f = match File::open(file_name) {
        Ok(v) => v,
        Err(_) => return None,
    };

    let mut res = Vec::new();
    let file = BufReader::new(&f);
    for line in file.lines() {
        if line.is_err() {
            return None;
        }

        let line = line.unwrap();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        match Glob::from_v2_string(&line) {
            Some(v) => res.push(v),
            None => continue,
        }
    }

    Some(res)
}

pub fn read_globs_from_dir<P: AsRef<Path>>(dir: P) -> Vec<Glob> {
    let mut globs_file = PathBuf::new();
    globs_file.push(dir);
    globs_file.push("globs2");

    match read_globs_v2_from_file(&globs_file) {
        Some(v) => v,
        None => {
            globs_file.pop();
            globs_file.push("globs");

            read_globs_v1_from_file(globs_file).unwrap_or_default()
        }
    }
}

/// The globs, indexed so that a name is matched against the few that can
/// match it rather than every one: a literal by the whole name (ignoring
/// case, as `Glob::matches` compares it), a simple glob (`*.ext`) by its
/// suffix, looked up for each suffix of the name and of its lowercase
/// form; only the full globs (a few dozen) are tried one by one. The
/// answer is the one trying every glob gives (`lookup_linear`).
pub struct GlobMap {
    globs: HashSet<Glob>,
    /// The globs, in the order they were added, indexed below by position.
    list: Vec<Glob>,
    literal: std::collections::HashMap<UniCase<String>, Vec<usize>>,
    /// Simple globs that ignore case: matched by the name's suffixes and
    /// its lowercase form's.
    simple: std::collections::HashMap<String, Vec<usize>>,
    /// Simple globs that do not: matched by the name's suffixes alone.
    simple_cs: std::collections::HashMap<String, Vec<usize>>,
    full: Vec<usize>,
}

impl GlobMap {
    pub fn new() -> GlobMap {
        GlobMap {
            globs: HashSet::new(),
            list: Vec::new(),
            literal: std::collections::HashMap::new(),
            simple: std::collections::HashMap::new(),
            simple_cs: std::collections::HashMap::new(),
            full: Vec::new(),
        }
    }

    pub fn add_glob(&mut self, glob: Glob) {
        if !self.globs.insert(glob.clone()) {
            return;
        }
        let at = self.list.len();
        match &glob.glob {
            GlobType::Literal(name) => self
                .literal
                .entry(UniCase::new(name.clone()))
                .or_default()
                .push(at),
            GlobType::Simple(suffix) if glob.case_sensitive => {
                self.simple_cs.entry(suffix.clone()).or_default().push(at)
            }
            GlobType::Simple(suffix) => self.simple.entry(suffix.clone()).or_default().push(at),
            GlobType::Full(_) => self.full.push(at),
        }
        self.list.push(glob);
    }

    pub fn add_globs(&mut self, globs: &[Glob]) {
        for glob in globs {
            self.add_glob(glob.clone());
        }
    }

    pub fn lookup_mime_type_for_file_name(&self, file_name: &str) -> Option<Vec<Mime>> {
        // Lowercased once, for every glob that ignores case.
        let lowercase = file_name.to_lowercase();
        let mut found: Vec<usize> = Vec::new();
        if let Some(at) = self.literal.get(&UniCase::new(file_name.to_owned())) {
            found.extend(at);
        }
        for suffix in suffixes(file_name) {
            for map in [&self.simple, &self.simple_cs] {
                if let Some(at) = map.get(suffix) {
                    found.extend(at);
                }
            }
        }
        if lowercase != file_name {
            for suffix in suffixes(&lowercase) {
                if let Some(at) = self.simple.get(suffix) {
                    found.extend(at);
                }
            }
        }
        found.extend(
            self.full
                .iter()
                .filter(|&&at| self.list[at].matches(file_name, &lowercase)),
        );
        found.sort_unstable();
        found.dedup();
        choose(found.iter().map(|&at| &self.list[at]).collect())
    }

    /// [`Self::lookup_mime_type_for_file_name`] by trying every glob: what
    /// the index must answer the same as (skwir's tests compare them).
    #[doc(hidden)]
    pub fn lookup_linear(&self, file_name: &str) -> Option<Vec<Mime>> {
        let lowercase = file_name.to_lowercase();
        choose(
            self.list
                .iter()
                .filter(|glob| glob.matches(file_name, &lowercase))
                .collect(),
        )
    }

    pub fn clear(&mut self) {
        *self = GlobMap::new();
    }
}

/// Every suffix of `name`, from the whole of it to the empty one.
fn suffixes(name: &str) -> impl Iterator<Item = &str> {
    name.char_indices()
        .map(|(at, _)| at)
        .chain(std::iter::once(name.len()))
        .map(move |at| &name[at..])
}

/// Of the globs that match a name, the types of the heaviest, and of
/// those the longest, in the order the globs were added.
fn choose(mut matching_globs: Vec<&Glob>) -> Option<Vec<Mime>> {
    // Sort in descending order by weight
    matching_globs.sort_by(|a, b| b.weight.cmp(&a.weight));

    let biggest_weight = matching_globs.first()?.weight;

    // "Keep only globs with the biggest weight."
    // -- shared-mime-info, "Recommended checking order"
    let matching_globs = matching_globs
        .iter()
        .filter(|glob| glob.weight == biggest_weight);

    // Needs to be after filtering for biggest weight
    // in case it changes which glob is the longest.
    let biggest_glob_length = matching_globs
        .clone()
        .map(|glob| glob.glob.len())
        .max()?;

    // "If the patterns are different, keep only the globs
    // with the longest pattern, as previously discussed."
    // -- shared-mime-info, "Recommended checking order"
    let res = matching_globs
        .filter(|glob| glob.glob.len() == biggest_glob_length)
        .map(|glob| glob.mime_type.clone())
        .collect();

    Some(res)
}

impl fmt::Debug for GlobMap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut lines = String::new();
        for glob in &self.list {
            lines.push_str(&format!("{:?}", glob));
            lines.push('\n');
        }

        write!(f, "Globs:\n{}", lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_type() {
        assert_eq!(
            determine_type("*.gif"),
            GlobType::Simple(".gif".to_string())
        );
        assert_eq!(
            determine_type("Foo*.gif"),
            GlobType::Full(Pattern::new("Foo*.gif").unwrap())
        );
        assert_eq!(
            determine_type("*[4].gif"),
            GlobType::Full(Pattern::new("*[4].gif").unwrap())
        );
        assert_eq!(
            determine_type("Makefile"),
            GlobType::Literal("Makefile".to_string())
        );
        assert_eq!(
            determine_type("sldkfjvlsdf\\\\slkdjf"),
            GlobType::Full(Pattern::new("sldkfjvlsdf\\\\slkdjf").unwrap())
        );
        assert_eq!(
            determine_type("tree.[ch]"),
            GlobType::Full(Pattern::new("tree.[ch]").unwrap())
        );
    }

    #[test]
    fn glob_v1_string() {
        assert_eq!(
            Glob::from_v1_string("text/rust:*.rs"),
            Some(Glob::simple(&Mime::from_str("text/rust").unwrap(), "*.rs"))
        );
        assert_eq!(
            Glob::from_v1_string("text/rust:*.rs"),
            Some(Glob::new(
                &Mime::from_str("text/rust").unwrap(),
                "*.rs",
                50,
                false
            ))
        );

        assert_eq!(Glob::from_v1_string(""), None);
        assert_eq!(Glob::from_v1_string("foo"), None);
        assert_eq!(Glob::from_v1_string("foo:"), None);
        assert_eq!(Glob::from_v1_string(":bar"), None);
        assert_eq!(Glob::from_v1_string(":"), None);
        assert_eq!(Glob::from_v1_string("foo:bar:baz"), None);
    }

    #[test]
    fn glob_v2_string() {
        assert_eq!(
            Glob::from_v2_string("80:text/rust:*.rs"),
            Some(Glob::with_weight(
                &Mime::from_str("text/rust").unwrap(),
                "*.rs",
                80
            ))
        );
        assert_eq!(
            Glob::from_v2_string("80:text/rust:*.rs"),
            Some(Glob::new(
                &Mime::from_str("text/rust").unwrap(),
                "*.rs",
                80,
                false
            ))
        );
        assert_eq!(
            Glob::from_v2_string("50:text/x-c++src:*.C:cs"),
            Some(Glob::new(
                &Mime::from_str("text/x-c++src").unwrap(),
                "*.C",
                50,
                true
            ))
        );

        assert_eq!(Glob::from_v2_string(""), None);
        assert_eq!(Glob::from_v2_string("foo"), None);
        assert_eq!(Glob::from_v2_string("foo:"), None);
        assert_eq!(Glob::from_v2_string(":bar"), None);
        assert_eq!(Glob::from_v2_string(":"), None);
        assert_eq!(Glob::from_v2_string("foo:bar:baz"), None);
        assert_eq!(Glob::from_v2_string("foo:bar:baz:blah"), None);

        assert_eq!(
            Glob::from_v2_string("50:text/x-c++src:*.C:cs,newflag:newfeature:somethingelse"),
            Some(Glob::new(
                &Mime::from_str("text/x-c++src").unwrap(),
                "*.C",
                50,
                true
            ))
        );
    }

    #[test]
    fn compare() {
        // Literal
        let copying = Glob::new(
            &Mime::from_str("text/x-copying").unwrap(),
            "copying",
            50,
            false,
        );
        assert_eq!(copying.compare(&"COPYING".to_string()), true);

        // Simple, case-insensitive
        let c_src = Glob::new(&Mime::from_str("text/x-csrc").unwrap(), "*.c", 50, false);
        assert_eq!(c_src.compare(&"foo.c".to_string()), true);
        assert_eq!(c_src.compare(&"FOO.C".to_string()), true);

        // Simple, case-sensitive
        let cplusplus_src = Glob::new(&Mime::from_str("text/x-c++src").unwrap(), "*.C", 50, true);
        assert_eq!(cplusplus_src.compare(&"foo.C".to_string()), true);
        assert_eq!(cplusplus_src.compare(&"foo.c".to_string()), false);
        assert_eq!(cplusplus_src.compare(&"foo.h".to_string()), false);

        // Full
        let video_x_anim = Glob::new(
            &Mime::from_str("video/x-anim").unwrap(),
            "*.anim[1-9j]",
            50,
            false,
        );
        assert_eq!(video_x_anim.compare(&"foo.anim0".to_string()), false);
        assert_eq!(video_x_anim.compare(&"foo.anim8".to_string()), true);
        assert_eq!(video_x_anim.compare(&"foo.animk".to_string()), false);
        assert_eq!(video_x_anim.compare(&"foo.animj".to_string()), true);
    }
}
