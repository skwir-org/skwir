use std::borrow::Cow::Borrowed as B;

use super::{
    Arg, Catalog, Element, Join, Key, Kind, List, Message, Name, PSEUDO, Size, Variant, Words,
    casing, dates, lists, numbers, register, withdraw,
};

// English as `words!` writes it: each array a static of its own.
static ONE_ITEM: [Element; 1] = [Element::Text(B("one item"))];
static ITEMS: [Element; 2] = [Element::Arg(B("count")), Element::Text(B(" items"))];
static COUNTED: [Variant; 2] = [
    Variant {
        key: Key::Word(B("one")),
        value: B(&ONE_ITEM),
    },
    Variant {
        key: Key::Word(B("other")),
        value: B(&ITEMS),
    },
];
static MOVED_ENGLISH: [Element; 3] = [
    Element::Text(B("Moved ")),
    Element::Select {
        on: B("count"),
        variants: B(&COUNTED),
        default: 1,
    },
    Element::Text(B(" to the Trash")),
];
static MOVED: Message = Message {
    domain: "skwir-core-words-test",
    id: "moved",
    english: &MOVED_ENGLISH,
    note: "",
};
static MADE_ENGLISH: [Element; 2] = [Element::Text(B("Made ")), Element::Arg(B("name"))];
static MADE: Message = Message {
    domain: "skwir-core-words-test",
    id: "made",
    english: &MADE_ENGLISH,
    note: "",
};
static KEPT_ENGLISH: [Element; 3] = [
    Element::Arg(B("size")),
    Element::Text(B(" kept: ")),
    Element::Arg(B("what")),
];
static KEPT: Message = Message {
    domain: "skwir-core-words-test",
    id: "kept",
    english: &KEPT_ENGLISH,
    note: "",
};

fn langs(tags: &[&str]) -> Vec<String> {
    tags.iter().map(|t| (*t).to_owned()).collect()
}

/// One test for the registry, which is the whole process's: a plural and
/// a kind in a language that has them, a message it lacks said whole in
/// English, and a catalog withdrawn with its owner.
#[test]
fn a_language_says_its_plurals_and_kinds_and_lacks_nothing_by_halves() {
    let moved = |n: u64, kind: Kind| {
        Words::message(&MOVED, vec![("count", n.into()), ("kind", kind.into())])
    };
    // English, the code's: one, and the rest.
    assert_eq!(
        moved(1, Kind::File).english(),
        "Moved one item to the Trash"
    );
    assert_eq!(moved(3, Kind::File).english(), "Moved 3 items to the Trash");
    assert_eq!(
        moved(1_200, Kind::File).english(),
        "Moved 1,200 items to the Trash"
    );
    register(
        "words-test",
        "skwir-core-words-test",
        Catalog::new(
            "fr",
            "moved = { $count ->\n    [0] Rien de déplacé\n    [one] { $kind ->\n        [folder] Un dossier déplacé\n       *[other] Un élément déplacé\n    }\n   *[other] { $count } éléments déplacés\n} dans la corbeille\n",
        ),
    );
    let fr = langs(&["fr-FR", "en"]);
    // French: one and two are as English, but 0 is said apart, and a
    // folder agrees.
    assert_eq!(
        moved(1, Kind::File).say_in(&fr),
        "Un élément déplacé dans la corbeille"
    );
    assert_eq!(
        moved(1, Kind::Folder).say_in(&fr),
        "Un dossier déplacé dans la corbeille"
    );
    assert_eq!(
        moved(0, Kind::File).say_in(&fr),
        "Rien de déplacé dans la corbeille"
    );
    assert_eq!(
        moved(2, Kind::File).say_in(&fr),
        "2 éléments déplacés dans la corbeille"
    );
    // A message French lacks: whole, in English, its name quoted as
    // English quotes it.
    let made = Words::message(&MADE, vec![("name", Name("it's a.txt".into()).into())]);
    assert_eq!(made.say_in(&fr), "Made “it's a.txt”");
    // Withdrawn with its owner: English again.
    withdraw("words-test");
    assert_eq!(
        moved(2, Kind::File).say_in(&fr),
        "Moved 2 items to the Trash"
    );
}

#[test]
fn a_name_is_quoted_as_its_language_quotes_and_its_controls_shown() {
    let made = |name: &str| Words::message(&MADE, vec![("name", Name(name.into()).into())]);
    assert_eq!(made("a.txt").english(), "Made “a.txt”");
    // The spoof's own name, said in its true order.
    assert_eq!(
        made("Hello_\u{202e}gnp.js").english(),
        "Made “Hello_⟨RLO⟩gnp.js”"
    );
    // Marks that reverse no letters are left as they are.
    assert_eq!(made("a\u{200f}b").english(), "Made “a\u{200f}b”");
    let mut out = String::new();
    super::quoted("fr-CA", "a.txt", &mut out);
    assert_eq!(out, "«\u{202f}a.txt\u{202f}»");
}

#[test]
fn arguments_are_said_as_their_types_say_them() {
    let kept = Words::message(
        &KEPT,
        vec![
            ("size", Size(1_500_000).into()),
            (
                "what",
                Words::message(&MOVED, vec![("count", 1u32.into())]).into(),
            ),
        ],
    );
    assert_eq!(kept.english(), "1.5 MB kept: Moved one item to the Trash");
    // Text as written, in every language.
    assert_eq!(
        Words::as_written("rclone: 403").say_in(&langs(&["fr"])),
        "rclone: 403"
    );
    // A missing argument is said so, never dropped.
    assert_eq!(Words::message(&MADE, Vec::new()).english(), "Made {$name}");
    // Out of the process, its English.
    assert_eq!(
        serde_json::to_value(&kept).unwrap(),
        "1.5 MB kept: Moved one item to the Trash"
    );
    let back: Words = serde_json::from_value(serde_json::json!("Écrit")).unwrap();
    assert_eq!(back, Words::as_written("Écrit"));
}

#[test]
fn the_pseudo_language_is_english_accented_longer_and_marked() {
    let made = Words::message(&MADE, vec![("name", Name("a.txt".into()).into())]);
    assert_eq!(made.say_in(&langs(&[PSEUDO, "en"])), "[Máádéé ‹“a.txt”›]");
}

#[test]
fn plurals_are_cldr_s() {
    use super::plurals::category;
    assert_eq!(category("en", 1), "one");
    assert_eq!(category("en", 0), "other");
    assert_eq!(category("fr", 0), "one");
    assert_eq!(category("pl", 2), "few");
    assert_eq!(category("pl", 5), "many");
    assert_eq!(category("ar", 0), "zero");
    assert_eq!(category("not a language", 1), "other");
}

#[test]
fn an_argument_s_type_is_its_own() {
    assert_eq!(Arg::from(3usize), Arg::Count(3));
    assert_eq!(Arg::from(Kind::Folder), Arg::Kind("folder"));
    assert_eq!(
        Arg::from(Name::of(std::path::Path::new("/a/b.txt"))),
        Arg::Name("b.txt".into())
    );
}

/// What `words!` builds: the English read as Fluent's syntax at compile
/// time, its arguments checked against it, said as any message is.
#[test]
fn words_written_beside_their_id_are_messages() {
    let moved = |n: usize| {
        crate::words!(
            test_moved = "Moved { $count ->
    [one] one item
   *[other] { $count } items
} to the Trash",
            count = n
        )
    };
    assert_eq!(moved(1).english(), "Moved one item to the Trash");
    assert_eq!(moved(4).english(), "Moved 4 items to the Trash");
    let message = moved(1).message_of().unwrap();
    assert_eq!((message.domain, message.id), ("skwir-core", "test_moved"));
    let named = crate::words!(
        test_named = "{ $name } is not { $kind ->
            [folder] a folder
           *[other] a file
        }",
        name = Name("a.txt".into()),
        kind = Kind::Folder,
        note = "Said of what a step found where it looked."
    );
    assert_eq!(named.english(), "“a.txt” is not a folder");
    assert_eq!(
        named.message_of().unwrap().note,
        "Said of what a step found where it looked."
    );
    assert_eq!(
        crate::words!(test_plain = "Nothing here").english(),
        "Nothing here"
    );
    assert_eq!(
        crate::words!(test_brace = "A {\"{\"} and a {\"}\"}").english(),
        "A { and a }"
    );
}

#[test]
fn a_folder_is_named_bare_its_controls_shown() {
    use super::Folder;
    assert_eq!(
        Arg::from(Folder::of(std::path::Path::new("/a/Docs/b.txt"))),
        Arg::Folder("Docs".into())
    );
    let mut out = String::new();
    super::said_arg(&Arg::Folder("x\u{202e}y".into()), "en", &[], &mut out);
    assert_eq!(out, "x⟨RLO⟩y");
}

/// An error's chain is said link by link in the reader's language: a link
/// written with `words!` and a system's error by its kind; any other as it
/// is; English to logs and agents.
#[test]
fn an_error_is_said_link_by_link_in_the_reader_s_language() {
    let error = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        .context(crate::words!(
            test_unreadable = "“{ $what }” could not be read",
            what = "notes"
        ))
        .context("the plan stopped");
    let said = Words::of_error(error);
    assert_eq!(
        said.english(),
        "the plan stopped: “notes” could not be read: permission denied"
    );
    // Text that is no message is as written; a message and the system's
    // words are said, here in the pseudo-language.
    assert_eq!(
        said.say_in(&langs(&[PSEUDO, "en"])),
        "the plan stopped: [“‹notes›” çööüüļd ñööt béé rééáád]: [péérmííššííööñ dééñííééd]"
    );
    // As an error, words are its message, said where it is said.
    let failed: anyhow::Error = crate::words!(test_failed = "It failed").into();
    assert_eq!(failed.to_string(), "It failed");
    assert!(failed.downcast_ref::<Words>().is_some());
}

/// Words holding an error, themselves a link of an error's chain, are said
/// whole: the nested chain is said in the same languages.
#[test]
fn an_error_in_words_in_an_error_is_said() {
    let inner = anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
    let outer = anyhow::Error::new(crate::words!(
        test_tool_failed = "the tool failed: { $error }",
        error = inner
    ))
    .context("trying the next");
    let said = Words::of_error(outer);
    assert_eq!(
        said.english(),
        "trying the next: the tool failed: permission denied"
    );
    assert_eq!(
        said.say_in(&langs(&[PSEUDO, "en"])),
        "trying the next: [tĥéé tööööļ fááííļééd: ‹[péérmííššííööñ dééñííééd]›]"
    );
}

/// A heading's first letter is cased by the reader's language, past what
/// is no letter, the rest as it is.
#[test]
fn a_heading_begins_as_its_language_begins_a_sentence() {
    let title = crate::words!(
        test_title = "copy { $what } to Docs",
        what = Name("a.txt".into())
    );
    assert_eq!(title.heading().english(), "Copy “a.txt” to Docs");
    assert_eq!(casing::heading("tr", "istanbul"), "İstanbul");
    assert_eq!(casing::heading("en", "istanbul"), "Istanbul");
    // A name keeps its case: past a quote, nothing is cased.
    assert_eq!(
        casing::heading("en", "“photos.zip” first"),
        "“photos.zip” first"
    );
    assert_eq!(casing::heading("ja", "ファイル"), "ファイル");
    // In the pseudo-language, its bracket comes first: nothing is cased.
    let said = title.heading().say_in(&langs(&[PSEUDO, "en"]));
    assert!(said.starts_with("[ç"), "{said}");
}

/// A list is joined as its language joins one.
#[test]
fn a_list_is_joined_by_its_language() {
    let names = || List::and(["a", "b", "c"].map(|n| Arg::Name(n.into())));
    let said = crate::words!(test_list = "Tagged { $names }", names = names());
    assert_eq!(said.english(), "Tagged “a”, “b”, and “c”");
    assert_eq!(
        lists::joined("fr", Join::And, &["a".into(), "b".into(), "c".into()]),
        "a, b et c"
    );
    assert_eq!(
        lists::joined("en", Join::Or, &["a".into(), "b".into()]),
        "a or b"
    );
    assert_eq!(lists::joined("en", Join::And, &["a".into()]), "a");
}

/// A sentence is cut around one of its arguments, the language placing it.
#[test]
fn a_sentence_is_cut_around_an_argument() {
    let held = crate::words!(
        test_held = "Held by “{ $title }” until it ends",
        title = Words::as_written("copy")
    );
    assert_eq!(
        held.say_around("title"),
        Some(("Held by “".to_owned(), "” until it ends".to_owned()))
    );
    assert_eq!(held.say_around("nothing"), None);
}

/// Words sent as data come back as the words they are: said in the
/// reader's language where a catalog has them, their English otherwise;
/// text as written stays text.
#[test]
fn words_sent_as_data_come_back_as_themselves() {
    let hello = crate::words!(
        test_carried =
            "Hello { $who }, { $count ->\n    [one] one file\n   *[other] { $count } files\n}",
        who = Name("Ana".into()),
        count = 3
    );
    let data = hello.to_data();
    let back: Words = serde_json::from_value(data.clone()).unwrap();
    assert_eq!(back.english(), "Hello “Ana”, 3 files");
    assert_eq!(
        serde_json::to_value(&back).unwrap(),
        serde_json::json!("Hello “Ana”, 3 files")
    );
    assert_eq!(back.to_data(), data);
    register(
        "words-test",
        env!("CARGO_PKG_NAME"),
        Catalog::new(
            "fr",
            "test_carried = Bonjour { $who }, { $count ->\n    [one] un fichier\n   *[other] { $count } fichiers\n}\n",
        ),
    );
    assert_eq!(
        back.say_in(&langs(&["fr", "en"])),
        "Bonjour «\u{202f}Ana\u{202f}», 3 fichiers"
    );
    withdraw("words-test");
    let heading: Words = serde_json::from_value(hello.heading().to_data()).unwrap();
    assert_eq!(heading.english(), "Hello “Ana”, 3 files");
    let text: Words = serde_json::from_value(serde_json::json!("as an agent wrote it")).unwrap();
    assert_eq!(text.english(), "as an agent wrote it");
}

/// Every translated catalog of the workspace (`crates/**/l10n/<lang>.ftl`)
/// says what its crate's English says: each message one the English has,
/// in the part of Fluent Skwir speaks, naming only the arguments the
/// English gives. How much of each language is there, and how much is
/// still a draft (a message whose comment says `draft`), is printed.
#[test]
fn every_catalog_says_what_its_english_says() {
    use std::collections::{BTreeMap, BTreeSet};

    fn named(elements: &[Element], into: &mut BTreeSet<String>) {
        for element in elements {
            match element {
                Element::Text(_) => {}
                Element::Arg(name) => {
                    into.insert(name.to_string());
                }
                Element::Select { on, variants, .. } => {
                    into.insert(on.to_string());
                    for variant in variants.iter() {
                        named(&variant.value, into);
                    }
                }
            }
        }
    }

    /// Each message of `source`: its id, the arguments it names, whether it
    /// is a draft; or why it cannot be read.
    fn messages(source: &str) -> Result<BTreeMap<String, (BTreeSet<String>, bool)>, Vec<String>> {
        let resource = match fluent_syntax::parser::parse(source) {
            Ok(resource) => resource,
            Err((_, errors)) => return Err(errors.iter().map(|e| format!("{e:?}")).collect()),
        };
        let mut found = BTreeMap::new();
        let mut problems = Vec::new();
        for entry in resource.body {
            let fluent_syntax::ast::Entry::Message(message) = entry else {
                continue;
            };
            let id = message.id.name.to_owned();
            let draft = message
                .comment
                .as_ref()
                .is_some_and(|comment| comment.content.iter().any(|line| line.trim() == "draft"));
            let Some(value) = &message.value else {
                problems.push(format!("{id}: says nothing"));
                continue;
            };
            if !message.attributes.is_empty() {
                problems.push(format!("{id}: has attributes"));
            }
            match super::pattern::from_fluent(value) {
                Ok(elements) => {
                    let mut names = BTreeSet::new();
                    named(&elements, &mut names);
                    found.insert(id, (names, draft));
                }
                Err(why) => problems.push(format!("{id}: {why}")),
            }
        }
        if problems.is_empty() {
            Ok(found)
        } else {
            Err(problems)
        }
    }

    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut folders = Vec::new();
    let mut walk = vec![crates];
    while let Some(dir) = walk.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if !path.is_dir() || name == "target" || name == "src" || name == "tests" {
                continue;
            }
            if name == "l10n" {
                folders.push(path);
            } else {
                walk.push(path);
            }
        }
    }
    let mut problems = Vec::new();
    let mut said: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for folder in folders {
        let english = std::fs::read_to_string(folder.join("en.ftl")).unwrap_or_default();
        let english = match messages(&english) {
            Ok(english) => english,
            Err(why) => {
                problems.push(format!("{}: en.ftl: {why:?}", folder.display()));
                continue;
            }
        };
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            let Some(language) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if language == "en" || path.extension().is_none_or(|e| e != "ftl") {
                continue;
            }
            let translated = match messages(&std::fs::read_to_string(&path).unwrap()) {
                Ok(translated) => translated,
                Err(why) => {
                    problems.extend(why.into_iter().map(|w| format!("{}: {w}", path.display())));
                    continue;
                }
            };
            let tally = said.entry(language.to_owned()).or_default();
            tally.0 += english.len();
            for (id, (names, draft)) in &translated {
                let Some((given, _)) = english.get(id) else {
                    problems.push(format!(
                        "{}: {id}: the English has no such message",
                        path.display()
                    ));
                    continue;
                };
                for name in names.difference(given) {
                    problems.push(format!(
                        "{}: {id}: names ${name}, which the English gives not",
                        path.display()
                    ));
                }
                tally.1 += 1;
                if *draft {
                    tally.2 += 1;
                }
            }
        }
    }
    for (language, (english, translated, drafts)) in &said {
        eprintln!("{language}: {translated} of {english} messages, {drafts} still drafts");
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Counts, decimals and sizes are written as the reader's language writes
/// numbers: English groups with commas, French with narrow spaces and a
/// decimal comma.
#[test]
fn numbers_are_written_as_the_language_writes_them() {
    assert_eq!(numbers::grouped("en", 1_234_567), "1,234,567");
    assert_eq!(numbers::grouped("fr", 1_234_567), "1\u{202f}234\u{202f}567");
    assert_eq!(numbers::grouped("de", 1_234), "1.234");
    assert_eq!(numbers::decimal("en", 15, 1), "1.5");
    assert_eq!(numbers::decimal("fr", 15, 1), "1,5");
    assert_eq!(numbers::decimal("en", 10, 1), "1.0");
    let size = |bytes| crate::fs::size_words_in(bytes, crate::fs::SizeUnits::Decimal);
    assert_eq!(size(999).english(), "999 B");
    assert_eq!(size(1_500).english(), "1.5 kB");
    assert_eq!(size(5_242_880).english(), "5.2 MB");
    assert_eq!(
        crate::fs::size_words_in(1_536, crate::fs::SizeUnits::Binary).english(),
        "1.5 KiB"
    );
    // A message French has not is said whole in English, its numbers too:
    // never an English sentence with French numbers.
    let moved = crate::words!(test_moved_n = "Moved { $count } files", count = 12_345);
    assert_eq!(moved.say_in(&langs(&["fr", "en"])), "Moved 12,345 files");
}

/// Moments are written as the dates' locale writes them; numbers and dates
/// follow the system's locales in the window, English's for agents.
#[test]
fn moments_are_written_as_the_locale_writes_dates() {
    // 2024-03-12 14:02:05 UTC, written in UTC.
    // SAFETY: a test of its own process, before any thread reads TZ.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("TZ", "UTC");
    }
    let at = |shape| super::Moment {
        at: 1_710_252_125,
        shape,
    };
    let fr = |shape| dates::written("fr", at(shape));
    let gb = |shape| dates::written("en-GB", at(shape));
    assert_eq!(gb(super::Shape::Time), "14:02");
    assert_eq!(fr(super::Shape::Time), "14:02");
    assert_eq!(gb(super::Shape::DayMonth), "12 Mar");
    assert_eq!(fr(super::Shape::DayMonth), "12 mars");
    assert_eq!(fr(super::Shape::Date), "12 mars 2024");
    assert_eq!(fr(super::Shape::MonthYear), "mars 2024");
    assert_eq!(
        dates::written("en-US", at(super::Shape::Time)),
        "2:02\u{202f}PM"
    );
    let today = crate::words!(
        test_today = "Today { $time }",
        time = at(super::Shape::Time)
    );
    assert_eq!(today.english(), "Today 2:02\u{202f}PM");
    crate::locale::set_formats(Some("fr-FR".into()), Some("fr-FR".into()));
    let moved = crate::words!(test_moved_many = "Moved { $count } files", count = 12_345);
    assert_eq!(moved.say(), "Moved 12\u{202f}345 files");
    assert_eq!(today.say(), "Today 14:02");
    assert_eq!(moved.english(), "Moved 12,345 files");
    crate::locale::set_formats(None, None);
}

/// A period's day, year and month are written on Skwir's calendar in the
/// reader's words, the day given whatever the zone: a Persian reader's
/// March 2024 is the Gregorian month, named in Persian, never the Persian
/// month it began in; a Thai reader's year keeps its own count.
#[test]
fn periods_are_written_on_the_calendar_that_counts_them() {
    // Fourteen hours ahead of UTC: noon there is still the day given.
    // SAFETY: a test of its own process, before any thread reads TZ.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("TZ", "Pacific/Kiritimati");
    }
    let on = |shape| super::Moment::on(2024, 3, 1, shape).unwrap();
    let en = |shape| dates::written("en", on(shape));
    assert_eq!(en(super::Shape::Date), "Mar 1, 2024");
    assert_eq!(en(super::Shape::DateLong), "March 1, 2024");
    assert_eq!(en(super::Shape::MonthYear), "March 2024");
    assert_eq!(en(super::Shape::Month), "March");
    assert_eq!(en(super::Shape::MonthShort), "Mar");
    assert_eq!(en(super::Shape::Year), "2024");
    assert_eq!(en(super::Shape::Day), "1");
    assert_eq!(
        dates::written("en-GB", on(super::Shape::DateLong)),
        "1 March 2024"
    );
    assert_eq!(
        dates::written("fr", on(super::Shape::DateLong)),
        "1 mars 2024"
    );
    assert_eq!(dates::written("fr", on(super::Shape::MonthShort)), "mars");
    // Persian: the Gregorian month in its words, not Esfand 1402.
    for shape in [
        super::Shape::MonthYear,
        super::Shape::Year,
        super::Shape::Day,
    ] {
        assert_eq!(
            dates::written("fa", on(shape)),
            dates::written("fa-u-ca-gregory", on(shape)),
            "{shape:?}"
        );
    }
    // A date is a date in any calendar: the locale's own, the Persian
    // (11 Esfand 1402), which is not the Gregorian.
    assert_eq!(
        dates::written("fa", on(super::Shape::Date)),
        dates::written("fa-u-ca-persian", on(super::Shape::Date))
    );
    assert_ne!(
        dates::written("fa", on(super::Shape::Date)),
        dates::written("fa-u-ca-gregory", on(super::Shape::Date))
    );
    // Thai: Buddhist years begin with the Gregorian's, counted from 543 BC.
    assert!(
        dates::written("th-TH", on(super::Shape::Year)).contains("2567"),
        "{}",
        dates::written("th-TH", on(super::Shape::Year))
    );
    // A date there is not is none.
    assert_eq!(super::Moment::on(2026, 2, 31, super::Shape::Date), None);
}

/// Shipped catalogs are registered by their crate's owner: the program's
/// at once, a plugin's by its crate's name either way it is spelt, and
/// taken back with the plugin.
#[test]
fn shipped_catalogs_come_and_go_with_their_crate() {
    static SHIPPED: [super::Shipped; 2] = [
        super::Shipped {
            domain: "skwir-plugin-sample",
            language: "fr",
            source: "sample_title = Échantillon\n",
        },
        super::Shipped {
            domain: "skwir-sample-core",
            language: "fr",
            source: "core_title = Cœur\n",
        },
    ];
    super::ship(&SHIPPED);
    super::register_shipped_of_program("skwir");
    assert_eq!(super::languages_of("skwir-sample-core"), ["fr"]);
    assert!(super::languages_of("skwir-plugin-sample").is_empty());
    super::register_shipped("sample", "skwir_plugin_sample");
    assert_eq!(super::languages_of("skwir-plugin-sample"), ["fr"]);
    withdraw("sample");
    assert!(super::languages_of("skwir-plugin-sample").is_empty());
    assert_eq!(super::languages_of("skwir-sample-core"), ["fr"]);
    // A plugin from outside answers its host's crate: the program's stays.
    super::register_shipped("outside", "skwir_sample_core");
    withdraw("outside");
    assert_eq!(super::languages_of("skwir-sample-core"), ["fr"]);
}

/// A sentence is cut around several arguments, each in the order the
/// language places it, with what follows it.
#[test]
fn a_sentence_is_cut_around_several_arguments() {
    let facts = crate::words!(
        test_facts = "{ $kind } · { $size } · { $when }",
        kind = Words::as_written("PDF"),
        size = Words::as_written("1 MB"),
        when = Words::as_written("today")
    );
    assert_eq!(
        facts.say_apart(&["when", "kind", "size"]),
        Some((
            String::new(),
            vec![
                (1, " · ".to_owned()),
                (2, " · ".to_owned()),
                (0, String::new())
            ]
        ))
    );
    assert_eq!(facts.say_apart(&["kind", "nothing"]), None);
}
