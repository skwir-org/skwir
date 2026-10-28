//! Fonts a document uses but does not carry: a PDF or a book's style naming
//! a font by name ("Georgia", "Arial-BoldMT"), a book in Chinese, Arabic or
//! Hindi without fonts of its own, a PDF naming a CJK font.
//!
//! `MuPDF` asks for a font by name, by script and language, or by CJK
//! ordering. This loader asks the desktop's fontconfig, as every application
//! on it does, through `fc-match`: the face it picks is the one the desktop
//! would. A font asked for by name is taken only when fontconfig has that
//! family: its nearest match would be some other face, and `MuPDF`'s own
//! standard fonts stand in better. A style's generic family (`serif`,
//! `sans-serif`, `monospace`, which `MuPDF` asks for by name before using its
//! own) is the desktop's, as in any reader on it. A fallback is asked for with the language
//! the script is written in. The `mupdf` crate's own system loader (its
//! `system-fonts` feature) is not linked: it answers by name only, and its
//! `font-kit` would share a `fontconfig` binding with gpui's copy of
//! `font-kit`, whose features break it when the workspace builds as one.
//! `MuPDF` keeps each font it gets, so each is asked for once per document;
//! the answers are cached here too, the absent ones as well. A system
//! without `fc-match` gets `MuPDF`'s own behaviour: its standard fonts, and no
//! glyph for other scripts.
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

use mupdf::{CjkFontOrdering, Font, FontHints, FontLoader};
use mupdf_sys::{
    FZ_LANG_ja, FZ_LANG_ko, FZ_LANG_ur, FZ_LANG_urd, FZ_LANG_zh_Hant, UCDN_SCRIPT_ARABIC,
    UCDN_SCRIPT_ARMENIAN, UCDN_SCRIPT_BENGALI, UCDN_SCRIPT_BOPOMOFO, UCDN_SCRIPT_CYRILLIC,
    UCDN_SCRIPT_DEVANAGARI, UCDN_SCRIPT_ETHIOPIC, UCDN_SCRIPT_GEORGIAN, UCDN_SCRIPT_GREEK,
    UCDN_SCRIPT_GUJARATI, UCDN_SCRIPT_GURMUKHI, UCDN_SCRIPT_HAN, UCDN_SCRIPT_HANGUL,
    UCDN_SCRIPT_HEBREW, UCDN_SCRIPT_HIRAGANA, UCDN_SCRIPT_KANNADA, UCDN_SCRIPT_KATAKANA,
    UCDN_SCRIPT_KHMER, UCDN_SCRIPT_LAO, UCDN_SCRIPT_MALAYALAM, UCDN_SCRIPT_MONGOLIAN,
    UCDN_SCRIPT_MYANMAR, UCDN_SCRIPT_ORIYA, UCDN_SCRIPT_SINHALA, UCDN_SCRIPT_SYRIAC,
    UCDN_SCRIPT_TAMIL, UCDN_SCRIPT_TELUGU, UCDN_SCRIPT_THAANA, UCDN_SCRIPT_THAI,
    UCDN_SCRIPT_TIBETAN,
};

/// The largest font file read: a CJK collection holds several faces.
const FONT_BYTES: u64 = 64 * 1024 * 1024;
/// The longest font name asked for; a document's names are its own.
const NAME_BYTES: usize = 128;

/// A face fontconfig chose: its file and index in it.
type Face = Option<(PathBuf, i32)>;

#[derive(Default)]
pub struct Fallback {
    /// `(language, serif)` → the face fontconfig chose, or none.
    chosen: Mutex<HashMap<(&'static str, bool), Face>>,
    /// `(family, bold, italic)` → the face of that family, or none.
    named: Mutex<HashMap<(String, bool, bool), Face>>,
}

impl FontLoader for Fallback {
    fn load_font(&self, name: &str, hints: FontHints) -> Option<Font> {
        let family = family(name)?;
        let face = {
            let mut named = self.named.lock().ok()?;
            named
                .entry((family.clone(), hints.bold, hints.italic))
                .or_insert_with(|| fc_family(&family, hints.bold, hints.italic))
                .clone()
        };
        let font = load(face?)?;
        // A face that only looks bold or italic by MuPDF's slant or
        // emboldening is refused when the document's metrics need the real
        // one.
        let exact = !hints.needs_exact_metrics
            || ((!hints.bold || font.is_bold()) && (!hints.italic || font.is_italic()));
        exact.then_some(font)
    }

    fn load_cjk_font(&self, _name: &str, ordering: CjkFontOrdering, serif: bool) -> Option<Font> {
        let language = match ordering {
            CjkFontOrdering::AdobeGb => "zh-cn",
            CjkFontOrdering::AdobeCns => "zh-tw",
            CjkFontOrdering::AdobeJapan => "ja",
            CjkFontOrdering::AdobeKorea => "ko",
        };
        self.font(language, serif)
    }

    fn load_fallback_font(&self, script: u32, language: u32, hints: FontHints) -> Option<Font> {
        self.font(written_in(script, language)?, hints.serif)
    }
}

impl Fallback {
    fn font(&self, language: &'static str, serif: bool) -> Option<Font> {
        let face = {
            let mut chosen = self.chosen.lock().ok()?;
            chosen
                .entry((language, serif))
                .or_insert_with(|| fc_match(language, serif))
                .clone()
        };
        load(face?)
    }
}

fn load((path, index): (PathBuf, i32)) -> Option<Font> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .ok()?
        .take(FONT_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let name = path.file_stem()?.to_string_lossy();
    Font::from_bytes_with_index(&name, index, &bytes).ok()
}

/// The family a font name asks for: a PDF's `PostScript` name without its
/// style (`Arial-BoldMT`, `Arial,Bold`, `TimesNewRomanPSMT`), a style's
/// family as written. Names too long or holding control characters are
/// asked for as nothing.
fn family(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name.len() > NAME_BYTES || name.chars().any(char::is_control) {
        return None;
    }
    // A subset's tag (`ABCDEF+Arial`) is the document's, not the family's.
    let name = match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => name,
    };
    let base = name.split([',', '-']).next()?;
    let base = ["PSMT", "MT", "PS"]
        .iter()
        .find_map(|suffix| base.strip_suffix(suffix).filter(|rest| !rest.is_empty()))
        .unwrap_or(base);
    (!base.is_empty()).then(|| base.to_owned())
}

/// The face of `family` fontconfig prefers in that style, if it has the
/// family: under its own name or written without spaces, as `PostScript`
/// names are (`TimesNewRoman` for Times New Roman).
fn fc_family(family: &str, bold: bool, italic: bool) -> Face {
    // Fontconfig's pattern syntax gives these characters a meaning.
    let mut pattern = String::new();
    for c in family.chars() {
        if matches!(c, '\\' | '-' | ':' | ',' | '=') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push_str(if bold {
        ":weight=bold"
    } else {
        ":weight=regular"
    });
    pattern.push_str(if italic {
        ":slant=italic"
    } else {
        ":slant=roman"
    });
    let output = Command::new("fc-match")
        .arg("--format=%{file}\t%{index}\t%{family}\n")
        .arg(pattern)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    of_family(&String::from_utf8_lossy(&output.stdout), family)
}

/// `fc-match`'s answer (`file`, `index`, `family`) if its families include
/// `family`, compared without case or spaces, or if `family` is generic.
fn of_family(line: &str, family: &str) -> Face {
    let generic = matches!(family, "serif" | "sans-serif" | "monospace");
    let squeeze = |name: &str| {
        name.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let wanted = squeeze(family);
    let mut fields = line.lines().next()?.split('\t');
    let (file, index, families) = (fields.next()?, fields.next()?, fields.next()?);
    let path = PathBuf::from(file);
    (path.is_absolute() && (generic || families.split(',').any(|name| squeeze(name) == wanted)))
        .then(|| (path, index.trim().parse().unwrap_or(0)))
}

/// The face fontconfig prefers for text in `language`: of the faces it
/// sorts for the style, the first whose coverage includes the language. Its
/// single best match is not enough, since a generic family (`serif`) weighs
/// more than the language and would give a face without its glyphs.
fn fc_match(language: &str, serif: bool) -> Face {
    let family = if serif { "serif" } else { "sans-serif" };
    let output = Command::new("fc-match")
        .arg("--sort")
        .arg("--format=%{file}\t%{index}\t%{lang}\n")
        .arg(format!("{family}:lang={language}"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    covering(&String::from_utf8_lossy(&output.stdout), language)
}

/// The first of `fc-match --sort`'s lines (`file`, `index`, `lang`) whose
/// languages include `language`.
fn covering(sorted: &str, language: &str) -> Face {
    sorted.lines().find_map(|line| {
        let mut fields = line.split('\t');
        let (file, index, languages) = (fields.next()?, fields.next()?, fields.next()?);
        let path = PathBuf::from(file);
        (path.is_absolute() && languages.split('|').any(|covered| covered == language))
            .then(|| (path, index.trim().parse().unwrap_or(0)))
    })
}

/// The language fontconfig is asked for text in `script`: the one `MuPDF`
/// names when it knows it (Chinese, Japanese, Korean, Urdu), else the
/// script's main language. Latin is left to `MuPDF`'s own fonts.
fn written_in(script: u32, language: u32) -> Option<&'static str> {
    #[allow(non_upper_case_globals)] // MuPDF's C names (`FZ_LANG_zh_Hant`).
    let named = match language {
        FZ_LANG_ja => Some("ja"),
        FZ_LANG_ko => Some("ko"),
        FZ_LANG_zh_Hant => Some("zh-tw"),
        FZ_LANG_ur | FZ_LANG_urd => Some("ur"),
        _ => None,
    };
    Some(match script {
        UCDN_SCRIPT_HAN | UCDN_SCRIPT_BOPOMOFO => named.unwrap_or("zh-cn"),
        UCDN_SCRIPT_HIRAGANA | UCDN_SCRIPT_KATAKANA => "ja",
        UCDN_SCRIPT_HANGUL => "ko",
        UCDN_SCRIPT_ARABIC => named.unwrap_or("ar"),
        UCDN_SCRIPT_HEBREW => "he",
        UCDN_SCRIPT_SYRIAC => "syr",
        UCDN_SCRIPT_THAANA => "dv",
        UCDN_SCRIPT_DEVANAGARI => "hi",
        UCDN_SCRIPT_BENGALI => "bn",
        UCDN_SCRIPT_GURMUKHI => "pa",
        UCDN_SCRIPT_GUJARATI => "gu",
        UCDN_SCRIPT_ORIYA => "or",
        UCDN_SCRIPT_TAMIL => "ta",
        UCDN_SCRIPT_TELUGU => "te",
        UCDN_SCRIPT_KANNADA => "kn",
        UCDN_SCRIPT_MALAYALAM => "ml",
        UCDN_SCRIPT_SINHALA => "si",
        UCDN_SCRIPT_THAI => "th",
        UCDN_SCRIPT_LAO => "lo",
        UCDN_SCRIPT_TIBETAN => "bo",
        UCDN_SCRIPT_MYANMAR => "my",
        UCDN_SCRIPT_KHMER => "km",
        UCDN_SCRIPT_MONGOLIAN => "mn",
        UCDN_SCRIPT_GEORGIAN => "ka",
        UCDN_SCRIPT_ARMENIAN => "hy",
        UCDN_SCRIPT_ETHIOPIC => "am",
        UCDN_SCRIPT_GREEK => "el",
        UCDN_SCRIPT_CYRILLIC => "ru",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_asked_for_in_their_language() {
        assert_eq!(written_in(UCDN_SCRIPT_HAN, 0), Some("zh-cn"));
        assert_eq!(written_in(UCDN_SCRIPT_HAN, FZ_LANG_ja), Some("ja"));
        assert_eq!(written_in(UCDN_SCRIPT_HAN, FZ_LANG_zh_Hant), Some("zh-tw"));
        assert_eq!(written_in(UCDN_SCRIPT_ARABIC, FZ_LANG_urd), Some("ur"));
        assert_eq!(written_in(UCDN_SCRIPT_DEVANAGARI, 0), Some("hi"));
        assert_eq!(written_in(mupdf_sys::UCDN_SCRIPT_LATIN, 0), None);
    }

    #[test]
    fn a_name_asks_for_its_family() {
        assert_eq!(family("Arial-BoldMT").as_deref(), Some("Arial"));
        assert_eq!(family("Arial,Bold").as_deref(), Some("Arial"));
        assert_eq!(
            family("TimesNewRomanPSMT").as_deref(),
            Some("TimesNewRoman")
        );
        assert_eq!(family("ABCDEF+Georgia").as_deref(), Some("Georgia"));
        assert_eq!(family(" Noto Serif ").as_deref(), Some("Noto Serif"));
        assert_eq!(family("MT"), Some("MT".into()));
        assert_eq!(family(""), None);
        assert_eq!(family("bad\nname"), None);
        assert_eq!(family(&"x".repeat(200)), None);
    }

    #[test]
    fn only_the_family_asked_for_is_taken() {
        let line = "/fonts/TNR.ttf\t0\tTimes New Roman,Times\n";
        assert_eq!(
            of_family(line, "TimesNewRoman"),
            Some((PathBuf::from("/fonts/TNR.ttf"), 0))
        );
        assert_eq!(
            of_family(line, "times"),
            Some((PathBuf::from("/fonts/TNR.ttf"), 0))
        );
        assert_eq!(
            of_family("/fonts/DejaVu.ttf\t0\tDejaVu Sans\n", "Georgia"),
            None
        );
        assert_eq!(of_family("", "Georgia"), None);
        assert_eq!(
            of_family("/fonts/NotoSerif.ttf\t0\tNoto Serif\n", "serif"),
            Some((PathBuf::from("/fonts/NotoSerif.ttf"), 0))
        );
    }

    #[test]
    fn the_first_face_covering_the_language_is_taken() {
        let sorted = "/fonts/Serif.ttf\t0\taa|en|fr\n\
                      /fonts/CJK.ttc\t3\taa|ja|ko|zh-cn\n\
                      /fonts/Other.ttf\t0\tzh-cn\n";
        assert_eq!(
            covering(sorted, "zh-cn"),
            Some((PathBuf::from("/fonts/CJK.ttc"), 3))
        );
        assert_eq!(covering(sorted, "hi"), None);
        assert_eq!(covering("relative.ttf\t0\tzh-cn\n", "zh-cn"), None);
    }
}
