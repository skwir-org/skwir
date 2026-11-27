//! GVfs's questions told apart in the language its daemon speaks
//! (ADR-082): a key's passphrase, asked each time and never kept, from a
//! server's password, which may be. GVfs asks in words of its own, which
//! gettext translates in its daemon's language and `gio mount` passes on as
//! said (its own words in English, `LC_MESSAGES=C`). Each question is
//! matched whole: as GVfs's source writes it, and as its catalog for that
//! language translates it (`<share>/locale/<name>/LC_MESSAGES/gvfs.mo`, GNU
//! gettext's format, read here), each `%s` standing for any name. The
//! language is gettext's choice from the daemon's own environment
//! (`/proc/<pid>/environ`, the daemon found on its bus), the catalogs those
//! of its program's `share` and of its data folders; where the daemon is
//! not read, Skwir's own environment and a test's (`Gvfs::env`) stand for
//! its. A question none of its words name (a code, a proxy's password, a
//! newer GVfs's) is neither: asked, and not kept.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// What a question of GVfs's asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asks {
    /// A key's passphrase (SFTP's), asked each time.
    Passphrase,
    /// A server's password, which may be kept.
    Password,
}

/// GVfs's questions for a key's passphrase, as its source writes them
/// (`daemon/gvfsbackendsftp.c`, 1.60 and 1.62): with the address's user,
/// and without.
const PASSPHRASES: [&str; 2] = [
    "Authentication Required\nEnter passphrase for secure key for “%s” on “%s”:",
    "Authentication Required\nEnter passphrase for secure key for “%s”:",
];

/// Its questions for a server's password: SFTP's, FTP's and AFP's with the
/// address's user and without (`gvfsbackendsftp.c`, `gvfsbackendftp.c`,
/// `gvfsafpserver.c`); WebDAV's for its realm and SMB's for its server
/// (`gvfsbackenddav.c`, `gvfsbackendsmbbrowse.c`); SMB's for a share, with
/// a user and without (`gvfsbackendsmb.c`). Not WebDAV's for a proxy, whose
/// password is not the server's, nor SFTP's for a verification code.
const PASSWORDS: [&str; 5] = [
    "Authentication Required\nEnter password for “%s” on “%s”:",
    "Authentication Required\nEnter user and password for “%s”:",
    "Authentication Required\nEnter password for “%s”:",
    "Authentication Required\nEnter password for share “%s” on “%s”:",
    "Authentication Required\nEnter user and password for share “%s” on “%s”:",
];

/// GVfs's gettext domain: its catalogs' name.
const DOMAIN: &str = "gvfs";

/// A catalog larger than this is not GVfs's (its French one: 45 KB).
const LARGEST: u64 = 4 << 20;

/// GVfs's questions in English and in the languages of the catalogs read.
pub(crate) struct Questions {
    passphrases: Vec<Pattern>,
    passwords: Vec<Pattern>,
}

impl Questions {
    /// As GVfs's source words them, in English alone.
    #[cfg(test)]
    pub fn english() -> Questions {
        Questions::in_catalogs(&[])
    }

    /// In English, and as each of `catalogs` translates them, whichever
    /// gettext takes.
    pub fn in_catalogs(catalogs: &[Catalog]) -> Questions {
        let patterns = |messages: &[&str]| -> Vec<Pattern> {
            messages
                .iter()
                .flat_map(|message| {
                    std::iter::once(*message)
                        .chain(catalogs.iter().filter_map(|c| c.translate(message)))
                        .map(Pattern::of)
                })
                .collect()
        };
        Questions {
            passphrases: patterns(&PASSPHRASES),
            passwords: patterns(&PASSWORDS),
        }
    }

    /// As GVfs's daemon on the bus `env` names (the session's, a test's)
    /// words them: in English, and in its language (module documentation).
    /// On a worker: asks the bus, reads `/proc` and a few catalogs.
    pub fn of_daemon(env: &[(OsString, OsString)]) -> Questions {
        let (environ, share) = match daemon(env) {
            Some((environ, share)) => (environ, share),
            // Not found, or not read: Skwir's environment, as a test's GVfs
            // is given it, stands for the daemon's.
            None => (
                std::env::vars_os().chain(env.iter().cloned()).collect(),
                None,
            ),
        };
        let environ = skwir_core::xdg::Elsewhere::new(environ);
        let folders: Vec<PathBuf> = share.into_iter().chain(environ.data_search()).collect();
        let var = |name: &str| environ.var(name).and_then(|value| value.into_string().ok());
        Questions::in_catalogs(&catalogs(&folders, &var))
    }

    /// What `message`, a question of GVfs's as `gio mount` said it, asks
    /// for: `None` where it is none of its questions.
    pub fn of(&self, message: &str) -> Option<Asks> {
        // A passphrase first: should both match, nothing is kept.
        if self.passphrases.iter().any(|p| p.matches(message)) {
            return Some(Asks::Passphrase);
        }
        self.passwords
            .iter()
            .any(|p| p.matches(message))
            .then_some(Asks::Password)
    }
}

/// `text`'s lines, the empty ones left out, as `gio mount`'s questions are
/// read (`gvfs::Prompts`).
fn lines(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A question's words around its names: what comes before the first,
/// between each two, after the last.
#[derive(Debug, PartialEq, Eq)]
struct Pattern(Vec<String>);

impl Pattern {
    /// `message`'s words, its lines as `gio mount`'s are read: each `%s`
    /// any name (`%1$s`, `%2$s` where a translation reorders them), `%%`
    /// a `%`.
    fn of(message: &str) -> Pattern {
        let message = lines(message);
        let mut pieces = vec![String::new()];
        let mut rest = message.as_str();
        while let Some(at) = rest.find('%') {
            let piece = pieces.last_mut().expect("one at least");
            piece.push_str(&rest[..at]);
            let after = &rest[at + 1..];
            if let Some(tail) = after.strip_prefix('%') {
                piece.push('%');
                rest = tail;
                continue;
            }
            let digits = after.bytes().take_while(u8::is_ascii_digit).count();
            let directive = if digits == 0 {
                Some(after)
            } else {
                after[digits..].strip_prefix('$')
            };
            if let Some(tail) = directive.and_then(|d| d.strip_prefix('s')) {
                pieces.push(String::new());
                rest = tail;
            } else {
                // No name: a `%` as written.
                piece.push('%');
                rest = after;
            }
        }
        pieces.last_mut().expect("one at least").push_str(rest);
        Pattern(pieces)
    }

    /// Whether `text` is these words, any name where one stands.
    fn matches(&self, text: &str) -> bool {
        let Some((first, rest)) = self.0.split_first() else {
            return false;
        };
        let Some(text) = text.strip_prefix(first.as_str()) else {
            return false;
        };
        let Some((last, between)) = rest.split_last() else {
            return text.is_empty();
        };
        let Some(mut text) = text.strip_suffix(last.as_str()) else {
            return false;
        };
        // Each piece where it is first found: names stand for any text.
        between.iter().all(|piece| match text.find(piece.as_str()) {
            Some(at) => {
                text = &text[at + piece.len()..];
                true
            }
            None => false,
        })
    }
}

/// A gettext catalog (a `.mo` file, GNU's format): its messages sorted by
/// their bytes, each beside its translation.
pub(crate) struct Catalog {
    bytes: Vec<u8>,
    big_endian: bool,
    count: usize,
    /// Where the table of messages starts: a length and an offset each.
    originals: usize,
    /// Where the table of translations starts, in the same order.
    translations: usize,
}

/// A `.mo` file's first word, in the byte order it was written in.
const MAGIC: u32 = 0x9504_12de;

impl Catalog {
    /// The catalog `bytes` hold: `None` where they are no `.mo` file, or
    /// one of a revision gettext does not read.
    pub fn parse(bytes: Vec<u8>) -> Option<Catalog> {
        let head: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
        let big_endian = if u32::from_le_bytes(head) == MAGIC {
            false
        } else if u32::from_be_bytes(head) == MAGIC {
            true
        } else {
            return None;
        };
        let mut catalog = Catalog {
            bytes,
            big_endian,
            count: 0,
            originals: 0,
            translations: 0,
        };
        // Its major revision: 0, or 1 (system-dependent strings beside,
        // which GVfs's questions are not).
        if catalog.word(4)? >> 16 > 1 {
            return None;
        }
        catalog.count = catalog.word(8)?;
        catalog.originals = catalog.word(12)?;
        catalog.translations = catalog.word(16)?;
        let size = catalog.count.checked_mul(8)?;
        for table in [catalog.originals, catalog.translations] {
            if table.checked_add(size)? > catalog.bytes.len() {
                return None;
            }
        }
        Some(catalog)
    }

    /// The catalog at `path`, if it is one.
    pub fn read(path: &Path) -> Option<Catalog> {
        Catalog::parse(skwir_core::fs::file::read_regular(path, LARGEST).ok()?)
    }

    /// The word at `at`.
    fn word(&self, at: usize) -> Option<usize> {
        let bytes: [u8; 4] = self.bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
        let word = if self.big_endian {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        };
        usize::try_from(word).ok()
    }

    /// String `index` of the table at `table`, up to its first NUL: a
    /// message without its plural, a translation's first form.
    fn string(&self, table: usize, index: usize) -> Option<&[u8]> {
        let entry = table.checked_add(index.checked_mul(8)?)?;
        let (length, offset) = (self.word(entry)?, self.word(entry.checked_add(4)?)?);
        let string = self.bytes.get(offset..offset.checked_add(length)?)?;
        string.split(|b| *b == 0).next()
    }

    /// `message`'s translation, where this catalog has one, in UTF-8 (as
    /// GVfs's are, and as its daemon sends them).
    pub fn translate(&self, message: &str) -> Option<&str> {
        // Looked for as gettext does: halving the sorted table.
        let (mut low, mut high) = (0, self.count);
        while low < high {
            let middle = low + (high - low) / 2;
            match self.string(self.originals, middle)?.cmp(message.as_bytes()) {
                Ordering::Less => low = middle + 1,
                Ordering::Greater => high = middle,
                Ordering::Equal => {
                    let translated =
                        std::str::from_utf8(self.string(self.translations, middle)?).ok()?;
                    return (!translated.is_empty()).then_some(translated);
                }
            }
        }
        None
    }
}

/// Whether `locale` is C (`C`, `C.UTF-8`, `POSIX`): English, whatever
/// `LANGUAGE` says.
fn is_c(locale: &str) -> bool {
    let name = locale.split(['.', '@']).next().unwrap_or_default();
    name == "C" || name == "POSIX"
}

/// The locales gettext looks for messages in, most wanted first, read
/// from `var` as GNU gettext reads the environment: the locale is
/// `LC_ALL`'s, else `LC_MESSAGES`'s, else `LANG`'s (a variable set to
/// nothing is not set); C, it is English, `LANGUAGE` ignored; else
/// `LANGUAGE`'s list where it is set, up to a C there, else the locale.
pub(crate) fn locales(var: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let set = |name: &str| var(name).filter(|value| !value.is_empty());
    let Some(locale) = set("LC_ALL")
        .or_else(|| set("LC_MESSAGES"))
        .or_else(|| set("LANG"))
    else {
        return Vec::new();
    };
    if is_c(&locale) {
        return Vec::new();
    }
    match set("LANGUAGE") {
        Some(list) => list
            .split(':')
            .filter(|name| !name.is_empty())
            .take_while(|name| *name != "C" && *name != "POSIX")
            .map(ToOwned::to_owned)
            .collect(),
        None => vec![locale],
    }
}

/// A codeset as gettext normalises it to name a folder: its letters in
/// lower case and its digits, `iso` before it where it is digits alone
/// (`UTF-8` is `utf8`, `8859-1` is `iso88591`).
fn normalised(codeset: &str) -> String {
    let kept: String = codeset
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if kept.chars().all(|c| c.is_ascii_digit()) {
        format!("iso{kept}")
    } else {
        kept
    }
}

/// A part of a locale's name where it has one, then none.
fn or_none(part: Option<&str>) -> Vec<Option<&str>> {
    part.map(Some).into_iter().chain([None]).collect()
}

/// The folders gettext looks in for `locale`
/// (`language[_territory][.codeset][@modifier]`), most specific first: its
/// modifier, territory and codeset left out in turn, the codeset also as
/// gettext normalises it, as its `_nl_make_l10nflist` orders them.
fn variants(locale: &str) -> Vec<String> {
    let (rest, modifier) = match locale.split_once('@') {
        Some((rest, modifier)) => (rest, Some(modifier)),
        None => (locale, None),
    };
    let (rest, codeset) = match rest.split_once('.') {
        Some((rest, codeset)) => (rest, Some(codeset)),
        None => (rest, None),
    };
    let (language, territory) = match rest.split_once('_') {
        Some((language, territory)) => (language, Some(territory)),
        None => (rest, None),
    };
    let normal = codeset
        .map(normalised)
        .filter(|n| Some(n.as_str()) != codeset);
    let codesets: Vec<Option<&str>> = codeset
        .into_iter()
        .chain(normal.as_deref())
        .map(Some)
        .chain([None])
        .collect();
    let mut variants = Vec::new();
    for modifier in or_none(modifier) {
        for territory in or_none(territory) {
            for codeset in &codesets {
                let mut name = language.to_owned();
                if let Some(territory) = territory {
                    name.push('_');
                    name.push_str(territory);
                }
                if let Some(codeset) = codeset {
                    name.push('.');
                    name.push_str(codeset);
                }
                if let Some(modifier) = modifier {
                    name.push('@');
                    name.push_str(modifier);
                }
                variants.push(name);
            }
        }
    }
    variants
}

/// GVfs's catalogs for the locales `var` chooses ([`locales`]), most
/// wanted first, found under the data folders `folders`
/// (`<folder>/locale/<name>/LC_MESSAGES/gvfs.mo`), each read once.
pub(crate) fn catalogs(folders: &[PathBuf], var: &dyn Fn(&str) -> Option<String>) -> Vec<Catalog> {
    let locales = locales(var);
    if locales.is_empty() {
        return Vec::new();
    }
    // Only the folders that hold catalogs at all: a session may list
    // dozens of data folders (NixOS's), each looked at once.
    let mut listed = HashSet::new();
    let folders: Vec<PathBuf> = folders
        .iter()
        .map(|folder| folder.join("locale"))
        .filter(|folder| listed.insert(folder.clone()) && folder.is_dir())
        .collect();
    let mut tried = HashSet::new();
    let mut found = Vec::new();
    for name in locales.iter().flat_map(|locale| variants(locale)) {
        for folder in &folders {
            let path = folder
                .join(&name)
                .join("LC_MESSAGES")
                .join(format!("{DOMAIN}.mo"));
            if tried.insert(path.clone())
                && let Some(catalog) = Catalog::read(&path)
            {
                found.push(catalog);
            }
        }
    }
    found
}

/// GVfs's daemon on the bus `env` names (the session's, a test's), as it
/// reads its words: its environment, and its program's data folder
/// (`prefix/share` of `prefix/libexec/gvfsd`), where it was installed (a
/// system's that wraps it, NixOS, lists it in no data folder of the
/// session's). `None` where it is not found or not read.
#[cfg(target_os = "linux")]
fn daemon(env: &[(OsString, OsString)]) -> Option<(Vec<(OsString, OsString)>, Option<PathBuf>)> {
    let bus = crate::bus::connect(env, std::time::Duration::from_secs(5)).ok()?;
    let dbus = zbus::blocking::fdo::DBusProxy::new(&bus).ok()?;
    let name = zbus::names::BusName::try_from(crate::gvfs::DAEMON).ok()?;
    let pid = dbus.get_connection_unix_process_id(name).ok()?;
    let program = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
    // GVfs's, not a program that took its number since (`.gvfsd-wrapped`
    // where a system wraps it).
    if !program.file_name()?.to_string_lossy().contains("gvfsd") {
        return None;
    }
    let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let environ = environ
        .split(|b| *b == 0)
        .filter_map(|pair| {
            let (name, value) = pair.split_at(pair.iter().position(|b| *b == b'=')?);
            Some((
                skwir_core::os_path::from_bytes(name.to_vec()),
                skwir_core::os_path::from_bytes(value[1..].to_vec()),
            ))
        })
        .collect();
    let folder = program.parent()?;
    let share = matches!(
        folder.file_name().and_then(|n| n.to_str()),
        Some("libexec" | "bin")
    )
    .then(|| folder.parent().map(|prefix| prefix.join("share")))
    .flatten();
    Some((environ, share))
}

/// GVfs runs on Linux alone.
#[cfg(not(target_os = "linux"))]
fn daemon(_env: &[(OsString, OsString)]) -> Option<(Vec<(OsString, OsString)>, Option<PathBuf>)> {
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fmt::Write as _;

    use super::*;

    /// GVfs's French words for its questions, as its catalog (1.60.3,
    /// 1.62.0) has them, no-break spaces inside the quotes and before the
    /// colon; and one of its Japanese, whose names are reordered.
    pub(crate) const FRENCH: [(&str, &str); 9] = [
        (
            PASSPHRASES[0],
            "Authentification requise\nSaisissez la phrase de passe de la clé SSH de \
             «\u{a0}%s\u{a0}» pour l’accès à «\u{a0}%s\u{a0}»\u{a0}:",
        ),
        (
            PASSPHRASES[1],
            "Authentification requise\nSaisissez la phrase de passe de la clé SSH pour \
             l’accès à «\u{a0}%s\u{a0}»\u{a0}:",
        ),
        (
            PASSWORDS[0],
            "Authentification requise\nSaisissez le mot de passe de «\u{a0}%s\u{a0}» pour \
             l’accès à «\u{a0}%s\u{a0}»\u{a0}:",
        ),
        (
            PASSWORDS[1],
            "Authentification requise\nSaisissez l’utilisateur et le mot de passe pour \
             l’accès à «\u{a0}%s\u{a0}»\u{a0}:",
        ),
        (
            PASSWORDS[2],
            "Authentification requise\nSaisissez le mot de passe pour «\u{a0}%s\u{a0}»\u{a0}:",
        ),
        (
            PASSWORDS[3],
            "Authentification requise\nSaisissez le mot de passe pour partager \
             «\u{a0}%s\u{a0}» sur «\u{a0}%s\u{a0}»\u{a0}:",
        ),
        (
            PASSWORDS[4],
            "Authentification requise\nSaisissez l’utilisateur et le mot de passe pour \
             partager «\u{a0}%s\u{a0}» sur «\u{a0}%s\u{a0}»\u{a0}:",
        ),
        (
            "Authentication Required\nEnter proxy password:",
            "Authentification requise\nSaisissez le mot de passe du serveur mandataire\u{a0}:",
        ),
        (
            "Enter verification code for %s on %s",
            "Saisissez le code de vérification pour %s sur %s",
        ),
    ];

    const JAPANESE: (&str, &str) = (
        PASSPHRASES[0],
        "認証が必要です\n“%2$s” 上の “%1$s” 用のセキュアキーのパスフレーズを入力してください:",
    );

    /// `text` as a `.po` file quotes it.
    fn quoted(text: &str) -> String {
        let mut quoted = String::from("\"");
        for c in text.chars() {
            match c {
                '\\' => quoted.push_str("\\\\"),
                '"' => quoted.push_str("\\\""),
                '\n' => quoted.push_str("\\n"),
                c => quoted.push(c),
            }
        }
        quoted.push('"');
        quoted
    }

    /// The catalog msgfmt (GNU gettext's, in the dev shell) makes of
    /// `messages`, written `endianness` first, at `mo`.
    pub(crate) fn msgfmt(messages: &[(&str, &str)], endianness: &str, mo: &Path) {
        let mut po =
            String::from("msgid \"\"\nmsgstr \"Content-Type: text/plain; charset=UTF-8\\n\"\n\n");
        for (message, translation) in messages {
            // A message with a name says so, as GVfs's do (`#, c-format`):
            // msgfmt then checks the translation keeps its names.
            if message.contains('%') {
                po.push_str("#, c-format\n");
            }
            writeln!(
                po,
                "msgid {}\nmsgstr {}\n",
                quoted(message),
                quoted(translation)
            )
            .unwrap();
        }
        let source = mo.with_extension("po");
        std::fs::create_dir_all(mo.parent().unwrap()).unwrap();
        std::fs::write(&source, po).unwrap();
        let made = std::process::Command::new("msgfmt")
            .arg("--check-format")
            .arg(format!("--endianness={endianness}"))
            .arg("-o")
            .arg(mo)
            .arg(&source)
            .status()
            .expect("msgfmt, GNU gettext's (in the dev shell)");
        assert!(made.success(), "msgfmt made {}", mo.display());
    }

    fn vars(set: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let set: Vec<(String, String)> = set
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| set.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    /// A translation's names filled in, as GVfs's daemon says it.
    pub(crate) fn said(translation: &str, names: &[&str]) -> String {
        let mut said = translation.to_owned();
        for (n, name) in names.iter().enumerate() {
            said = said.replace(&format!("%{}$s", n + 1), name);
        }
        for name in names {
            said = said.replacen("%s", name, 1);
        }
        said
    }

    #[test]
    fn gvfs_s_questions_are_told_apart_in_english() {
        let english = Questions::english();
        let cases = [
            (
                "Authentication Required\nEnter passphrase for secure key for “me” on “example.com”:",
                Some(Asks::Passphrase),
            ),
            (
                "Authentication Required\nEnter passphrase for secure key for “example.com”:",
                Some(Asks::Passphrase),
            ),
            (
                "Authentication Required\nEnter password for “me” on “example.com”:",
                Some(Asks::Password),
            ),
            (
                "Authentication Required\nEnter user and password for “example.com”:",
                Some(Asks::Password),
            ),
            (
                "Authentication Required\nEnter password for “WebDAV share”:",
                Some(Asks::Password),
            ),
            (
                "Authentication Required\nEnter password for share “photos” on “nas”:",
                Some(Asks::Password),
            ),
            // A name holding the words around it is a name still.
            (
                "Authentication Required\nEnter password for “a ” on “b” on “c”:",
                Some(Asks::Password),
            ),
            // Neither: asked, and not kept.
            ("Authentication Required\nEnter proxy password:", None),
            ("Enter verification code for me on example.com", None),
            ("Enter passphrase for key '/home/u/.ssh/id_ed25519':", None),
            (
                "Authentication Required\nEnter password for “me” on “example.com”: now",
                None,
            ),
            ("", None),
        ];
        for (message, asks) in cases {
            assert_eq!(english.of(message), asks, "{message:?}");
        }
    }

    #[test]
    fn gvfs_s_questions_are_told_apart_in_its_catalog_s_language() {
        let folder = tempfile::tempdir().unwrap();
        for (endianness, at) in [("little", "le.mo"), ("big", "be.mo")] {
            let mo = folder.path().join(at);
            msgfmt(&FRENCH, endianness, &mo);
            let catalog = Catalog::read(&mo).expect("a catalog");
            assert_eq!(catalog.big_endian, endianness == "big");
            assert_eq!(catalog.translate(FRENCH[0].0), Some(FRENCH[0].1));
            assert_eq!(catalog.translate("Not one of its messages"), None);
            let french = Questions::in_catalogs(&[catalog]);
            let cases = [
                (
                    FRENCH[0].1,
                    &["me", "example.com"][..],
                    Some(Asks::Passphrase),
                ),
                (FRENCH[1].1, &["example.com"][..], Some(Asks::Passphrase)),
                (
                    FRENCH[2].1,
                    &["me", "example.com"][..],
                    Some(Asks::Password),
                ),
                (FRENCH[3].1, &["example.com"][..], Some(Asks::Password)),
                (FRENCH[4].1, &["Partage WebDAV"][..], Some(Asks::Password)),
                (FRENCH[5].1, &["photos", "nas"][..], Some(Asks::Password)),
                (FRENCH[6].1, &["photos", "nas"][..], Some(Asks::Password)),
                (FRENCH[7].1, &[][..], None),
                (FRENCH[8].1, &["me", "example.com"][..], None),
            ];
            for (translation, names, asks) in cases {
                let message = said(translation, names);
                assert_eq!(french.of(&message), asks, "{message:?}");
            }
            // Its English still: the daemon's language may be another.
            assert_eq!(
                french.of("Authentication Required\nEnter password for “me” on “example.com”:"),
                Some(Asks::Password)
            );
        }
        // Without its catalog, French is none of its questions: asked, and
        // not kept.
        let passphrase = said(FRENCH[0].1, &["me", "example.com"]);
        assert_eq!(Questions::english().of(&passphrase), None);
        // A translation whose names are reordered (`%2$s`, `%1$s`).
        let mo = folder.path().join("ja.mo");
        msgfmt(&[JAPANESE], "little", &mo);
        let japanese = Questions::in_catalogs(&[Catalog::read(&mo).unwrap()]);
        assert_eq!(
            japanese.of(&said(JAPANESE.1, &["me", "example.com"])),
            Some(Asks::Passphrase)
        );
    }

    #[test]
    fn what_is_no_catalog_is_not_read_as_one() {
        assert!(Catalog::parse(Vec::new()).is_none());
        assert!(Catalog::parse(b"not a catalog at all".to_vec()).is_none());
        // Its tables beyond its end.
        let mut cut = MAGIC.to_le_bytes().to_vec();
        cut.extend([0, 0, 0, 0, 9, 0, 0, 0, 28, 0, 0, 0, 100, 0, 0, 0]);
        assert!(Catalog::parse(cut).is_none());
        // A revision gettext does not read.
        let mut newer = MAGIC.to_le_bytes().to_vec();
        newer.extend([0, 0, 2, 0]);
        newer.extend([0; 12]);
        assert!(Catalog::parse(newer).is_none());
    }

    #[test]
    fn the_locale_is_chosen_as_gettext_chooses_it() {
        let of = |set: &[(&str, &str)]| locales(&vars(set));
        assert_eq!(of(&[("LANG", "fr_FR.UTF-8")]), ["fr_FR.UTF-8"]);
        assert_eq!(
            of(&[("LANG", "de_DE.UTF-8"), ("LC_MESSAGES", "fr_FR.UTF-8")]),
            ["fr_FR.UTF-8"]
        );
        assert_eq!(
            of(&[("LC_ALL", "it_IT.UTF-8"), ("LC_MESSAGES", "fr_FR.UTF-8")]),
            ["it_IT.UTF-8"]
        );
        assert_eq!(
            of(&[("LC_ALL", ""), ("LANG", "fr_FR.UTF-8")]),
            ["fr_FR.UTF-8"],
            "set to nothing is not set"
        );
        // LANGUAGE's list first, up to a C in it.
        assert_eq!(
            of(&[("LANG", "en_US.UTF-8"), ("LANGUAGE", "fr:de::ja")]),
            ["fr", "de", "ja"]
        );
        assert_eq!(
            of(&[("LANG", "en_US.UTF-8"), ("LANGUAGE", "fr:C:de")]),
            ["fr"]
        );
        assert_eq!(
            of(&[("LANG", "fr_FR.UTF-8"), ("LANGUAGE", "")]),
            ["fr_FR.UTF-8"]
        );
        // The C locale is English, whatever LANGUAGE says.
        assert!(of(&[("LANG", "C"), ("LANGUAGE", "fr")]).is_empty());
        assert!(of(&[("LC_ALL", "C.UTF-8"), ("LANG", "fr_FR.UTF-8")]).is_empty());
        assert!(of(&[("LANG", "POSIX")]).is_empty());
        assert!(of(&[("LANGUAGE", "fr")]).is_empty(), "no locale is C");
        assert_eq!(
            variants("fr_FR.UTF-8@euro"),
            [
                "fr_FR.UTF-8@euro",
                "fr_FR.utf8@euro",
                "fr_FR@euro",
                "fr.UTF-8@euro",
                "fr.utf8@euro",
                "fr@euro",
                "fr_FR.UTF-8",
                "fr_FR.utf8",
                "fr_FR",
                "fr.UTF-8",
                "fr.utf8",
                "fr",
            ]
        );
        assert_eq!(variants("sr@latin"), ["sr@latin", "sr"]);
        assert_eq!(
            variants("de_DE.utf8"),
            ["de_DE.utf8", "de_DE", "de.utf8", "de"]
        );
        assert_eq!(variants("ja"), ["ja"]);
        assert_eq!(normalised("8859-1"), "iso88591");
    }

    #[test]
    fn gvfs_s_catalogs_are_found_in_the_language_chosen() {
        let share = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let mo = |language: &str| {
            share
                .path()
                .join("locale")
                .join(language)
                .join("LC_MESSAGES/gvfs.mo")
        };
        msgfmt(&FRENCH, "little", &mo("fr"));
        msgfmt(&[JAPANESE], "little", &mo("ja"));
        let folders = [elsewhere.path().to_path_buf(), share.path().to_path_buf()];
        let found = |set: &[(&str, &str)]| catalogs(&folders, &vars(set));
        let passphrase = said(FRENCH[0].1, &["me", "example.com"]);
        let french = Questions::in_catalogs(&found(&[("LANG", "fr_FR.UTF-8")]));
        assert_eq!(french.of(&passphrase), Some(Asks::Passphrase));
        let french = Questions::in_catalogs(&found(&[("LC_MESSAGES", "fr_CA.UTF-8")]));
        assert_eq!(french.of(&passphrase), Some(Asks::Passphrase));
        // Each language of LANGUAGE's list.
        assert_eq!(
            found(&[("LANG", "en_US.UTF-8"), ("LANGUAGE", "de:ja:fr")]).len(),
            2
        );
        // None for English, or a language GVfs's catalogs here lack.
        assert!(found(&[("LANG", "C.UTF-8"), ("LANGUAGE", "fr")]).is_empty());
        assert!(found(&[("LANG", "de_DE.UTF-8")]).is_empty());
        assert!(found(&[]).is_empty());
    }

    /// Every language GVfs ships tells its questions apart: each passphrase
    /// as one, each password as one, none taken for the other. With
    /// `SKWIR_TEST_GVFS` naming its package (`just remote-check`).
    #[test]
    fn every_language_of_gvfs_tells_its_questions_apart() {
        let Some(package) = std::env::var_os("SKWIR_TEST_GVFS").map(PathBuf::from) else {
            eprintln!("skipped: SKWIR_TEST_GVFS names no GVfs package");
            return;
        };
        let locale = package.join("share/locale");
        let mut languages = 0;
        let mut translated = 0;
        for entry in std::fs::read_dir(&locale).expect("GVfs's catalogs") {
            let entry = entry.unwrap();
            let Some(catalog) = Catalog::read(&entry.path().join("LC_MESSAGES/gvfs.mo")) else {
                continue;
            };
            languages += 1;
            let language = entry.file_name();
            let questions = Questions::in_catalogs(std::slice::from_ref(&catalog));
            for (messages, asks) in [
                (&PASSPHRASES[..], Asks::Passphrase),
                (&PASSWORDS[..], Asks::Password),
            ] {
                for message in messages {
                    let Some(translation) = catalog.translate(message) else {
                        continue;
                    };
                    translated += 1;
                    let message = said(translation, &["someone", "example.com"]);
                    assert_eq!(
                        questions.of(&message),
                        Some(asks),
                        "{language:?}: {message:?}"
                    );
                }
            }
        }
        assert!(
            languages > 10,
            "{languages} languages in {}",
            locale.display()
        );
        eprintln!("{translated} questions in {languages} languages told apart");
    }
}
