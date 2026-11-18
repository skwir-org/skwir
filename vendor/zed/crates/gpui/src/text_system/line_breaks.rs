//! Where a line may wrap: the Unicode line breaking algorithm (UAX #14) in
//! full, as ICU4X's line segmenter answers it. No line starts with closing
//! punctuation (`。`, `、`, `」`, `）`, `！`, `？`, `…`: Japanese and Chinese
//! kinsoku) nor ends with opening punctuation (`「`, `（`); Thai, Lao, Khmer
//! and Burmese break between their words, found in a dictionary; Tibetan
//! after its tsheg; Chinese, Japanese and Korean between characters, but not
//! before a small kana or a prolonged sound mark (the standard's strict
//! default); a word of any script that puts spaces between its words is
//! kept whole, its marks with their letters, and the joiners, no-break
//! spaces and bidirectional controls (an isolated name, ADR-139's) glue.
//!
//! The wrapped layout (`LineLayout::compute_wrap_boundaries`), its narrowest
//! width (`LineLayout::min_wrap_width`) and the line wrapper
//! (`LineWrapper::wrap_line`, `truncate_wrapped_line`) ask it alike, so they
//! never disagree.
//!
//! One tailoring is gpui's: a line's indent, the spaces it starts with
//! (and those after each newline), is no place to wrap, nor is the place
//! before its first other character, so no row is only an indent (an
//! editor's continuation rows keep it, `IndentAdjustment`).
//!
//! Text all ASCII is answered here, by the rules as they apply to its
//! classes (`ascii`), which give what the segmenter gives (a test compares
//! them); any other goes to the segmenter. The segmenter's data is compiled
//! in, behind one seam (`segmenter`), where a data pack read at run time
//! could take its place.

use collections::FxHashMap;
use icu_segmenter::{LineSegmenter, LineSegmenterBorrowed, options::LineBreakOptions};
use smallvec::{SmallVec, smallvec};
use std::sync::LazyLock;

/// The places a line may wrap at, found once for its text: before each of
/// its byte indices set here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LineBreaks {
    /// A bit for each byte of the text: whether the line may wrap before
    /// it. Inline up to 256 bytes (a file name).
    bits: SmallVec<[u64; 4]>,
}

impl LineBreaks {
    /// Where `text`, one line or several (each newline starts a new one),
    /// may wrap: its opportunities (UAX #14), but in each line's indent.
    pub(crate) fn of(text: &str) -> Self {
        let mut breaks = Self::opportunities(text);
        breaks.keep_indents(text);
        breaks
    }

    /// Each place `text` may wrap at by UAX #14: the byte indices between
    /// its start and its end (neither of which is one).
    fn opportunities(text: &str) -> Self {
        let mut breaks = Self {
            bits: smallvec![0; text.len().div_ceil(64)],
        };
        if text.is_ascii() {
            ascii::opportunities(text.as_bytes(), &mut breaks.bits);
        } else {
            for index in segmenter().segment_str(text) {
                if 0 < index && index < text.len() {
                    breaks.bits[index / 64] |= 1 << (index % 64);
                }
            }
        }
        breaks
    }

    /// Whether a line may wrap before the byte at `index`.
    #[inline]
    pub(crate) fn before(&self, index: usize) -> bool {
        self.bits
            .get(index / 64)
            .is_some_and(|word| (word >> (index % 64)) & 1 == 1)
    }

    /// No wrap in a line's indent, nor before its first character other
    /// than a space.
    fn keep_indents(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let mut start = 0;
        loop {
            let mut index = start;
            while index < bytes.len() && bytes[index] == b' ' {
                index += 1;
            }
            for cleared in start..(index + 1).min(bytes.len()) {
                self.bits[cleared / 64] &= !(1 << (cleared % 64));
            }
            match text[index..].find('\n') {
                Some(offset) => start = index + offset + 1,
                None => break,
            }
        }
    }
}

/// Where texts beyond ASCII that the line wrappers wrapped may wrap, kept
/// by their text (`TextSystem::line_breaks`): a text wrapped again at
/// another width (a grid's names counted again as it is zoomed) is not
/// segmented again. In two generations of `GENERATION` texts each: a text
/// asked for is kept in the current one, and once it is full, it becomes
/// the previous one and what was there goes, so what goes on being asked
/// stays and the rest goes, at most twice `GENERATION` kept. ASCII is not
/// kept: its table of steps is about as quick as a look-up.
#[derive(Default)]
pub(crate) struct KeptBreaks {
    current: FxHashMap<Box<str>, LineBreaks>,
    previous: FxHashMap<Box<str>, LineBreaks>,
}

impl KeptBreaks {
    /// The texts a generation holds.
    pub(crate) const GENERATION: usize = 4096;

    /// `text`'s breaks, if kept (a copy: a name's are a few words, held
    /// inline): moved into the current generation.
    pub(crate) fn get(&mut self, text: &str) -> Option<LineBreaks> {
        if let Some(breaks) = self.current.get(text) {
            return Some(breaks.clone());
        }
        let (text, breaks) = self.previous.remove_entry(text)?;
        self.keep(text, breaks.clone());
        Some(breaks)
    }

    /// Keeps `breaks`, found for `text`, in the current generation.
    pub(crate) fn insert(&mut self, text: &str, breaks: LineBreaks) {
        self.keep(text.into(), breaks);
    }

    fn keep(&mut self, text: Box<str>, breaks: LineBreaks) {
        if self.current.len() >= Self::GENERATION {
            self.previous = std::mem::take(&mut self.current);
        }
        if self.current.capacity() == 0 {
            // A generation's room at once, not grown a text at a time.
            self.current.reserve(Self::GENERATION);
        }
        self.current.insert(text, breaks);
    }

    /// How many texts are kept.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.current.len() + self.previous.len()
    }
}

/// The segmenter that finds where text other than ASCII may wrap: ICU4X's,
/// the rules of UAX #14 and the dictionaries of Thai, Lao, Khmer and
/// Burmese compiled in (`compiled_data`), the standard's strict breaks
/// (`LineBreakStrictness::Strict`, its default) and no locale's tailoring.
/// Where the data comes from is said here alone: a data pack read at run
/// time would be loaded here instead
/// (`LineSegmenter::try_new_dictionary_with_buffer_provider`).
fn segmenter() -> LineSegmenterBorrowed<'static> {
    static SEGMENTER: LazyLock<LineSegmenterBorrowed<'static>> =
        LazyLock::new(|| LineSegmenter::new_dictionary(LineBreakOptions::default()));
    *SEGMENTER
}

/// UAX #14 over text all ASCII, by the classes of its characters
/// (`LineBreak.txt`): what ICU4X's segmenter finds there (the tests compare
/// them on every string of up to four characters' classes and longer ones
/// at random), without its tables. The rules that apply to ASCII ask, of
/// what came before a character, only the class of the last one not a
/// space (a mark read as its base), whether spaces followed it, and whether
/// it was an opening bracket: a state, and a table of steps from each state
/// by each class, worked out from the rules when gpui is built.
mod ascii {
    /// The line breaking classes of ASCII.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    #[repr(u8)]
    enum Class {
        /// Letters, `#&*<=>@^_`~`: alphabetic.
        AL,
        /// Tab, `|`: a break after.
        BA,
        /// Vertical tab, form feed: a mandatory break.
        BK,
        /// `}`: closing punctuation.
        CL,
        /// Other controls: combining marks, as their base.
        CM,
        /// `)`, `]`: closing parentheses.
        CP,
        /// Carriage return.
        CR,
        /// `!`, `?`: exclamation, interrogation.
        EX,
        /// `-`: a hyphen.
        HY,
        /// `,.:;`: infix numeric separators.
        IS,
        /// Line feed.
        LF,
        /// Digits.
        NU,
        /// `([{`: opening punctuation.
        OP,
        /// `%`: a postfix numeric.
        PO,
        /// `$+\`: prefix numerics.
        PR,
        /// `"`, `'`: quotation marks.
        QU,
        /// The space.
        SP,
        /// `/`: a symbol that allows a break after.
        SY,
    }

    use Class::*;

    const ALL: [Class; 18] = [
        AL, BA, BK, CL, CM, CP, CR, EX, HY, IS, LF, NU, OP, PO, PR, QU, SP, SY,
    ];

    const fn class_of(byte: u8) -> Class {
        match byte {
            b'\t' | b'|' => BA,
            b'\n' => LF,
            0x0B | 0x0C => BK,
            b'\r' => CR,
            0x00..=0x1F | 0x7F => CM,
            b' ' => SP,
            b'!' | b'?' => EX,
            b'"' | b'\'' => QU,
            b'$' | b'+' | b'\\' => PR,
            b'%' => PO,
            b'(' | b'[' | b'{' => OP,
            b')' | b']' => CP,
            b'}' => CL,
            b',' | b'.' | b':' | b';' => IS,
            b'-' => HY,
            b'/' => SY,
            b'0'..=b'9' => NU,
            _ => AL,
        }
    }

    /// What a line reads as before a character: the class of the last
    /// character before it (a mark's base's, LB9), as a number, but where
    /// these say otherwise. Never a space's or a mark's.
    type State = u8;
    /// After spaces, the last character before them no opening bracket.
    const SPACES: State = 18;
    /// After an opening bracket and spaces (LB14).
    const SPACES_AFTER_OP: State = 19;
    /// Before the first character.
    const START: State = 20;
    /// A step's bit saying a line may wrap before the character.
    const WRAPS: u16 = 0x8000;

    const fn is(state: State, class: Class) -> bool {
        state == class as u8
    }

    /// Whether a line may wrap between a character of class `before` and
    /// one of class `after`, nothing else between them (no space, no hard
    /// break, no mark). It may only before a letter, a digit, an opening
    /// bracket or a numeric prefix or postfix, and there only after what
    /// allows it.
    const fn between(before: Class, after: Class) -> bool {
        match after {
            // LB13 (× CL, × CP, × EX, × IS, × SY), LB19 (× QU), LB21 (× BA,
            // × HY).
            CL | CP | EX | IS | SY | QU | BA | HY => false,
            AL | NU | OP | PR | PO => match before {
                // LB19 (QU ×), LB14 (OP ×); LB23, LB24, LB28, LB30 (an
                // alphabetic or a digit before a letter, a digit, a numeric
                // prefix or postfix, or an opening bracket), LB25 (NU ×
                // (NU | PO | PR)).
                QU | OP | AL | NU => false,
                // LB31: a break after.
                BA | EX => true,
                // LB25 ((HY | SY) × NU).
                HY | SY => !matches!(after, NU),
                // LB29 (IS × AL), LB25 (IS × NU).
                IS => !matches!(after, AL | NU),
                // LB30 (CP × (AL | NU)), LB25 (CP × (PO | PR)).
                CP => matches!(after, OP),
                // LB25 (CL × (PO | PR)).
                CL => !matches!(after, PO | PR),
                // LB24 ((PR | PO) × AL), LB25 ((PR | PO) × (OP | NU)).
                PR | PO => matches!(after, PR | PO),
                // Never a state.
                BK | CM | CR | LF | SP => false,
            },
            // Answered before.
            BK | CM | CR | LF | SP => false,
        }
    }

    /// Whether a line in `state` may wrap before a character of `class`
    /// (the rules named are UAX #14's).
    const fn wraps(state: State, class: Class) -> bool {
        if state == START {
            // LB2.
            false
        } else if is(state, CR) {
            // LB5: after a carriage return, but before a line feed.
            !matches!(class, LF)
        } else if is(state, BK) || is(state, LF) {
            // LB4, LB5.
            true
        } else if matches!(class, BK | CR | LF | SP) {
            // LB6, LB7.
            false
        } else if state == SPACES {
            // LB13 (× CL, CP, EX, IS, SY), else LB18 (SP ÷); a mark after a
            // space is a letter (LB10).
            !matches!(class, CL | CP | EX | IS | SY)
        } else if state == SPACES_AFTER_OP {
            // LB14 (OP SP* ×).
            false
        } else if matches!(class, CM) {
            // LB9: a mark stays with its base.
            false
        } else {
            between(ALL[state as usize], class)
        }
    }

    /// The state after a character of `class`, read in `state`.
    const fn next(state: State, class: Class) -> State {
        match class {
            SP if is(state, OP) || state == SPACES_AFTER_OP => SPACES_AFTER_OP,
            SP => SPACES,
            // LB9, LB10: a mark reads as its base, or as a letter where it
            // has none.
            CM if state >= SPACES || is(state, BK) || is(state, CR) || is(state, LF) => AL as u8,
            CM => state,
            _ => class as u8,
        }
    }

    /// Each byte's class, then each state's step by each class: where the
    /// next state's row starts (its number times 32), and `WRAPS` where a
    /// line may wrap before the character. Rows of 32, so a step is looked
    /// up without a bound, the row it leads to found by an or.
    static CLASSES: [u8; 128] = {
        let mut classes = [0; 128];
        let mut byte = 0;
        while byte < 128 {
            classes[byte] = class_of(byte as u8) as u8;
            byte += 1;
        }
        classes
    };
    static STEPS: [u16; 32 * 32] = {
        let mut steps = [0; 32 * 32];
        let mut state = 0;
        while state <= START {
            let mut class = 0;
            while class < ALL.len() {
                let wraps = if wraps(state, ALL[class]) { WRAPS } else { 0 };
                steps[(state as usize) << 5 | class] =
                    wraps | (next(state, ALL[class]) as u16) << 5;
                class += 1;
            }
            state += 1;
        }
        steps
    };

    /// Sets in `bits` (one a byte) each place a line of ASCII `text` may
    /// wrap at, as UAX #14 has it.
    pub(super) fn opportunities(text: &[u8], bits: &mut [u64]) {
        let mut row = usize::from(START) << 5;
        for (chunk, word) in text.chunks(64).zip(bits) {
            let mut found = 0;
            for (at, byte) in chunk.iter().enumerate() {
                let class = usize::from(CLASSES[usize::from(byte & 0x7F)]);
                let step = STEPS[(row | class) & 0x3FF];
                found |= u64::from(step >> 15) << at;
                row = usize::from(step & !WRAPS);
            }
            *word = found;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where `text` may wrap, the indent's tailoring left out.
    fn opportunities_of(text: &str) -> Vec<usize> {
        let breaks = LineBreaks::opportunities(text);
        (0..text.len())
            .filter(|index| breaks.before(*index))
            .collect()
    }

    /// What the segmenter finds in `text`, between its ends.
    fn segmented(text: &str) -> Vec<usize> {
        segmenter()
            .segment_str(text)
            .filter(|index| 0 < *index && *index < text.len())
            .collect()
    }

    /// What the ASCII pass finds in `text`.
    fn by_classes(text: &str) -> Vec<usize> {
        assert!(text.is_ascii());
        opportunities_of(text)
    }

    /// `text` in the pieces a line may end after.
    fn pieces(text: &str) -> Vec<&str> {
        let breaks = LineBreaks::of(text);
        let mut pieces = Vec::new();
        let mut start = 0;
        for (index, _) in text.char_indices().skip(1) {
            if breaks.before(index) {
                pieces.push(&text[start..index]);
                start = index;
            }
        }
        pieces.push(&text[start..]);
        pieces
    }

    #[track_caller]
    fn assert_whole(text: &str) {
        assert_eq!(pieces(text), [text], "{text:?}");
    }

    #[test]
    fn ascii_is_answered_as_the_segmenter_answers_it() {
        // Every pair of ASCII characters.
        for first in 0u8..128 {
            for second in 0u8..128 {
                let text = String::from_utf8(vec![first, second]).unwrap();
                assert_eq!(by_classes(&text), segmented(&text), "{text:?}");
            }
        }
        // Every string of up to four characters' classes, one character
        // for each (a mark, a tab, `|`, the hard breaks among them).
        let one_of_each = "\u{1}\t|\n\u{b}\r !\"a$%()},-/1";
        let chars: Vec<char> = one_of_each.chars().collect();
        let mut text = String::new();
        for length in 1..=4u32 {
            for mut n in 0..chars.len().pow(length) {
                text.clear();
                for _ in 0..length {
                    text.push(chars[n % chars.len()]);
                    n /= chars.len();
                }
                assert_eq!(by_classes(&text), segmented(&text), "{text:?}");
            }
        }
        // Longer strings at random: words, numbers, punctuation and spaces.
        let alphabet: Vec<u8> = (0u8..128)
            .chain(b"aaaaeeeiioo     --..,,()11$%/".iter().copied())
            .collect();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut random = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..3000 {
            let length = (random() % 48) as usize + 1;
            let bytes: Vec<u8> = (0..length)
                .map(|_| alphabet[(random() % alphabet.len() as u64) as usize])
                .collect();
            let text = String::from_utf8(bytes).unwrap();
            assert_eq!(by_classes(&text), segmented(&text), "{text:?}");
        }
    }

    /// Where UAX #14 parts from the rules gpui had for ASCII (a word made
    /// of letters, digits and `-_.'$%@#^~,=:;!)]}"`, a break before any
    /// other character but a space), the standard is followed.
    #[test]
    fn ascii_wraps_as_the_standard_has_it() {
        // Kept whole, as before.
        for word in [
            "Hello123",
            "var_name",
            "3.1415",
            "10^2",
            "1~2",
            "100%",
            "@mention",
            "#hashtag",
            "$variable",
            "a=1",
            "Self::is_word_char",
            "on;",
            "plz!",
            "see)",
            "github.com",
            "it's",
            "file_2026-11-18.txt",
            "photo(1).jpg",
        ] {
            assert_whole(word);
        }
        // After a hyphen, not inside the word: "non-|English".
        assert_eq!(pieces("non-English"), ["non-", "English"]);
        // After a slash, not before it.
        assert_eq!(pieces("zed-industries/zed"), ["zed-", "industries/", "zed"]);
        assert_eq!(pieces("a/b/c.txt"), ["a/", "b/", "c.txt"]);
        // After a question mark, not before it.
        assert_eq!(pieces("foo?b=2"), ["foo?", "b=2"]);
        // `&`, `+`, `*`, `<` and `>` are letters and numeric prefixes: no
        // break before them.
        assert_whole("a=1&b=2");
        assert_whole("2+3");
        assert_whole("a<b>c");
        assert_eq!(pieces("C++ code"), ["C+", "+ ", "code"]);
        // Brackets: no break after an opening one, before a closing one,
        // or between a word and its bracket.
        assert_eq!(pieces("a (b) c"), ["a ", "(b) ", "c"]);
        assert_whole("a(b)c");
        assert_eq!(pieces("()[]{}<>"), ["()", "[]", "{}", "<>"]);
        // No break before punctuation, even after a space.
        assert_whole("a !");
        assert_whole("x .5");
        // A number keeps its sign and unit.
        assert_eq!(pieces("x -1"), ["x ", "-1"]);
        assert_whole("$(5");
        // A tab allows a break after it.
        assert_eq!(pieces("a\tb"), ["a\t", "b"]);
    }

    #[test]
    fn an_indent_is_no_place_to_wrap() {
        assert_eq!(pieces("   aaa bb"), ["   aaa ", "bb"]);
        assert_eq!(pieces("aa   bb"), ["aa   ", "bb"]);
        // Each line has its own.
        assert_eq!(
            pieces("a b\n  c d"),
            ["a ", "b\n  c ", "d"],
            "nothing before c"
        );
        assert_whole("     ");
        // The same beyond ASCII.
        assert_eq!(pieces("  été là"), ["  été ", "là"]);
    }

    /// What 0071 told by Unicode properties, kept: every script that puts
    /// spaces between its words keeps them whole, marks with their letters.
    #[test]
    fn a_word_of_any_script_is_kept_whole() {
        for word in [
            // Latin, its accents composed and typed apart, Vietnamese.
            "ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏ",
            "ĀāĂăĄąĆćĈĉĊċČčĎď",
            "ƀƁƂƃƄƅƆƇƈƉƊƋƌƍƎƏ",
            "e\u{301}te\u{301}",
            "ThậmchíđếnkhithuachạychúngcònnhẫntâmgiếtnốtsốđôngtùchínhtrịởYênBáivàCaoBằng",
            "l'été",
            "aujourd'hui",
            "won’t",
            "‘twas",
            "quoted”",
            "well…",
            "more⋯",
            "µ²½ªº",
            "2×3",
            // Cyrillic, Greek, Armenian, Georgian.
            "АБВГДЕЖЗИЙКЛМНОП",
            "Ελληνικά",
            "Հայերեն",
            "ქართული",
            // Hebrew with its points, Arabic with its digits and a vowel.
            "שלום",
            "\u{5e9}\u{5c1}\u{5b8}\u{5dc}\u{5d5}\u{5b9}\u{5dd}",
            "مرحبا",
            "\u{661}\u{662}\u{663}\u{64e}",
            // Devanagari (a virama, vowel signs, digits), Bengali, Tamil.
            "हिन्दी१२३",
            "গিয়েছিলেন",
            "ছেলে",
            "হচ্ছিল",
            "தமிழ்",
            // Ethiopic, Canadian syllabics.
            "አማርኛ",
            "ᐃᓄᒃᑎᑐᑦ",
        ] {
            assert_whole(word);
        }
        assert_eq!(pieces("שלום עולם"), ["שלום ", "עולם"]);
        assert_eq!(pieces("Ελληνικά λέξη"), ["Ελληνικά ", "λέξη"]);
    }

    /// Glue: no break beside a joiner, a no-break space or hyphen, or a
    /// bidirectional control (0071's, the standard's): a name isolated
    /// inside its quotes (ADR-139) wraps as its words and its quotes do.
    #[test]
    fn glue_holds() {
        assert_whole("a\u{202F}b\u{00A0}c\u{2011}d");
        assert_whole("a\u{200d}b\u{2060}c\u{feff}d");
        assert_whole("a\u{200f}b\u{202e}c\u{202c}d");
        assert_whole("\u{2068}name\u{2069}”");
        assert_eq!(
            pieces("“\u{2068}a name\u{2069}” x"),
            ["“\u{2068}a ", "name\u{2069}” ", "x"]
        );
        assert_eq!(
            pieces("a \u{2068}שלום\u{2069} b"),
            ["a ", "\u{2068}שלום\u{2069} ", "b"]
        );
        // French guillemets keep their words, spaces between (LB15a, LB15b).
        assert_eq!(pieces("« Bonjour » dit-il"), ["« Bonjour » ", "dit-", "il"]);
        // An emoji's joined sequence is one.
        assert_eq!(
            pieces("😀😁👨\u{200d}👩\u{200d}👧"),
            ["😀", "😁", "👨\u{200d}👩\u{200d}👧"]
        );
    }

    /// Chinese, Japanese and Korean break between characters, but no line
    /// starts with closing punctuation, a small kana or a prolonged sound
    /// mark, nor ends with opening punctuation (kinsoku).
    #[test]
    fn east_asian_text_breaks_between_characters_but_its_punctuation() {
        assert_eq!(pieces("中文字"), ["中", "文", "字"]);
        assert_eq!(pieces("ㄅㄆ"), ["ㄅ", "ㄆ"]);
        assert_eq!(pieces("ꆈꌠ"), ["ꆈ", "ꌠ"]);
        assert_eq!(pieces("한국어 문장"), ["한", "국", "어 ", "문", "장"]);
        assert_eq!(
            pieces("語。語、本，語」日）日！日？日…日"),
            [
                "語。", "語、", "本，", "語」", "日）", "日！", "日？", "日…", "日"
            ]
        );
        assert_eq!(pieces("日「本（語"), ["日", "「本", "（語"]);
        assert_eq!(
            pieces("「こんにちは」と言った。"),
            ["「こ", "ん", "に", "ち", "は」", "と", "言っ", "た。"]
        );
        assert_eq!(pieces("ちょっとキャー"), ["ちょっ", "と", "キャー"]);
        // A Japanese voiced mark typed apart stays with its kana.
        assert_eq!(pieces("か\u{3099}か"), ["か\u{3099}", "か"]);
    }

    /// Thai, Lao, Khmer and Burmese, written without spaces between their
    /// words, break between words a dictionary knows, never inside one, nor
    /// before a mark (0071 broke them between any two letters); Tibetan
    /// after its tsheg.
    #[test]
    fn scripts_without_spaces_break_between_their_words() {
        assert_eq!(pieces("ข้าวมันไก่อร่อยมาก"), ["ข้าว", "มัน", "ไก่", "อร่อย", "มาก"]);
        assert_eq!(pieces("ขอบคุณครับ"), ["ขอบคุณ", "ครับ"]);
        assert_eq!(pieces("กี่ข"), ["กี่", "ข"]);
        assert_whole("\u{e35}\u{e48}");
        assert_eq!(pieces("ລາວສາມາດເວົ້າ"), ["ລາວ", "ສາມາດ", "ເວົ້າ"]);
        assert_eq!(pieces("ខ្ញុំស្រលាញ់ភាសាខ្មែរ"), ["ខ្ញុំ", "ស្រលាញ់", "ភាសាខ្មែរ"]);
        assert_eq!(pieces("မြန်မာဘာသာစကား"), ["မြန်မာဘာသာ", "စကား"]);
        assert_eq!(pieces("བོད་ཀྱི་སྐད་ཡིག"), ["བོད་", "ཀྱི་", "སྐད་", "ཡིག"]);
    }

    /// Breaks kept by their text stay while they go on being asked for, two
    /// generations at most: one asked for in the previous generation comes
    /// back to the current one; one not asked for in two goes.
    #[test]
    fn breaks_are_kept_by_their_text_for_two_generations() {
        let mut kept = KeptBreaks::default();
        let first = LineBreaks::of("été là");
        kept.insert("été là", first.clone());
        assert_eq!(kept.get("été là"), Some(first.clone()));
        let others = |kept: &mut KeptBreaks, mark: char| {
            for n in 0..KeptBreaks::GENERATION {
                kept.insert(&format!("{mark}{n}"), LineBreaks::default());
            }
        };
        others(&mut kept, 'é');
        assert_eq!(
            kept.get("été là"),
            Some(first),
            "from the previous generation"
        );
        others(&mut kept, 'à');
        others(&mut kept, 'ü');
        assert!(kept.get("été là").is_none(), "two generations without it");
        assert!(kept.len() <= 2 * KeptBreaks::GENERATION);
    }

    #[test]
    fn breaks_lie_between_characters() {
        for text in [
            "日本語ですね",
            "ข้าวมันไก่",
            "été là-bas",
            "a\u{301} b",
            "😀😁",
        ] {
            let found = opportunities_of(text);
            assert!(!found.is_empty(), "{text:?}");
            assert!(
                found.iter().all(|index| text.is_char_boundary(*index)),
                "{text:?}: {found:?}"
            );
        }
        assert!(opportunities_of("").is_empty());
        assert!(LineBreaks::of("").bits.is_empty());
        assert!(!LineBreaks::of("a b").before(99));
    }
}
