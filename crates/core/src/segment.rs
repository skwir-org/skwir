//! Where a text's words begin and end, in every script (ADR-139).
//!
//! A word is found by Unicode's word boundaries (UAX #29) as ICU4X's word
//! segmenter finds them: letters and digits joined as a language joins
//! them ("l'école", "3,14", `file_name`, "צה\"ל"), and the words of the
//! scripts written without spaces found by its dictionaries (Chinese,
//! Japanese, Thai, Lao, Khmer, Burmese): "日本語の設定" is three words,
//! "เปิดไฟล์" two. What a person moves through, selects or types by the
//! word goes through here: a text field's Ctrl and the arrows, its double
//! click ([`word_start_before`], [`word_end_after`], [`segment_at`]), the
//! words a search is typed in ([`terms`]).
//!
//! - **The data is compiled in** (the user's choice, 2026-11-18): ICU4X's
//!   word rules (73 KiB) and its dictionaries of Chinese and Japanese words
//!   (1.9 MiB) and of Thai, Lao, Khmer and Burmese ones (1.0 MiB), 2.99 MiB
//!   of the program in all (3,138,000 bytes: a stripped release build
//!   against the same build without them). The second dictionaries are
//!   those gpui's line breaks read too (`text_system/line_breaks.rs`),
//!   linked once: beside them, words add 1.95 MiB (2,041,072 bytes). Its
//!   pages are read only where a text needs them: a text all ASCII never
//!   reaches them (below), a Latin or a Hebrew one only the rules'. This
//!   module alone reads that data for words, so a pack of it loaded at run
//!   time (`WordSegmenter::try_new_dictionary_with_buffer_provider`) could
//!   take its place without a caller changing.
//! - **Dictionaries, not ICU4X's models**, for Thai, Lao, Khmer and
//!   Burmese: the models are 0.69 MiB smaller, but read Thai at 8.4 µs a
//!   character against the dictionary's 0.07 µs (a field of 32,767 Thai
//!   characters: 275 ms against 2.4 ms, on the window's thread at each
//!   Ctrl and arrow), and part "สวัสดี" (hello) and miss "เปิด|ไฟล์" (open
//!   the file), which the dictionary does not.
//! - **A text all ASCII is parted here**, by UAX #29's rules as ICU4X
//!   applies them to ASCII (`ascii`), which a test holds to ICU4X's own
//!   answer for every short text.

use std::ops::Range;
use std::sync::LazyLock;

use icu_segmenter::iterators::WordBreakIteratorWithWordType;
use icu_segmenter::options::WordBreakInvariantOptions;
use icu_segmenter::scaffold::Utf8;
use icu_segmenter::{WordSegmenter, WordSegmenterBorrowed};

/// A stretch of a text between two word boundaries, by byte offsets: a
/// word (letters, digits, ideographs, joined as UAX #29 joins them), or
/// what lies between two (spaces, punctuation, a symbol).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub range: Range<usize>,
    pub is_word: bool,
}

/// The segments of `text`, from its start to its end, each touching the
/// next: every word boundary of it parts two.
pub fn segments(text: &str) -> Segments<'_> {
    if text.is_ascii() {
        Segments {
            parts: Parts::Ascii(text.as_bytes()),
            at: 0,
        }
    } else {
        by_icu4x(text)
    }
}

/// [`segments`] by ICU4X whatever the text holds: what the ASCII rules are
/// held to.
fn by_icu4x(text: &str) -> Segments<'_> {
    Segments {
        parts: Parts::Icu(segmenter().segment_str(text).iter_with_word_type()),
        at: 0,
    }
}

/// ICU4X's word segmenter, its rules and dictionaries compiled in
/// (`compiled_data`), no locale's tailoring: where the data comes from is
/// said here alone.
fn segmenter() -> WordSegmenterBorrowed<'static> {
    static SEGMENTER: LazyLock<WordSegmenterBorrowed<'static>> =
        LazyLock::new(|| WordSegmenter::new_dictionary(WordBreakInvariantOptions::default()));
    *SEGMENTER
}

/// The words of `text`, each with the byte offset it begins at.
pub fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    segments(text)
        .filter(|segment| segment.is_word)
        .map(move |segment| (segment.range.start, &text[segment.range]))
}

/// Where a move a word back from byte `offset` lands (Ctrl and the arrow
/// toward the text's start, Ctrl+Backspace): the start of the word `offset`
/// is in or after, else the text's start.
pub fn word_start_before(text: &str, offset: usize) -> usize {
    let mut start = 0;
    for segment in segments(text) {
        if segment.range.start >= offset {
            break;
        }
        if segment.is_word {
            start = segment.range.start;
        }
    }
    start
}

/// Where a move a word on from byte `offset` lands (Ctrl and the arrow
/// toward the text's end, Ctrl+Delete): the end of the word `offset` is in
/// or before, else the text's end.
pub fn word_end_after(text: &str, offset: usize) -> usize {
    segments(text)
        .find(|segment| segment.is_word && segment.range.end > offset)
        .map_or(text.len(), |segment| segment.range.end)
}

/// The segment holding the character at byte `offset`, what a double click
/// on it selects: its word, or the spaces or the punctuation it is in. At
/// the text's end, its last segment; nothing in an empty text.
pub fn segment_at(text: &str, offset: usize) -> Range<usize> {
    let mut last = 0..0;
    for segment in segments(text) {
        if segment.range.end > offset {
            return segment.range;
        }
        last = segment.range;
    }
    last
}

/// What was typed to find something, as the words a person means by it:
/// parted at spaces, and inside a run of a script written without them at
/// its words, so "設定を開く" is three terms as "open the settings" is
/// three. A term keeps what is joined to its letters ("l'école", "c++",
/// "ctrl+t", "設定。"): only two words that nothing parts are cut apart,
/// which UAX #29 leaves only where a script written without spaces meets a
/// word ("PDF文件").
pub fn terms(typed: &str) -> Vec<&str> {
    let mut terms = Vec::new();
    for token in typed.split_whitespace() {
        // Two words of ASCII always have something between them.
        if token.is_ascii() {
            terms.push(token);
            continue;
        }
        let mut start = 0;
        let mut after_word = false;
        for segment in segments(token) {
            if segment.is_word && after_word {
                terms.push(&token[start..segment.range.start]);
                start = segment.range.start;
            }
            after_word = segment.is_word;
        }
        terms.push(&token[start..]);
    }
    terms
}

/// What [`segments`] walks.
pub struct Segments<'a> {
    parts: Parts<'a>,
    /// Where the next segment starts.
    at: usize,
}

enum Parts<'a> {
    Ascii(&'a [u8]),
    /// ICU4X's boundaries, each with what the segment before it is.
    Icu(WordBreakIteratorWithWordType<'static, 'a, Utf8>),
}

impl Iterator for Segments<'_> {
    type Item = Segment;

    fn next(&mut self) -> Option<Segment> {
        let start = self.at;
        let (end, is_word) = match &mut self.parts {
            Parts::Ascii(text) => {
                let first = *text.get(start)?;
                (ascii::segment_end(text, start), ascii::begins_word(first))
            }
            // The first boundary is the text's start.
            Parts::Icu(boundaries) => loop {
                let (end, kind) = boundaries.next()?;
                if end > start {
                    break (end, kind.is_word_like());
                }
            },
        };
        self.at = end;
        Some(Segment {
            range: start..end,
            is_word,
        })
    }
}

/// UAX #29's word boundaries in a text all ASCII, as ICU4X draws them.
///
/// Of the rules, ASCII meets these: CR LF stays whole and any other line
/// break stands alone (`WB3`–`WB3b`), spaces run together (`WB3d`), letters and
/// digits join (`WB5`, `WB8`–`WB10`), a letter joins a letter across `:`, `.` or `'`
/// (`WB6`–`WB7`) and a digit a digit across `,`, `;`, `.` or `'` (`WB11`–`WB12`), and
/// `_` joins whatever word it touches (`WB13a`–`WB13b`). Everything else stands
/// alone (`WB999`). A segment is a word where it begins with a letter, a
/// digit or `_` (ICU4X counts `_` alone as one).
mod ascii {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Class {
        Cr,
        Lf,
        /// Vertical tab and form feed.
        Newline,
        Letter,
        Numeric,
        /// `_`.
        ExtendNumLet,
        /// `:`.
        MidLetter,
        /// `,` and `;`.
        MidNum,
        /// `.` and `'` (`MidNumLet` and `Single_Quote`).
        MidNumLetQ,
        /// The space; a tab is no `WSegSpace`.
        Space,
        Other,
    }

    fn class(byte: u8) -> Class {
        match byte {
            b'\r' => Class::Cr,
            b'\n' => Class::Lf,
            0x0b | 0x0c => Class::Newline,
            b'A'..=b'Z' | b'a'..=b'z' => Class::Letter,
            b'0'..=b'9' => Class::Numeric,
            b'_' => Class::ExtendNumLet,
            b':' => Class::MidLetter,
            b',' | b';' => Class::MidNum,
            b'.' | b'\'' => Class::MidNumLetQ,
            b' ' => Class::Space,
            _ => Class::Other,
        }
    }

    /// Whether a segment beginning with `first` is a word.
    pub(super) fn begins_word(first: u8) -> bool {
        matches!(
            class(first),
            Class::Letter | Class::Numeric | Class::ExtendNumLet
        )
    }

    /// Where the segment starting at `start` (within `text`) ends.
    pub(super) fn segment_end(text: &[u8], start: usize) -> usize {
        let mut at = start + 1;
        while at < text.len() && joined(text, at) {
            at += 1;
        }
        at
    }

    /// Whether no boundary parts `text[at - 1]` from `text[at]`.
    fn joined(text: &[u8], at: usize) -> bool {
        use Class::{Cr, ExtendNumLet, Letter, Lf, MidLetter, MidNum, MidNumLetQ, Numeric, Space};
        let (before, after) = (class(text[at - 1]), class(text[at]));
        let ahead = text.get(at + 1).map(|&byte| class(byte));
        let behind = at.checked_sub(2).map(|index| class(text[index]));
        // No arm joins a line break but CR's to LF: it stands alone.
        match (before, after) {
            (Cr, Lf)
            | (Space, Space)
            | (Letter | Numeric | ExtendNumLet, Letter | Numeric | ExtendNumLet) => true,
            (Letter, MidLetter | MidNumLetQ) => ahead == Some(Letter),
            (MidLetter | MidNumLetQ, Letter) => behind == Some(Letter),
            (Numeric, MidNum | MidNumLetQ) => ahead == Some(Numeric),
            (MidNum | MidNumLetQ, Numeric) => behind == Some(Numeric),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parted(text: &str) -> Vec<(&str, bool)> {
        segments(text)
            .map(|segment| (&text[segment.range], segment.is_word))
            .collect()
    }

    fn by_icu4x_parted(text: &str) -> Vec<(&str, bool)> {
        by_icu4x(text)
            .map(|segment| (&text[segment.range], segment.is_word))
            .collect()
    }

    fn words_of(text: &str) -> Vec<&str> {
        words(text).map(|(_, word)| word).collect()
    }

    /// The ASCII rules agree with ICU4X on every text of up to four of
    /// these characters (one or two of each class UAX #29 gives ASCII),
    /// on every pair of ASCII characters, and on longer texts drawn from
    /// them: the same boundaries, the same words.
    #[test]
    fn the_ascii_rules_agree_with_icu4x() {
        const ALPHABET: &[u8] = b"aZ09_.':,;\" \t\r\n\x0b-@";
        let check = |text: &str| {
            assert_eq!(parted(text), by_icu4x_parted(text), "{text:?}");
        };
        let mut text = Vec::new();
        for len in 1..=4u32 {
            for mut n in 0..ALPHABET.len().pow(len) {
                text.clear();
                for _ in 0..len {
                    text.push(ALPHABET[n % ALPHABET.len()]);
                    n /= ALPHABET.len();
                }
                check(std::str::from_utf8(&text).unwrap());
            }
        }
        for a in 0..128u8 {
            for b in 0..128u8 {
                check(std::str::from_utf8(&[a, b]).unwrap());
            }
        }
        // Longer texts, from a fixed seed: what chains (`a.b.c`, `1,2.3`).
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..5_000 {
            text.clear();
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            for at in 0..(seed % 24) {
                let pick = (seed >> (at % 48)) as usize ^ (at as usize * 7);
                text.push(ALPHABET[pick % ALPHABET.len()]);
            }
            check(std::str::from_utf8(&text).unwrap());
        }
        check("the file.txt, 3.14 or 1,000; it's a:b_c\r\n");
    }

    #[test]
    fn chinese_is_parted_into_its_words() {
        assert_eq!(words_of("我们在北京工作"), ["我们", "在", "北京", "工作"]);
        // Beside Latin letters and digits, with nothing between them.
        assert_eq!(words_of("PDF文件"), ["PDF", "文件"]);
    }

    #[test]
    fn japanese_is_parted_into_its_words() {
        assert_eq!(
            words_of("日本語の設定を開く"),
            ["日本語", "の", "設定", "を", "開く"]
        );
        // A word of katakana, its long vowel included.
        assert_eq!(words_of("ファイルマネージャー"), ["ファイルマネージャー"]);
    }

    /// Thai and Lao by their dictionaries (ICU4X 2.4's): a vowel or a tone
    /// mark never parted from its letter.
    #[test]
    fn thai_is_parted_into_its_words() {
        assert_eq!(words_of("สวัสดีครับ"), ["สวัสดี", "ครับ"]);
        assert_eq!(words_of("เปิดไฟล์"), ["เปิด", "ไฟล์"]);
        assert_eq!(words_of("ไฟล์และโฟลเดอร์"), ["ไฟล์", "และ", "โฟลเดอร์"]);
        assert_eq!(words_of("ພາສາລາວ"), ["ພາສາ", "ລາວ"]);
    }

    #[test]
    fn hebrew_words_are_parted_at_spaces_and_kept_whole_across_quotes() {
        assert_eq!(words_of("שלום עולם"), ["שלום", "עולם"]);
        // Gershayim written as ASCII's double quote (WB7b–c), and a geresh
        // as its apostrophe (WB7a).
        assert_eq!(words_of("צה\"ל ג'ירפה"), ["צה\"ל", "ג'ירפה"]);
    }

    #[test]
    fn french_keeps_its_apostrophes_and_parts_its_spaces() {
        assert_eq!(
            words_of("l'école d’été « Fichier »"),
            ["l'école", "d’été", "Fichier"]
        );
        // A narrow no-break space joins a number's groups (WB13a–b), and
        // the word before a colon it stands before.
        assert_eq!(
            words_of("3,14 et 1\u{202f}000"),
            ["3,14", "et", "1\u{202f}000"]
        );
        assert_eq!(segment_at("Nom\u{202f}:", 0), 0.."Nom\u{202f}".len());
    }

    #[test]
    fn a_move_by_words_lands_at_a_word_s_start_or_end() {
        let text = "copy 日本語の設定 now";
        // From inside a word, its start; from a word's start, the one before.
        assert_eq!(word_start_before(text, 2), 0);
        let settings = text.find("設定").unwrap();
        assert_eq!(word_start_before(text, settings + 3), settings);
        assert_eq!(word_start_before(text, settings), text.find('の').unwrap());
        assert_eq!(word_start_before(text, 0), 0);
        // On: the end of the word it is in or before; past the last, the end.
        assert_eq!(word_end_after(text, 0), 4);
        assert_eq!(word_end_after(text, 4), text.find('の').unwrap());
        assert_eq!(word_end_after(text, text.len() - 1), text.len());
        assert_eq!(word_end_after("a, ", 1), 3);
        assert_eq!(word_start_before("", 0), 0);
        assert_eq!(word_end_after("", 0), 0);
    }

    #[test]
    fn a_double_click_selects_the_segment_under_it() {
        let text = "ab, 北京工作";
        assert_eq!(segment_at(text, 1), 0..2);
        assert_eq!(segment_at(text, 2), 2..3, "punctuation alone");
        assert_eq!(segment_at(text, 3), 3..4, "the space");
        let beijing = text.find('北').unwrap();
        assert_eq!(segment_at(text, beijing + 3), beijing..beijing + 6);
        assert_eq!(segment_at(text, text.len()), beijing + 6..text.len());
        assert_eq!(segment_at("  ", 0), 0..2, "spaces run together");
        assert_eq!(segment_at("", 0), 0..0);
    }

    #[test]
    fn terms_are_parted_at_spaces_and_inside_a_script_written_without_them() {
        assert_eq!(terms("  open  the settings "), ["open", "the", "settings"]);
        assert_eq!(terms("c++ ctrl+t l'école"), ["c++", "ctrl+t", "l'école"]);
        assert_eq!(terms("設定を開く"), ["設定", "を", "開く"]);
        assert_eq!(terms("我们在北京"), ["我们", "在", "北京"]);
        assert_eq!(terms("เปิดไฟล์"), ["เปิด", "ไฟล์"]);
        assert_eq!(terms("PDF文件"), ["PDF", "文件"]);
        // What is joined to a word stays with it.
        assert_eq!(terms("設定。"), ["設定。"]);
        // An ideographic space parts terms as a space does.
        assert_eq!(terms("北京\u{3000}工作"), ["北京", "工作"]);
        assert_eq!(terms("שלום עולם"), ["שלום", "עולם"]);
        assert!(terms(" \t").is_empty());
    }
}
