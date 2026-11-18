//! What no font draws (ADR-139): the first time in a run that text in a
//! script the user reads cannot be drawn (no font installed has its
//! letters, so they show as boxes), a toast says so and names a font that
//! draws them. Text in any other script (a name in a script nobody here
//! reads) is only logged, with its font: a toast at every start for a
//! stray file name would teach the user to dismiss it.
//!
//! gpui says which graphemes ran out of fallback fonts
//! (`App::on_missing_glyphs`): Linux's text system reports them; macOS's
//! and Windows' fall back on every font the system has and report none.
//! The fonts named are Noto's, the families Linux's text system
//! (cosmic-text) looks for first for each script, so installing one draws
//! the letters at the next start; any font that has them does too. The
//! guide (`docs/guide.md`, "Fonts for every script") names each system's
//! packages.
//!
//! As it starts, the text system is told the first of the reader's
//! languages written in Han characters (`han_forms`), so that it draws
//! them in that language's forms (直 and 骨 are drawn apart in Japan,
//! Korea, China, Taiwan and Hong Kong), whatever language the window
//! speaks: fixed for the run, as fonts are (a language chosen meanwhile
//! shows at the next start, ADR-139, 6).

use std::collections::HashSet;
use std::sync::Arc;

use skwir_core::App;
use skwir_core::app::NotifyLevel;
use skwir_core::scripts::{self, HanForms, Script};
use skwir_core::words;
use skwir_core::words::Words;

/// Watches for text no font draws, for the whole run: called once, as
/// the application starts.
pub fn watch(app: &Arc<App>, cx: &mut gpui::App) {
    if let Some(han) = scripts::han_language() {
        han_forms(cx, &han.tag);
    }
    let app = app.weak();
    let mut logged: HashSet<Option<Script>> = HashSet::new();
    let mut warned = false;
    cx.on_missing_glyphs(move |missing, _| {
        let read = scripts::read();
        let han = scripts::han_language().map(|han| han.forms);
        for glyph in missing {
            let grapheme = glyph.grapheme();
            let script = scripts::of(grapheme);
            let font = script.and_then(|script| family(script, han));
            if logged.insert(script) {
                tracing::info!(
                    script = script.as_ref().map_or("none", Script::code),
                    first = %code_point(grapheme),
                    font = font.unwrap_or("unknown"),
                    "no font installed draws it"
                );
            }
            let Some(script) = script.filter(|script| read.contains(script)) else {
                continue;
            };
            if warned {
                continue;
            }
            let Some(app) = app.upgrade() else {
                return;
            };
            warned = true;
            app.notify(NotifyLevel::Warning, warning(script, font, grapheme));
        }
    })
    .detach();
}

/// Tells the text system which language's forms of the Han characters to
/// draw (`language`, a BCP 47 tag: `ja`, `zh-TW`), before the first window:
/// on Linux, the fallback faces it picks (gpui patch 0074; at start it
/// picks by the system's locale, which a language chosen in Skwir or a
/// second language of the system's overrides here). macOS and Windows pick
/// their own.
fn han_forms(cx: &mut gpui::App, language: &str) {
    tracing::debug!(language, "Han characters drawn in this language's forms");
    cx.text_system().set_language(language);
}

/// What the toast says: the font to install, else the first letter none
/// draws. Said in English when the letters missing are those of the
/// window's own language, so that the toast itself can be read.
fn warning(script: Script, font: Option<&'static str>, grapheme: &str) -> Words {
    let words = match font {
        Some(font) => words!(
            glyphs_missing = "No installed font has the letters of some text here, so they show as boxes. Install { $font } to read them: Skwir's guide names its package on each system.",
            font = Words::as_written(font),
            note = "A warning shown once a run, when text in a script the user reads cannot be drawn. The font is a family's name (Noto Sans Devanagari, Noto Sans CJK JP), written as it is."
        ),
        None => words!(
            glyphs_missing_letter = "No installed font has the letters of some text here (the first is { $letter }), so they show as boxes. Install a font that has them: Skwir's guide names Noto's packages on each system.",
            letter = Words::as_written(code_point(grapheme)),
            note = "A warning shown once a run, when text in a script the user reads cannot be drawn and Skwir knows no font for it. The letter is a code point, written as Unicode writes it (U+1E900)."
        ),
    };
    let window = skwir_core::locale::languages();
    let unreadable = window
        .first()
        .is_some_and(|language| scripts::of_language(language).contains(&script));
    if unreadable {
        // The window's own language is what no font draws: these words are
        // said in English, whose letters Skwir's own font always has, and
        // kept as written so the window says them as they are.
        Words::as_written(words.say_in(&[skwir_core::locale::ENGLISH.to_owned()]))
    } else {
        words
    }
}

/// The first character of `grapheme` as Unicode writes a code point
/// (`U+0F40`): a name for a letter that cannot be drawn.
fn code_point(grapheme: &str) -> String {
    grapheme
        .chars()
        .next()
        .map_or_else(String::new, |c| format!("U+{:04X}", u32::from(c)))
}

/// The Noto family that draws `script`: the one Linux's text system
/// (cosmic-text 0.19, `font/fallback/unix.rs`) looks for first, Han in the
/// forms the reader reads (`han`, Simplified Chinese where they read none,
/// as cosmic-text does). None for a script it names none for.
fn family(script: Script, han: Option<HanForms>) -> Option<&'static str> {
    let cjk = |forms: Option<HanForms>| match forms {
        Some(HanForms::Japanese) => "Noto Sans CJK JP",
        Some(HanForms::Korean) => "Noto Sans CJK KR",
        Some(HanForms::Traditional) => "Noto Sans CJK TC",
        Some(HanForms::HongKong) => "Noto Sans CJK HK",
        Some(HanForms::Simplified) | None => "Noto Sans CJK SC",
    };
    Some(match script.code() {
        "Latn" | "Grek" | "Cyrl" => "Noto Sans",
        "Hani" => cjk(han),
        "Bopo" => cjk(han.or(Some(HanForms::Traditional))),
        "Hira" | "Kana" => cjk(Some(HanForms::Japanese)),
        "Hang" => cjk(Some(HanForms::Korean)),
        "Adlm" => "Noto Sans Adlam",
        "Arab" => "Noto Sans Arabic",
        "Armn" => "Noto Sans Armenian",
        "Beng" => "Noto Sans Bengali",
        "Buhd" => "Noto Sans Buhid",
        "Cakm" => "Noto Sans Chakma",
        "Cher" => "Noto Sans Cherokee",
        "Dsrt" => "Noto Sans Deseret",
        "Deva" => "Noto Sans Devanagari",
        "Ethi" => "Noto Sans Ethiopic",
        "Geor" => "Noto Sans Georgian",
        "Goth" => "Noto Sans Gothic",
        "Gran" => "Noto Sans Grantha",
        "Gujr" => "Noto Sans Gujarati",
        "Guru" => "Noto Sans Gurmukhi",
        "Hano" => "Noto Sans Hanunoo",
        "Hebr" => "Noto Sans Hebrew",
        "Java" => "Noto Sans Javanese",
        "Knda" => "Noto Sans Kannada",
        "Khmr" => "Noto Sans Khmer",
        "Laoo" => "Noto Sans Lao",
        "Mlym" => "Noto Sans Malayalam",
        "Mong" => "Noto Sans Mongolian",
        "Mymr" => "Noto Sans Myanmar",
        "Orya" => "Noto Sans Oriya",
        "Runr" => "Noto Sans Runic",
        "Sinh" => "Noto Sans Sinhala",
        "Syrc" => "Noto Sans Syriac",
        "Tglg" => "Noto Sans Tagalog",
        "Tagb" => "Noto Sans Tagbanwa",
        "Tale" => "Noto Sans Tai Le",
        "Lana" => "Noto Sans Tai Tham",
        "Tavt" => "Noto Sans Tai Viet",
        "Taml" => "Noto Sans Tamil",
        "Telu" => "Noto Sans Telugu",
        "Thaa" => "Noto Sans Thaana",
        "Thai" => "Noto Sans Thai",
        // Noto draws Tibetan in a serif alone.
        "Tibt" => "Noto Serif Tibetan",
        "Tfng" => "Noto Sans Tifinagh",
        "Vaii" => "Noto Sans Vai",
        "Yiii" => "Noto Sans Yi",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(code: &str) -> Script {
        Script::named(code).unwrap()
    }

    #[test]
    fn each_script_names_the_noto_family_that_draws_it() {
        let deva = scripts::of("नमस्ते").unwrap();
        assert_eq!(family(deva, None), Some("Noto Sans Devanagari"));
        assert_eq!(family(named("Thai"), None), Some("Noto Sans Thai"));
        assert_eq!(family(named("Tibt"), None), Some("Noto Serif Tibetan"));
        // Han in the forms the reader reads, Chinese's Simplified where
        // they read none; the kanas are Japanese's, Hangul Korean's.
        let han = scripts::of("直").unwrap();
        assert_eq!(family(han, None), Some("Noto Sans CJK SC"));
        assert_eq!(
            family(han, Some(HanForms::Japanese)),
            Some("Noto Sans CJK JP")
        );
        assert_eq!(
            family(han, Some(HanForms::HongKong)),
            Some("Noto Sans CJK HK")
        );
        assert_eq!(
            family(named("Kana"), Some(HanForms::Simplified)),
            Some("Noto Sans CJK JP")
        );
        assert_eq!(family(named("Hang"), None), Some("Noto Sans CJK KR"));
        assert_eq!(family(named("Bopo"), None), Some("Noto Sans CJK TC"));
        // A script Noto's fallback list names nothing for.
        assert_eq!(family(named("Egyp"), None), None);
    }

    #[test]
    fn the_warning_names_the_font_else_the_letter() {
        let deva = named("Deva");
        assert_eq!(
            warning(deva, Some("Noto Sans Devanagari"), "न").english(),
            "No installed font has the letters of some text here, so they show as boxes. Install Noto Sans Devanagari to read them: Skwir's guide names its package on each system."
        );
        assert_eq!(
            warning(named("Egyp"), None, "\u{13000}").english(),
            "No installed font has the letters of some text here (the first is U+13000), so they show as boxes. Install a font that has them: Skwir's guide names Noto's packages on each system."
        );
        assert_eq!(code_point("e\u{301}"), "U+0065");
        assert_eq!(code_point(""), "");
    }
}
