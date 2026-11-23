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

/// A message known as the program runs (a plugin from outside's, protocol
/// 2): said from the catalogs registered for its domain, else from the
/// English it was given, else as its id; agents read the English, its
/// arguments said as their types say, and it comes back from its data as
/// the words it is.
#[test]
fn a_message_known_as_the_program_runs_is_said_from_its_domain() {
    const DOMAIN: &str = "outside:words-test";
    let english = Catalog::new(
        "en",
        "copied = { $count ->\n    [one] Copied one word of { $name }\n   *[other] Copied { $count } words of { $name }\n}\nonly-english = Only in English\n",
    );
    let said = |id: &str, args: Vec<(String, Arg)>| {
        Words::of_catalog(DOMAIN, id, english.message(id).as_deref(), args)
    };
    let three = || {
        vec![
            ("count".to_owned(), Arg::Count(3)),
            ("name".to_owned(), Arg::Name("it's.txt".into())),
        ]
    };
    assert_eq!(
        said("copied", three()).english(),
        "Copied 3 words of “it's.txt”"
    );
    register(
        "outside-words-test",
        DOMAIN,
        Catalog::new(
            "fr",
            "copied = { $count ->\n    [one] Un mot de { $name } copié\n   *[other] { $count } mots de { $name } copiés\n}\n",
        ),
    );
    let fr = langs(&["fr", "en"]);
    assert_eq!(
        said("copied", three()).say_in(&fr),
        "3 mots de “it's.txt” copiés"
    );
    assert_eq!(
        said("copied", three()).english(),
        "Copied 3 words of “it's.txt”",
        "agents read the English"
    );
    assert_eq!(
        said("only-english", Vec::new()).say_in(&fr),
        "Only in English"
    );
    assert_eq!(said("nowhere", Vec::new()).say_in(&fr), "nowhere", "its id");
    assert_eq!(said("nowhere", Vec::new()).english(), "nowhere");
    // As data and back (what the window sends itself), still the message.
    let data = said("copied", three()).to_data();
    let back: Words = serde_json::from_value(data).unwrap();
    assert_eq!(back.say_in(&fr), "3 mots de “it's.txt” copiés");
    assert!(
        said("copied", three())
            .say_in(&langs(&[PSEUDO]))
            .starts_with('[')
    );
    withdraw("outside-words-test");
    assert_eq!(
        said("copied", three()).say_in(&fr),
        "Copied 3 words of “it's.txt”",
        "withdrawn: English"
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
    // The quotes are each language's data: core's French catalog says them,
    // and a reader of Canadian French finds it.
    register_french_quotes();
    let quoted = |language: &str| {
        let mut out = String::new();
        super::quoted(language, "a.txt", &mut out);
        out
    };
    assert_eq!(quoted("fr-CA"), "«\u{202f}a.txt\u{202f}»");
    // Two scripts of one language quote apart: no code knows either.
    register(
        "words-test",
        "skwir-core",
        Catalog::new("zh-Hant", "name_quote_open = 「\nname_quote_close = 」\n"),
    );
    register(
        "words-test",
        "skwir-core",
        Catalog::new("zh-Hans", "name_quote_open = “\nname_quote_close = ”\n"),
    );
    assert_eq!(quoted("zh-Hant-TW"), "「a.txt」");
    assert_eq!(quoted("zh-Hans"), "“a.txt”");
    // A language whose catalogs say no quotes quotes as English does.
    assert_eq!(quoted("de"), "“a.txt”");
    assert_eq!(quoted("en"), "“a.txt”");
    // Withdrawn, the quotes go with the catalog.
    withdraw("words-test");
    assert_eq!(quoted("zh-Hant"), "“a.txt”");
}

/// French quotes, as a catalog of core's writes them: guillemets, a narrow
/// no-break space inside each, written as Fluent's escape.
fn register_french_quotes() {
    register(
        "words-test",
        "skwir-core",
        Catalog::new(
            "fr",
            "name_quote_open = «{ \"\\u202F\" }\nname_quote_close = { \"\\u202F\" }»\n",
        ),
    );
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
    assert_eq!(
        kept.english(),
        "1.5\u{a0}MB kept: Moved one item to the Trash"
    );
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
        "1.5\u{a0}MB kept: Moved one item to the Trash"
    );
    let back: Words = serde_json::from_value(serde_json::json!("Écrit")).unwrap();
    assert_eq!(back, Words::as_written("Écrit"));
}

/// Where the window draws it, a name is isolated from its sentence inside
/// its quotes, and a folder named bare is too: a Hebrew name, and one
/// whose RLO would reverse what follows it, reorder nothing around them.
/// English, words said in given languages, text said plainly and data
/// carry no mark.
#[test]
fn a_name_is_isolated_where_the_window_draws_it() {
    const FSI: char = '\u{2068}';
    const PDI: char = '\u{2069}';
    let hebrew = "שלום 2.txt";
    let moved = |name: &str| {
        crate::words!(
            test_isolated_moved = "Moved { $name } to { $folder }",
            name = Name(name.into()),
            folder = super::Folder("מסמכים".into())
        )
    };
    let said = moved(hebrew).say();
    assert_eq!(
        said,
        "Moved “\u{2068}שלום 2.txt\u{2069}” to \u{2068}מסמכים\u{2069}"
    );
    assert_eq!(moved(hebrew).english(), "Moved “שלום 2.txt” to מסמכים");
    // The spoof's control is a tag: the isolate holds none that could end
    // it or reverse the words after it, a PDI of the name's own included.
    assert_eq!(
        moved("Hello_\u{202e}gnp.js").say(),
        "Moved “\u{2068}Hello_⟨RLO⟩gnp.js\u{2069}” to \u{2068}מסמכים\u{2069}"
    );
    assert_eq!(
        moved("Hello_\u{202e}gnp.js").english(),
        "Moved “Hello_⟨RLO⟩gnp.js” to מסמכים"
    );
    let early = moved("a\u{2069}b").say();
    assert_eq!(early.matches(FSI).count(), 2, "{early}");
    assert_eq!(early.matches(PDI).count(), 2, "{early}");
    assert!(early.contains("a⟨PDI⟩b"), "{early}");
    // Nothing but the window's words carries a mark.
    for plain in [
        moved(hebrew).english(),
        moved(hebrew).say_plain(),
        moved(hebrew).say_in(&langs(&["fr", "en"])),
        moved(hebrew).to_string(),
        serde_json::to_string(&moved(hebrew)).unwrap(),
        moved(hebrew).to_data().to_string(),
    ] {
        assert!(!plain.contains([FSI, PDI]), "{plain}");
    }
    assert_eq!(moved(hebrew).say_plain(), moved(hebrew).english());
    // Words sent as data and back are isolated where they are drawn.
    let back: Words = serde_json::from_value(moved(hebrew).to_data()).unwrap();
    assert_eq!(back.say(), said);
    // A name in words in words, each of a list's, an error's: isolated
    // where the window says them, in English never.
    let nested = crate::words!(
        test_isolated_nested = "{ $what }: { $names }",
        what = moved(hebrew),
        names = List::and([Name("א".into()), Name("b".into())])
    );
    assert_eq!(
        nested.say(),
        format!("{said}: “{FSI}א{PDI}” and “{FSI}b{PDI}”")
    );
    let failed = Words::of_error(anyhow::Error::new(moved(hebrew)).context("the plan stopped"));
    assert_eq!(failed.say(), format!("the plan stopped: {said}"));
    assert_eq!(
        failed.english(),
        "the plan stopped: Moved “שלום 2.txt” to מסמכים"
    );
    // The quotes are the sentence's, in every language: outside the
    // isolate, French's narrow spaces with them.
    register_french_quotes();
    let mut out = String::new();
    super::formats::within(
        super::formats::Formats::window().isolating(super::Isolation::Names),
        || super::quoted("fr", hebrew, &mut out),
    );
    assert_eq!(out, "«\u{202f}\u{2068}שלום 2.txt\u{2069}\u{202f}»");
    // A sentence that begins with a folder keeps its case where the window
    // says it: the mark comes first, which no casing changes.
    let full = crate::words!(
        test_isolated_full = "{ $folder } is full",
        folder = super::Folder("docs".into())
    );
    assert_eq!(full.heading().say(), "\u{2068}docs\u{2069} is full");
    // An empty folder's name takes no marks.
    let nowhere = crate::words!(
        test_isolated_nowhere = "Moved to { $folder }",
        folder = super::Folder(String::new())
    );
    assert_eq!(nowhere.say(), "Moved to ");
}

/// Keys inside a sentence are left to right in every language: between
/// LRI and PDI where the window draws them, plain in English, in words said
/// plainly or in given languages, and in data, which brings them back as
/// keys.
#[test]
fn keys_are_isolated_left_to_right_where_the_window_draws_them() {
    const LRI: char = '\u{2066}';
    const PDI: char = '\u{2069}';
    let back_with = || {
        crate::words!(
            test_keys_back = "Back ({ $keys })",
            keys = super::Keys("Alt+←".into())
        )
    };
    let said = back_with().say();
    assert_eq!(said, format!("Back ({LRI}Alt+←{PDI})"));
    for plain in [
        back_with().english(),
        back_with().say_plain(),
        back_with().say_in(&langs(&["fr", "en"])),
        back_with().to_string(),
        serde_json::to_string(&back_with()).unwrap(),
        back_with().to_data().to_string(),
    ] {
        assert!(!plain.contains([LRI, PDI]), "{plain}");
    }
    assert_eq!(back_with().english(), "Back (Alt+←)");
    // As data and back, isolated again where drawn.
    let back: Words = serde_json::from_value(back_with().to_data()).unwrap();
    assert_eq!(back.say(), said);
    assert_eq!(
        back_with().to_data()["args"]["keys"],
        serde_json::json!({ "keys": "Alt+←" })
    );
    // In the right-to-left pseudo-language, the sentence's words are set
    // from the right, the keys whole between their marks.
    let rtl = back_with().say_in(&langs(&[crate::direction::PSEUDO_RTL, "en"]));
    assert!(rtl.contains("‹Alt+←›"), "{rtl}");
    // No keys, no marks.
    let none = crate::words!(
        test_keys_none = "Press { $keys }",
        keys = super::Keys(String::new())
    );
    assert_eq!(none.say(), "Press ");
}

/// A sentence cut around a name hands back parts the drawing shows right:
/// the name cut out leaves its quotes, the sentence's, and no mark
/// isolating it; each other name is isolated whole within one part.
#[test]
fn a_sentence_cut_around_a_name_keeps_its_quotes_not_its_marks() {
    const FSI: char = '\u{2068}';
    const PDI: char = '\u{2069}';
    let moved = crate::words!(
        test_cut_moved = "Moved { $name } to { $folder }",
        name = Name("שלום.txt".into()),
        folder = super::Folder("Docs".into())
    );
    assert_eq!(
        moved.say_around("name"),
        Some(("Moved “".to_owned(), format!("” to {FSI}Docs{PDI}")))
    );
    assert_eq!(
        moved.say_around("folder"),
        Some((format!("Moved “{FSI}שלום.txt{PDI}” to "), String::new()))
    );
    let (before, placed) = moved.say_apart(&["folder", "name"]).unwrap();
    assert_eq!(before, "Moved “");
    assert_eq!(placed, vec![(1, "” to ".to_owned()), (0, String::new())]);
    // Balanced in every part: no part opens an isolate another closes.
    let (before, after) = moved.say_around("name").unwrap();
    for part in [before, after] {
        assert_eq!(
            part.matches(FSI).count(),
            part.matches(PDI).count(),
            "{part}"
        );
    }
}

#[test]
fn the_pseudo_language_is_english_accented_longer_and_marked() {
    let made = Words::message(&MADE, vec![("name", Name("a.txt".into()).into())]);
    assert_eq!(made.say_in(&langs(&[PSEUDO, "en"])), "[Máádéé ‹“a.txt”›]");
    // The right-to-left one is the English, its arguments marked, as the
    // direction draws it right to left.
    let rtl = crate::direction::PSEUDO_RTL;
    assert_eq!(
        made.say_in(&langs(&[rtl, "en"])),
        crate::direction::pseudo_rtl("Made ‹“a.txt”›")
    );
    let back: Words = serde_json::from_value(made.to_data()).unwrap();
    assert_eq!(back.say_in(&langs(&[rtl])), made.say_in(&langs(&[rtl])));
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

/// A path is said as a folder is: bare, its controls shown, isolated where
/// the window draws it, plain elsewhere; as data, the path as it is.
#[test]
fn a_path_is_said_as_a_folder_is() {
    const FSI: char = '\u{2068}';
    const PDI: char = '\u{2069}';
    let path = std::path::PathBuf::from("/a/Hello_\u{202e}gnp.js/מסמכים");
    assert_eq!(
        Arg::from(path.as_path()),
        Arg::Path("/a/Hello_\u{202e}gnp.js/מסמכים".into())
    );
    assert_eq!(Arg::from(&path), Arg::from(path.as_path()));
    let not_made = |path: &std::path::Path| {
        crate::words!(
            test_path_not_made = "the folder { $path } could not be made",
            path = path
        )
    };
    assert_eq!(
        not_made(&path).say(),
        format!("the folder {FSI}/a/Hello_⟨RLO⟩gnp.js/מסמכים{PDI} could not be made")
    );
    for plain in [not_made(&path).english(), not_made(&path).say_plain()] {
        assert_eq!(
            plain,
            "the folder /a/Hello_⟨RLO⟩gnp.js/מסמכים could not be made"
        );
    }
    let data = not_made(&path).to_data();
    assert_eq!(
        data["args"]["path"],
        serde_json::json!({ "path": "/a/Hello_\u{202e}gnp.js/מסמכים" })
    );
    let back: Words = serde_json::from_value(data).unwrap();
    assert_eq!(back.say(), not_made(&path).say());
}

/// A name drawn alone (a row, a tile, a tab, the trail, a title): its
/// controls shown in every saying, isolated where the window draws it,
/// nothing else added; as data, the name as it is; in a sentence, isolated
/// once.
#[test]
fn a_name_alone_shows_its_controls_and_nothing_else() {
    const FSI: char = '\u{2068}';
    const PDI: char = '\u{2069}';
    let spoof = Words::name_alone("hello_\u{202e}gnp.js");
    assert_eq!(spoof.say(), format!("{FSI}hello_⟨RLO⟩gnp.js{PDI}"));
    for plain in [
        spoof.say_plain(),
        spoof.english(),
        spoof.say_in(&langs(&["fr", "en"])),
        spoof.to_string(),
        spoof.heading().say_plain(),
    ] {
        assert_eq!(plain, "hello_⟨RLO⟩gnp.js");
    }
    assert_eq!(spoof.heading(), spoof, "a name is never cased");
    assert_eq!(Words::name_alone("שלום").say(), format!("{FSI}שלום{PDI}"));
    assert!(Words::name_alone("").say().is_empty());
    assert!(Words::name_alone("").is_empty());
    let data = spoof.to_data();
    assert_eq!(data, serde_json::json!({ "alone": "hello_\u{202e}gnp.js" }));
    let back: Words = serde_json::from_value(data).unwrap();
    assert_eq!(back, spoof);
    let in_sentence = crate::words!(
        test_alone_in_sentence = "Opened { $place }",
        place = spoof.clone()
    );
    assert_eq!(
        in_sentence.say(),
        format!("Opened {FSI}hello_⟨RLO⟩gnp.js{PDI}")
    );
    assert_eq!(in_sentence.english(), "Opened hello_⟨RLO⟩gnp.js");
}

/// Words of someone else's making that are a name (a plugin's title):
/// said as a name alone, in a sentence isolated once, as a whole, the
/// marks of what they hold never shown as tags.
#[test]
fn words_that_are_a_name_show_their_controls() {
    const FSI: char = '\u{2068}';
    const PDI: char = '\u{2069}';
    let title = Words::shown(Words::as_written("Hex\u{202e}"));
    let off = crate::words!(
        test_shown_off = "The plugin { $plugin } was turned off",
        plugin = title.clone()
    );
    assert_eq!(
        off.say(),
        format!("The plugin {FSI}Hex⟨RLO⟩{PDI} was turned off")
    );
    assert_eq!(off.english(), "The plugin Hex⟨RLO⟩ was turned off");
    // A message holding a name: isolated whole, the name's marks not shown.
    let named = Words::shown(crate::words!(
        test_shown_named = "Viewer of { $name }",
        name = Name("a.txt".into())
    ));
    assert_eq!(named.say(), format!("{FSI}Viewer of “a.txt”{PDI}"));
    assert_eq!(Words::shown(named.clone()), named, "shown once");
    let back: Words = serde_json::from_value(named.to_data()).unwrap();
    assert_eq!(back.say(), named.say());
}

/// Words that name something (a plan's title) are quoted as a name is:
/// by the sentence's language, their controls shown, isolated whole where
/// the window draws them; as data, the words they are.
#[test]
fn named_words_are_quoted_as_a_name_is() {
    const FSI: char = '\u{2068}';
    const PDI: char = '\u{2069}';
    let title = || crate::words!(test_named_title = "copy { $n } items", n = 3);
    let held = |title: Words| {
        crate::words!(
            test_named_held = "Held by { $title } until it ends",
            title = super::Named(title)
        )
    };
    assert_eq!(
        held(title()).english(),
        "Held by “copy 3 items” until it ends"
    );
    assert_eq!(
        held(title()).say(),
        format!("Held by “{FSI}copy 3 items{PDI}” until it ends")
    );
    // An agent's title: its controls shown, never obeyed.
    let spoof = held(Words::as_written("copy\u{202e}smeti"));
    assert_eq!(spoof.english(), "Held by “copy⟨RLO⟩smeti” until it ends");
    // A name inside: isolated once, whole, its own marks left out.
    let inside = held(crate::words!(
        test_named_inside = "copy { $name }",
        name = Name("a.txt".into())
    ));
    assert_eq!(
        inside.say(),
        format!("Held by “{FSI}copy “a.txt”{PDI}” until it ends")
    );
    // Quoted by the sentence's language, the title said in it first.
    register(
        "words-test",
        env!("CARGO_PKG_NAME"),
        Catalog::new(
            "fr",
            "name_quote_open = «{ \"\\u202F\" }\nname_quote_close = { \"\\u202F\" }»\ntest_named_held = Retenu par { $title } jusqu’à sa fin\ntest_named_title = copier { $n } éléments\n",
        ),
    );
    assert_eq!(
        held(title()).say_in(&langs(&["fr", "en"])),
        "Retenu par «\u{202f}copier 3 éléments\u{202f}» jusqu’à sa fin"
    );
    let back: Words = serde_json::from_value(held(title()).to_data()).unwrap();
    assert_eq!(
        back.say_in(&langs(&["fr", "en"])),
        held(title()).say_in(&langs(&["fr", "en"]))
    );
    withdraw("words-test");
    // Cut around, it leaves its quotes, as a name does.
    assert_eq!(
        held(title()).say_around("title"),
        Some(("Held by “".to_owned(), "” until it ends".to_owned()))
    );
}

/// A moment before 1970 is one like any other, said as its date (a scan
/// of an old photograph, a file a tool dated 0): only a time no calendar
/// can say is none.
#[test]
fn a_moment_before_1970_is_said() {
    use std::time::{Duration, UNIX_EPOCH};
    // Noon: the same day in every time zone a test runs in.
    let noon = jiff::civil::date(1965, 6, 15)
        .at(12, 0, 0, 0)
        .to_zoned_datetime(jiff::tz::TimeZone::UTC)
        .unwrap()
        .timestamp();
    let when = std::time::SystemTime::try_from(noon).unwrap();
    let moment = super::Moment::of(when, super::Shape::Date).expect("a date");
    assert_eq!(moment.at, noon.as_second());
    let said = crate::words!(test_moment_old = "made { $when }", when = moment);
    assert_eq!(said.english(), "made Jun 15, 1965");
    // Half a second before 1970 is in its last second, not the first after.
    let just_before = UNIX_EPOCH - Duration::from_millis(500);
    assert_eq!(
        super::Moment::of(just_before, super::Shape::Time).map(|m| m.at),
        Some(-1)
    );
    assert_eq!(
        super::Moment::of(UNIX_EPOCH, super::Shape::Time).map(|m| m.at),
        Some(0)
    );
    // Past what a calendar says.
    let ages_ago = UNIX_EPOCH.checked_sub(Duration::from_secs(400_000_000_000));
    assert!(ages_ago.is_none_or(|t| super::Moment::of(t, super::Shape::Date).is_none()));
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
    // Cased as the language its words are said in: English's, where the
    // reader's lacks them (a Turkish reader's i keeps no dot in English).
    let import = crate::words!(test_import = "import { $n } files", n = 3);
    assert_eq!(
        import.heading().say_in(&langs(&["tr", "en"])),
        "Import 3 files"
    );
    // Words it places first are cased as they begin one; an argument that
    // is no words of Skwir's never is, in the window and plainly alike.
    let failed = crate::words!(test_plan_failed = "{ $plan } failed", plan = import.clone());
    assert_eq!(failed.heading().english(), "Import 3 files failed");
    let full = crate::words!(
        test_heading_full = "{ $folder } is full",
        folder = super::Folder("docs".into())
    );
    assert_eq!(full.heading().say_plain(), "docs is full");
    assert_eq!(full.heading().english(), "docs is full");
    let typed = crate::words!(test_heading_typed = "{ $text } not found", text = "abc");
    assert_eq!(typed.heading().english(), "abc not found");
}

/// A sentence holds words of its own language, whole (ADR-139, 4): where
/// the reader's catalog lacks the sentence, the words it places are said
/// in English too, though the catalog has them; where it has the sentence,
/// in its language first.
#[test]
fn a_sentence_holds_words_of_its_own_language() {
    let bin = || crate::words!(test_whole_bin = "Trash", noun);
    let moved = || {
        crate::words!(
            test_whole_moved = "Moved { $n } items to { $place }",
            n = 3,
            place = bin()
        )
    };
    let size = || {
        crate::words!(
            test_whole_size = "Freed { $size }",
            size = super::Size(1500)
        )
    };
    register(
        "words-test",
        env!("CARGO_PKG_NAME"),
        Catalog::new("fr", "test_whole_bin = Corbeille\n"),
    );
    let fr = langs(&["fr", "en"]);
    assert_eq!(moved().say_in(&fr), "Moved 3 items to Trash");
    assert_eq!(bin().say_in(&fr), "Corbeille", "alone, the reader's");
    register(
        "words-test",
        env!("CARGO_PKG_NAME"),
        Catalog::new(
            "fr",
            "test_whole_bin = Corbeille\ntest_whole_moved = { $n } éléments mis dans { $place }\n",
        ),
    );
    assert_eq!(moved().say_in(&fr), "3 éléments mis dans Corbeille");
    // A reader of two languages: the sentence's own first for what it
    // places, though another comes first to the reader.
    register(
        "words-test",
        env!("CARGO_PKG_NAME"),
        Catalog::new("de", "test_whole_bin = Papierkorb\n"),
    );
    assert_eq!(
        moved().say_in(&langs(&["de", "fr", "en"])),
        "3 éléments mis dans Corbeille",
        "said in French, never Papierkorb"
    );
    withdraw("words-test");
    assert_eq!(size().english(), "Freed 1.5\u{a0}kB");
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
    // Said from the English it carries, as the message is: in the
    // pseudo-language, its arguments marked.
    assert_eq!(
        back.say_in(&langs(&[PSEUDO, "en"])),
        hello.say_in(&langs(&[PSEUDO, "en"]))
    );
    register(
        "words-test",
        env!("CARGO_PKG_NAME"),
        Catalog::new(
            "fr",
            "name_quote_open = «{ \"\\u202F\" }\nname_quote_close = { \"\\u202F\" }»\ntest_carried = Bonjour { $who }, { $count ->\n    [one] un fichier\n   *[other] { $count } fichiers\n}\n",
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

/// What the catalogs' test reads of a catalog: each message, by its id.
mod reading {
    use std::collections::{BTreeMap, BTreeSet};

    use super::super::{Element, Key, grammar};

    /// The words beside each argument of a message: the arguments joined
    /// to them by a no-break space (or to a percent sign, with none), and
    /// those parted from them by a plain one, each with the side (`<` the
    /// words before, `>` those after).
    #[derive(Default)]
    pub struct Follows {
        pub joined: BTreeSet<String>,
        pub parted: BTreeSet<String>,
    }

    /// A message of a catalog, as the test holds it to its English.
    pub struct Read {
        /// The arguments it names.
        pub names: BTreeSet<String>,
        /// Those it chooses a variant by, as a plural's count or a kind:
        /// never a noun.
        pub chosen_by: BTreeSet<String>,
        /// Whether its comment says `draft`.
        pub draft: bool,
        /// Whether its comment says `noun` (`words!(…, noun)`, in English).
        pub noun: bool,
        pub follows: Follows,
        pub elements: Vec<Element>,
        /// Its attributes (a noun's gender and forms), by name, in order.
        pub attributes: Vec<(String, Vec<Element>)>,
    }

    fn follows(elements: &[Element], into: &mut Follows) {
        const NO_BREAK: [char; 2] = ['\u{a0}', '\u{202f}'];
        for (at, element) in elements.iter().enumerate() {
            let name = match element {
                Element::Arg(name) | Element::Form { of: name, .. } => name,
                Element::Select { variants, .. } | Element::Gender { variants, .. } => {
                    for variant in variants.iter() {
                        follows(&variant.value, into);
                    }
                    continue;
                }
                Element::Text(_) => continue,
            };
            if let Some(Element::Text(after)) = elements.get(at + 1) {
                if after.starts_with(NO_BREAK) || after.starts_with('%') {
                    into.joined.insert(format!("{name}>"));
                } else if after.starts_with(' ') {
                    into.parted.insert(format!("{name}>"));
                }
            }
            if let Some(Element::Text(before)) = at.checked_sub(1).map(|b| &elements[b]) {
                if before.ends_with(NO_BREAK) {
                    into.joined.insert(format!("{name}<"));
                } else if before.ends_with(' ') {
                    into.parted.insert(format!("{name}<"));
                }
            }
        }
    }

    fn named(elements: &[Element], into: &mut BTreeSet<String>, chosen_by: &mut BTreeSet<String>) {
        for element in elements {
            match element {
                Element::Text(_) => {}
                Element::Arg(name) | Element::Form { of: name, .. } => {
                    into.insert(name.to_string());
                }
                Element::Select { on, variants, .. } | Element::Gender { on, variants, .. } => {
                    into.insert(on.to_string());
                    if matches!(element, Element::Select { .. }) {
                        chosen_by.insert(on.to_string());
                    }
                    for variant in variants.iter() {
                        named(&variant.value, into, chosen_by);
                    }
                }
            }
        }
    }

    /// Each message of `source`, by its id; or why it cannot be read.
    pub fn messages(source: &str) -> Result<BTreeMap<String, Read>, Vec<String>> {
        let resource = match fluent_syntax::parser::parse(source) {
            Ok(resource) => resource,
            Err((_, errors)) => return Err(errors.iter().map(|e| format!("{e:?}")).collect()),
        };
        let mut found = BTreeMap::new();
        let mut problems = Vec::new();
        let mut seen = BTreeSet::new();
        for entry in resource.body {
            let fluent_syntax::ast::Entry::Message(message) = entry else {
                continue;
            };
            let id = message.id.name.to_owned();
            if !seen.insert(id.clone()) {
                // Said as its first copy: the second would be lost unseen.
                problems.push(format!("{id}: written twice"));
                continue;
            }
            let said = |word: &str| {
                message
                    .comment
                    .as_ref()
                    .is_some_and(|comment| comment.content.iter().any(|line| line.trim() == word))
            };
            let (draft, noun) = (said("draft"), said("noun"));
            let Some(value) = &message.value else {
                problems.push(format!("{id}: says nothing"));
                continue;
            };
            let mut attributes = Vec::new();
            for attribute in &message.attributes {
                match super::super::pattern::from_fluent(&attribute.value) {
                    Ok(elements) => attributes.push((attribute.id.name.to_owned(), elements)),
                    Err(why) => problems.push(format!("{id}: .{}: {why}", attribute.id.name)),
                }
            }
            match super::super::pattern::from_fluent(value) {
                Ok(elements) => {
                    let (mut names, mut chosen_by) = (BTreeSet::new(), BTreeSet::new());
                    named(&elements, &mut names, &mut chosen_by);
                    let mut after = Follows::default();
                    follows(&elements, &mut after);
                    found.insert(
                        id,
                        Read {
                            names,
                            chosen_by,
                            draft,
                            noun,
                            follows: after,
                            elements,
                            attributes,
                        },
                    );
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

    /// What a language gives its nouns (`tools/words/grammar`): its genders,
    /// and its forms, each with whether it holds a preposition a name has
    /// not (`+`).
    #[derive(Default)]
    pub struct Grammar {
        pub genders: BTreeSet<String>,
        pub forms: BTreeMap<String, bool>,
    }

    /// Each language's grammar, by its tag, as `tools/words/grammar`
    /// declares it.
    pub fn grammars(source: &str) -> Result<BTreeMap<String, Grammar>, String> {
        let mut out = BTreeMap::new();
        for line in source.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut words = line.split_whitespace();
            let language = words.next().unwrap_or_default().to_owned();
            let mut grammar = Grammar::default();
            let mut list = None;
            for word in words {
                match word {
                    "genders:" | "forms:" => list = Some(word),
                    _ if !word
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "+-_".contains(c)) =>
                    {
                        return Err(format!("{language}: `{word}` names nothing Fluent can"));
                    }
                    _ => match list {
                        Some("genders:") if word != grammar::NO_GENDER => {
                            grammar.genders.insert(word.to_owned());
                        }
                        Some("forms:") if word != grammar::GENDER_ATTRIBUTE => {
                            let fused = word.ends_with('+');
                            grammar
                                .forms
                                .insert(word.trim_end_matches('+').to_owned(), fused);
                        }
                        _ => return Err(format!("{language}: `{word}` is out of place")),
                    },
                }
            }
            if out.insert(language.clone(), grammar).is_some() {
                return Err(format!("{language} is declared twice"));
            }
        }
        Ok(out)
    }

    /// Whether `elements` are text alone (a noun's form, its gender).
    fn text_of(elements: &[Element]) -> Option<String> {
        elements
            .iter()
            .map(|element| match element {
                Element::Text(text) => Some(text.as_ref()),
                _ => None,
            })
            .collect()
    }

    /// What is wrong with the grammar of message `id` of a catalog in
    /// `language`, held to its English: attributes only on a noun, each
    /// from the language's declared set, a noun given every one; `GENDER`
    /// and `FORM` only on an argument the English gives that is no count or
    /// kind, every `GENDER` defaulting to `*[other]` among declared
    /// genders, every form declared, a form a name has not only where the
    /// argument is a noun.
    pub fn grammar_problems(
        id: &str,
        english: &Read,
        translated: &Read,
        language: &str,
        grammar: Option<&Grammar>,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        let declared = || format!("{language} declares none in tools/words/grammar");
        if !translated.attributes.is_empty() && !english.noun {
            problems.push(format!(
                "{id}: has attributes, but its English is no noun (`words!(…, noun)`): only a noun carries its gender and forms"
            ));
        }
        for (name, value) in &translated.attributes {
            let Some(grammar) = grammar else {
                problems.push(format!("{id}: .{name}: {}", declared()));
                continue;
            };
            let Some(text) = text_of(value) else {
                problems.push(format!(
                    "{id}: .{name} is text alone: a noun's form holds no argument"
                ));
                continue;
            };
            if name == grammar::GENDER_ATTRIBUTE {
                if !grammar.genders.contains(text.trim()) {
                    problems.push(format!(
                        "{id}: .gender = {text} is none of {language}'s genders ({})",
                        grammar
                            .genders
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            } else if !grammar.forms.contains_key(name) {
                problems.push(format!("{id}: .{name} is no form {language} declares"));
            }
        }
        if let (true, Some(grammar)) = (english.noun, grammar) {
            let given: BTreeSet<&str> = translated
                .attributes
                .iter()
                .map(|(n, _)| n.as_str())
                .collect();
            let wanted = (!grammar.genders.is_empty())
                .then_some(grammar::GENDER_ATTRIBUTE)
                .into_iter()
                .chain(grammar.forms.keys().map(String::as_str));
            for name in wanted {
                if !given.contains(name) {
                    problems.push(format!(
                        "{id}: a noun in {language} is given its .{name}: a sentence may ask for it"
                    ));
                }
            }
        }
        walk(
            &translated.elements,
            &mut Vec::new(),
            &mut |element, within| {
                let argument = |on: &str| {
                    english.chosen_by.contains(on).then(|| {
                    format!("{id}: the English chooses by ${on} (a count or a kind), which is no noun")
                })
                };
                match element {
                    Element::Gender {
                        on,
                        variants,
                        default,
                    } => {
                        let Some(grammar) = grammar.filter(|g| !g.genders.is_empty()) else {
                            problems.push(format!("{id}: GENDER(${on}): {}", declared()));
                            return;
                        };
                        problems.extend(argument(on));
                        if variants.get(*default).map(|v| &v.key)
                            != Some(&Key::Word(grammar::NO_GENDER.into()))
                        {
                            problems.push(format!(
                            "{id}: GENDER(${on}) defaults to *[other], what a name, a path or text as written takes"
                        ));
                        }
                        for variant in variants.iter() {
                            let known = matches!(&variant.key, Key::Word(word)
                            if word == grammar::NO_GENDER || grammar.genders.contains(word.as_ref()));
                            if !known {
                                problems.push(format!(
                                "{id}: GENDER(${on}) chooses [{:?}], none of {language}'s genders",
                                variant.key
                            ));
                            }
                        }
                    }
                    Element::Form { of, case } => {
                        let Some(fused) = grammar.and_then(|g| g.forms.get(case.as_ref())) else {
                            problems.push(format!(
                                "{id}: FORM(${of}, case: \"{case}\") is no form {language} declares"
                            ));
                            return;
                        };
                        problems.extend(argument(of));
                        let as_noun = within
                            .iter()
                            .any(|(on, key)| on == of && key != grammar::NO_GENDER);
                        if *fused && !as_noun {
                            problems.push(format!(
                            "{id}: FORM(${of}, case: \"{case}\") holds what a name has not: take it in a GENDER(${of}) variant, and say in *[other] what a name takes"
                        ));
                        }
                    }
                    _ => {}
                }
            },
        );
        problems
    }

    /// Each element of `elements` and of their variants, with the `GENDER`
    /// variants it stands in (their argument and key).
    fn walk(
        elements: &[Element],
        within: &mut Vec<(String, String)>,
        f: &mut impl FnMut(&Element, &[(String, String)]),
    ) {
        for element in elements {
            f(element, within);
            match element {
                Element::Select { variants, .. } => {
                    for variant in variants.iter() {
                        walk(&variant.value, within, f);
                    }
                }
                Element::Gender { on, variants, .. } => {
                    for variant in variants.iter() {
                        let key = match &variant.key {
                            Key::Word(word) => word.to_string(),
                            Key::Number(n) => n.to_string(),
                        };
                        within.push((on.to_string(), key));
                        walk(&variant.value, within, f);
                        within.pop();
                    }
                }
                Element::Text(_) | Element::Arg(_) | Element::Form { .. } => {}
            }
        }
    }
}

/// Every translated catalog of the workspace (`crates/**/l10n/<lang>.ftl`)
/// says what its crate's English says: each message one the English has,
/// in the part of Fluent Skwir speaks, naming only the arguments the
/// English gives, and keeping on one line what the English keeps there: an
/// argument the English joins to the words beside it by a no-break space
/// (a number to its unit, `{ $n }\u{a0}MB`, `10\u{a0}{ $unit }`) or to a
/// percent sign (`{ $n }%`) is never parted from them by a plain one. Its
/// grammar is its language's (`tools/words/grammar`, `grammar_problems`):
/// a noun's attributes, `GENDER` and `FORM`. How much of each language is
/// there, and how much is still a draft (a message whose comment says
/// `draft`), is printed.
#[test]
fn every_catalog_says_what_its_english_says() {
    use std::collections::BTreeMap;

    use reading::{grammar_problems, grammars, messages};

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("core is two folders below the repository's root");
    let grammar_file = root.join("tools/words/grammar");
    let grammars = grammars(&std::fs::read_to_string(&grammar_file).unwrap())
        .unwrap_or_else(|why| panic!("{}: {why}", grammar_file.display()));
    let crates = root.join("crates");
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
        for (id, read) in &english {
            if !read.attributes.is_empty() {
                problems.push(format!(
                    "{}: en.ftl: {id}: English gives no attributes",
                    folder.display()
                ));
            }
        }
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
            for (id, read) in &translated {
                let Some(given) = english.get(id) else {
                    problems.push(format!(
                        "{}: {id}: the English has no such message",
                        path.display()
                    ));
                    continue;
                };
                for name in read.names.difference(&given.names) {
                    problems.push(format!(
                        "{}: {id}: names ${name}, which the English gives not",
                        path.display()
                    ));
                }
                let kept = &given.follows;
                for side in read.follows.parted.intersection(&kept.joined) {
                    if !kept.parted.contains(side) {
                        let (name, words) = match side.strip_suffix('>') {
                            Some(name) => (name, "what follows"),
                            None => (side.trim_end_matches('<'), "what comes before"),
                        };
                        problems.push(format!(
                            "{}: {id}: parts ${name} from {words} by a plain space, \
                             where the English joins them by a no-break space",
                            path.display()
                        ));
                    }
                }
                problems.extend(
                    grammar_problems(id, given, read, language, grammars.get(language))
                        .into_iter()
                        .map(|why| format!("{}: {why}", path.display())),
                );
                tally.1 += 1;
                if read.draft {
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
    assert_eq!(size(999).english(), "999\u{a0}B");
    assert_eq!(size(1_500).english(), "1.5\u{a0}kB");
    assert_eq!(size(5_242_880).english(), "5.2\u{a0}MB");
    assert_eq!(
        crate::fs::size_words_in(1_536, crate::fs::SizeUnits::Binary).english(),
        "1.5\u{a0}KiB"
    );
    // What rounds up to a whole next unit is said in it, never as a
    // thousand of the one below.
    assert_eq!(size(999_949).english(), "999.9\u{a0}kB");
    assert_eq!(size(999_950).english(), "1.0\u{a0}MB");
    assert_eq!(size(999_990).english(), "1.0\u{a0}MB");
    assert_eq!(size(999_999_999).english(), "1.0\u{a0}GB");
    assert_eq!(
        crate::fs::size_words_in(1_048_570, crate::fs::SizeUnits::Binary).english(),
        "1.0\u{a0}MiB"
    );
    assert_eq!(
        crate::fs::size_words_to(999_999_999_999, crate::fs::SizeUnits::Decimal, 0).english(),
        "1\u{a0}TB"
    );
    assert_eq!(
        size(u64::MAX).english(),
        "18,446.7\u{a0}PB",
        "past the last unit"
    );
    // A message French has not is said whole in English, its numbers too:
    // never an English sentence with French numbers.
    let moved = crate::words!(test_moved_n = "Moved { $count } files", count = 12_345);
    assert_eq!(moved.say_in(&langs(&["fr", "en"])), "Moved 12,345 files");
}

/// Sizes counted in other units are said again where words are kept by
/// their revision: it moves with the units, and only with a change.
#[test]
fn the_size_units_move_what_is_said() {
    use crate::fs::{SizeUnits, set_size_units, size_words};
    set_size_units(SizeUnits::Decimal);
    let before = super::revision();
    set_size_units(SizeUnits::Decimal);
    assert_eq!(super::revision(), before, "no change, no move");
    set_size_units(SizeUnits::Binary);
    assert_ne!(super::revision(), before);
    assert_eq!(size_words(1_536).english(), "1.5\u{a0}KiB");
    set_size_units(SizeUnits::Decimal);
    assert_eq!(size_words(1_536).english(), "1.5\u{a0}kB");
}

/// Moments are written as the dates' locale writes them; numbers and dates
/// follow the system's locales in the window, English's for agents.
#[test]
fn moments_are_written_as_the_locale_writes_dates() {
    // 2024-03-12 14:02:05 UTC, written in UTC.
    crate::locale::set_zone(Some(jiff::tz::TimeZone::UTC));
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
    crate::locale::set_zone(Some(
        jiff::tz::TimeZone::get("Pacific/Kiritimati")
            .expect("the zone database knows Pacific/Kiritimati"),
    ));
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

/// Where the window's dates setting says ISO 8601, every date said for the
/// window is written as ISO writes it, whatever its shape and whoever says
/// it; agents' English never is.
#[test]
fn dates_are_written_as_iso_where_the_setting_says_so() {
    use super::{Moment, Shape};
    crate::locale::set_zone(Some(jiff::tz::TimeZone::UTC));
    crate::locale::set_formats(None, Some("en-GB".into()));
    let on = |shape| Moment::on(2026, 3, 12, shape).expect("a day there is");
    assert_eq!(on(Shape::Date).said(), "12 Mar 2026");
    crate::locale::set_iso_dates(true);
    for (shape, iso) in [
        (Shape::Time, "12:00"),
        (Shape::Weekday, "2026-03-12"),
        (Shape::DayMonth, "2026-03-12"),
        (Shape::Date, "2026-03-12"),
        (Shape::DateLong, "2026-03-12"),
        (Shape::DateTime, "2026-03-12 12:00"),
        (Shape::DateTimeSeconds, "2026-03-12 12:00:00"),
        (Shape::MonthYear, "2026-03"),
        (Shape::Month, "2026-03"),
        (Shape::MonthShort, "03"),
        (Shape::Year, "2026"),
        (Shape::Day, "12"),
    ] {
        assert_eq!(on(shape).said(), iso, "{shape:?}");
    }
    let words = crate::words!(test_on_day = "On { $day }", day = on(Shape::Date));
    assert_eq!(words.say(), "On 2026-03-12");
    assert_ne!(words.english(), "On 2026-03-12", "agents read English's");
    crate::locale::set_iso_dates(false);
    assert_eq!(words.say(), "On 12 Mar 2026");
    crate::locale::set_formats(None, None);
}

/// A desktop's own 12- or 24-hour switch outweighs its locale's hours,
/// whatever else the locale says.
#[test]
fn the_desktop_s_hours_outweigh_the_locale_s() {
    use super::{Moment, Shape};
    use crate::locale::{Hours, set_formats, set_hours};
    crate::locale::set_zone(Some(jiff::tz::TimeZone::UTC));
    let two = Moment {
        at: 1_710_252_125,
        shape: Shape::Time,
    };
    set_formats(None, Some("en-US".into()));
    assert_eq!(two.said(), "2:02\u{202f}PM");
    set_hours(Some(Hours::TwentyFour));
    assert_eq!(two.said(), "14:02");
    assert_eq!(crate::locale::dates(), "en-US-u-hc-h23");
    set_formats(None, Some("en-GB".into()));
    set_hours(Some(Hours::Twelve));
    assert_eq!(two.said(), "2:02\u{202f}pm");
    set_hours(None);
    assert_eq!(two.said(), "14:02");
    // The locale's other keywords and its private use kept, its own
    // hour cycle replaced.
    set_hours(Some(Hours::TwentyFour));
    for (locale, with) in [
        ("de-DE-u-co-phonebk", "de-DE-u-co-phonebk-hc-h23"),
        ("en-US-u-hc-h12", "en-US-u-hc-h23"),
        ("en-US-u-hc-h12-ca-gregory", "en-US-u-ca-gregory-hc-h23"),
        ("en-US-x-skwir", "en-US-u-hc-h23-x-skwir"),
    ] {
        set_formats(None, Some(locale.into()));
        assert_eq!(crate::locale::dates(), with);
    }
    set_hours(None);
    set_formats(None, None);
}

/// A noun of Skwir's (a place's title) carries its grammar in each
/// language's catalog: a German sentence takes its case, a French one its
/// article, a preposition fused with it where it is a noun; what is no
/// noun (a folder's name as written, a file's quoted name) has no gender
/// and is its own form, in either.
#[test]
fn a_sentence_takes_its_noun_s_gender_and_forms() {
    let bin = || crate::words!(test_noun_bin = "Trash", noun);
    let library = || crate::words!(test_noun_library = "Library", noun);
    let shelf = || crate::words!(test_noun_shelf = "Shelf", noun);
    let moved =
        |place: Words| crate::words!(test_noun_moved = "Moved to { $place }", place = place);
    let full = |place: Arg| crate::words!(test_noun_full = "{ $place } is full", place = place);
    register(
        "words-test",
        "skwir-core",
        Catalog::new(
            "de",
            "name_quote_open = „\nname_quote_close = “\n\
             test_noun_bin = Papierkorb\n    .gender = masculine\n    .accusative = den Papierkorb\n    .dative = dem Papierkorb\n\
             test_noun_library = Bibliothek\n    .gender = feminine\n    .accusative = die Bibliothek\n    .dative = der Bibliothek\n\
             test_noun_moved = In { FORM($place, case: \"accusative\") } verschoben\n\
             test_noun_full = { GENDER($place) ->\n    [masculine] Der { $place } ist voll\n    [feminine] Die { $place } ist voll\n   *[other] { $place } ist voll\n}\n",
        ),
    );
    let de = langs(&["de-AT", "en"]);
    assert_eq!(moved(bin()).say_in(&de), "In den Papierkorb verschoben");
    assert_eq!(moved(library()).say_in(&de), "In die Bibliothek verschoben");
    assert_eq!(full(bin().into()).say_in(&de), "Der Papierkorb ist voll");
    assert_eq!(
        full(library().into()).say_in(&de),
        "Die Bibliothek ist voll"
    );
    // No noun: a folder's name as written, a file's name, quoted as German
    // quotes; and a noun German's catalog lacks, said in English.
    assert_eq!(
        moved(Words::as_written("Docs")).say_in(&de),
        "In Docs verschoben"
    );
    assert_eq!(
        full(Name("a.txt".into()).into()).say_in(&de),
        "„a.txt“ ist voll"
    );
    assert_eq!(moved(shelf()).say_in(&de), "In Shelf verschoben");
    assert_eq!(full(shelf().into()).say_in(&de), "Shelf ist voll");
    // A noun said in a language that is not the sentence's (a reader's next
    // one, which has it): its grammar is not the sentence's.
    register(
        "words-test",
        "skwir-core",
        Catalog::new("fr", "test_noun_shelf = Étagère\n    .gender = feminine\n"),
    );
    assert_eq!(
        full(shelf().into()).say_in(&langs(&["de", "fr", "en"])),
        "Étagère ist voll"
    );
    // One language in one script, whatever the region: a Hong Kong reader's
    // sentence from a Traditional Chinese catalog takes its noun's form from
    // a catalog of Hong Kong's (a form made up for the test).
    register(
        "words-test",
        "skwir-core",
        Catalog::new(
            "zh-Hant",
            "test_noun_moved = 已移至{ FORM($place, case: \"locative\") }\n",
        ),
    );
    register(
        "words-test",
        "skwir-core",
        Catalog::new(
            "zh-HK",
            "test_noun_bin = 回收筒\n    .locative = 回收筒中\n",
        ),
    );
    assert_eq!(moved(bin()).say_in(&langs(&["zh-HK"])), "已移至回收筒中");
    // A heading of a noun is the noun; words come back from their data are
    // the words they were.
    assert_eq!(
        full(bin().heading().into()).say_in(&de),
        "Der Papierkorb ist voll"
    );
    let back: Words = serde_json::from_value(bin().to_data()).unwrap();
    assert_eq!(moved(back).say_in(&de), "In den Papierkorb verschoben");
    // English, and agents, read the English as it is.
    assert_eq!(moved(bin()).english(), "Moved to Trash");
    assert_eq!(full(library().into()).english(), "Library is full");

    // French: the noun with its article, and « de » fused with it, which a
    // name has not.
    let home = || crate::words!(test_noun_home = "Home", noun);
    let opening =
        |place: Words| crate::words!(test_noun_opening = "Opening { $place }…", place = place);
    let search =
        |place: Words| crate::words!(test_noun_search = "Search { $place }", place = place);
    register(
        "words-test",
        "skwir-core",
        Catalog::new(
            "fr",
            "test_noun_bin = Corbeille\n    .gender = feminine\n    .definite = la Corbeille\n    .de = de la Corbeille\n\
             test_noun_home = Dossier personnel\n    .gender = masculine\n    .definite = le Dossier personnel\n    .de = du Dossier personnel\n\
             test_noun_shelf = Étagère\n    .gender = feminine\n    .definite = l’Étagère\n    .de = de l’Étagère\n\
             test_noun_opening = Ouverture { GENDER($place) ->\n    [masculine] { FORM($place, case: \"de\") }\n    [feminine] { FORM($place, case: \"de\") }\n   *[other] de { $place }\n}…\n\
             test_noun_search = Rechercher dans { FORM($place, case: \"definite\") }\n",
        ),
    );
    let fr = langs(&["fr-FR", "en"]);
    assert_eq!(opening(bin()).say_in(&fr), "Ouverture de la Corbeille…");
    assert_eq!(
        opening(home()).say_in(&fr),
        "Ouverture du Dossier personnel…"
    );
    assert_eq!(opening(shelf()).say_in(&fr), "Ouverture de l’Étagère…");
    assert_eq!(
        opening(Words::as_written("Docs")).say_in(&fr),
        "Ouverture de Docs…"
    );
    assert_eq!(search(shelf()).say_in(&fr), "Rechercher dans l’Étagère");
    assert_eq!(
        search(Words::as_written("Docs")).say_in(&fr),
        "Rechercher dans Docs"
    );
    // In the pseudo-language, the English, its argument marked, the noun
    // in it said in the pseudo-language too.
    assert_eq!(
        opening(bin()).say_in(&langs(&[PSEUDO, "en"])),
        "[ØØpééñííñĝ ‹[Tráášĥ]›…]"
    );
    withdraw("words-test");
    assert_eq!(opening(bin()).say_in(&fr), "Opening Trash…");
}

/// The catalogs' test refuses an id a catalog writes twice: the program
/// says its first copy, and the second would be lost unseen.
#[test]
fn the_catalogs_test_refuses_an_id_written_twice() {
    assert_eq!(
        reading::messages("a = un\nb = deux\na = autre\n").err(),
        Some(vec!["a: written twice".to_owned()])
    );
    assert!(reading::messages("a = un\nb = deux\n").is_ok());
}

/// The catalogs' test holds a translation's grammar to its language's
/// declaration: attributes on a noun alone, from the declared set, every
/// one given; GENDER and FORM on what may be a noun, by declared genders
/// and forms, a form a name has not only where the argument is a noun.
#[test]
fn the_catalogs_test_refuses_grammar_a_language_does_not_declare() {
    use reading::{grammar_problems, grammars, messages};
    let english = messages(
        "# noun\nbin = Trash\n\
         plain = Copy\n\
         moved = Moved to { $place }\n\
         counted = { $count ->\n    [one] one item\n   *[other] { $count } items\n} in { $place }\n",
    )
    .unwrap();
    let declared =
        grammars("# A comment.\nfr  genders: masculine feminine  forms: definite de+\n").unwrap();
    let problems = |translated: &str, language: &str| -> Vec<String> {
        let translated = messages(translated).unwrap();
        translated
            .iter()
            .flat_map(|(id, read)| {
                grammar_problems(id, &english[id], read, language, declared.get(language))
            })
            .collect()
    };
    // As French declares it: no problem.
    assert_eq!(
        problems(
            "bin = Corbeille\n    .gender = feminine\n    .definite = la Corbeille\n    .de = de la Corbeille\n\
             moved = Déplacé { GENDER($place) ->\n    [feminine] { FORM($place, case: \"de\") }\n   *[other] dans { FORM($place, case: \"definite\") }\n}\n",
            "fr",
        ),
        Vec::<String>::new()
    );
    let refused = |translated: &str, language: &str, why: &str| {
        let found = problems(translated, language);
        assert!(found.iter().any(|p| p.contains(why)), "{why}: {found:?}");
    };
    // Attributes on what is no noun.
    refused(
        "plain = Copier\n    .gender = masculine\n",
        "fr",
        "plain: has attributes, but its English is no noun",
    );
    // A gender, a form the language does not declare; a form not text.
    refused(
        "bin = Corbeille\n    .gender = féminin\n    .definite = la Corbeille\n    .de = de la Corbeille\n",
        "fr",
        "is none of fr's genders",
    );
    refused(
        "bin = Corbeille\n    .gender = feminine\n    .definite = la Corbeille\n    .de = de la Corbeille\n    .dativ = x\n",
        "fr",
        ".dativ is no form fr declares",
    );
    refused(
        "bin = Corbeille\n    .gender = feminine\n    .definite = la { $x }\n    .de = de la Corbeille\n",
        "fr",
        ".definite is text alone",
    );
    // A noun without every form its language declares.
    refused(
        "bin = Corbeille\n    .gender = feminine\n",
        "fr",
        "is given its .definite",
    );
    // A language that declares no grammar gives nothing, chooses by none.
    refused(
        "bin = Papierkorb\n    .gender = masculine\n",
        "de",
        ".gender: de declares none",
    );
    refused(
        "moved = { GENDER($place) ->\n   *[other] Nach { $place } verschoben\n}\n",
        "de",
        "GENDER($place): de declares none",
    );
    // GENDER defaults to other, chooses among declared genders.
    refused(
        "moved = { GENDER($place) ->\n    [other] dans { $place }\n   *[feminine] dans { FORM($place, case: \"definite\") }\n}\n",
        "fr",
        "defaults to *[other]",
    );
    refused(
        "moved = { GENDER($place) ->\n    [neuter] x\n   *[other] dans { $place }\n}\n",
        "fr",
        "none of fr's genders",
    );
    // What the English chooses by is a count or a kind: no noun.
    refused(
        "counted = { GENDER($count) ->\n   *[other] { $count } éléments\n} dans { $place }\n",
        "fr",
        "the English chooses by $count",
    );
    // A form not declared; one a name has not, outside a noun's variant.
    refused(
        "moved = Déplacé { FORM($place, case: \"dative\") }\n",
        "fr",
        "is no form fr declares",
    );
    refused(
        "moved = Déplacé { FORM($place, case: \"de\") }\n",
        "fr",
        "holds what a name has not",
    );
    refused(
        "moved = Déplacé { GENDER($place) ->\n    [feminine] x\n   *[other] { FORM($place, case: \"de\") }\n}\n",
        "fr",
        "holds what a name has not",
    );
    // The declaration itself names only what Fluent can, once a language.
    assert!(grammars("fr genders: masculin«\n").is_err());
    assert!(grammars("fr forms: de\nfr forms: a\n").is_err());
    assert!(grammars("fr definite\n").is_err());
}

// A plugin's title, what it needs a program for, and a command of it, as
// `words!` writes them in its crate.
static DESCRIBED_TITLE_ENGLISH: [Element; 1] = [Element::Text(B("Sample"))];
static DESCRIBED_TITLE: Message = Message {
    domain: "skwir-plugin-described",
    id: "described_plugin_title",
    english: &DESCRIBED_TITLE_ENGLISH,
    note: "",
};
static DESCRIBED_NEEDS_ENGLISH: [Element; 2] =
    [Element::Text(B("Pictures of ")), Element::Arg(B("what"))];
static DESCRIBED_NEEDS: Message = Message {
    domain: "skwir-plugin-described",
    id: "described_plugin_needs",
    english: &DESCRIBED_NEEDS_ENGLISH,
    note: "",
};
static DESCRIBED_RUN_ENGLISH: [Element; 1] = [Element::Text(B("Run the sample"))];
static DESCRIBED_RUN: Message = Message {
    domain: "skwir-plugin-described",
    id: "described_plugin_run",
    english: &DESCRIBED_RUN_ENGLISH,
    note: "",
};

/// A plugin that is off is described in the reader's language (its title,
/// what it needs a program for), from the catalogs shipped for its crate:
/// registering nothing, indexing none. Nothing else it says is said from
/// them, nor are its words anywhere but where it is described. On, its
/// catalogs registered say it, and the rest; a plugin from outside is
/// described from what its host keeps.
#[test]
fn a_plugin_that_is_off_is_described_in_the_reader_s_language() {
    const OUTSIDE: &str = "outside:described";
    static SHIPPED: [super::Shipped; 1] = [super::Shipped {
        domain: "skwir-plugin-described",
        language: "fr",
        source: "# Before it, a message on several lines.\n\
            described_plugin_moved = { $count ->\n    [one] Un\n   *[other] { $count }\n}\n\
            described_plugin_needs = Des images de { $what }\n\
            described_plugin_run = Lancer l’échantillon\n\
            described_plugin_title = Échantillon\n",
    }];
    super::ship(&SHIPPED);
    let fr = langs(&["fr"]);
    let title = Words::message(&DESCRIBED_TITLE, vec![]).describing();
    let needs =
        || Words::message(&DESCRIBED_NEEDS, vec![("what", Arg::Text(B("SVG")))]).describing();
    let run = Words::message(&DESCRIBED_RUN, vec![]);
    assert_eq!(title.say_in(&fr), "Échantillon", "off, described in French");
    assert_eq!(needs().say_in(&fr), "Des images de SVG");
    assert_eq!(title.english(), "Sample", "to agents, English");
    assert_eq!(serde_json::to_value(&title).unwrap(), "Sample");
    assert_eq!(title.heading().say_in(&fr), "Échantillon");
    assert_eq!(run.say_in(&fr), "Run the sample", "nothing else it says");
    assert_eq!(
        Words::message(&DESCRIBED_TITLE, vec![]).say_in(&fr),
        "Sample",
        "only where it is described"
    );
    crate::locale::set_chosen(Some("fr".into()));
    assert_eq!(
        needs().say_around("what"),
        Some(("Des images de ".into(), String::new())),
        "cut for the window, in its language"
    );
    assert_eq!(title.say(), "Échantillon");
    assert!(
        super::languages_of("skwir-plugin-described").is_empty(),
        "nothing registered"
    );
    assert_eq!(
        super::described_indexed("skwir-plugin-described"),
        Some(false),
        "no catalog indexed"
    );
    // Come back from its data, it describes nothing.
    let back: Words = serde_json::from_value(title.to_data()).unwrap();
    assert_eq!(back.say_in(&fr), "Sample");
    // A reader of English first, of a language none says, of a region.
    assert_eq!(title.say_in(&langs(&["en", "fr"])), "Sample");
    assert_eq!(title.say_in(&langs(&["de"])), "Sample");
    assert_eq!(title.say_in(&langs(&["de", "fr-CA"])), "Échantillon");
    assert!(Words::as_written("").describing().is_empty());
    // On, its catalogs registered say it and all it says; those first.
    super::register_shipped("described", "skwir_plugin_described");
    assert_eq!(title.say_in(&fr), "Échantillon");
    assert_eq!(run.say_in(&fr), "Lancer l’échantillon");
    register(
        "described",
        "skwir-plugin-described",
        Catalog::new("fr", "described_plugin_title = Enregistré\n"),
    );
    assert_eq!(title.say_in(&fr), "Enregistré", "registered first");
    // Off again: as before.
    withdraw("described");
    assert_eq!(run.say_in(&fr), "Run the sample");
    assert_eq!(title.say_in(&fr), "Échantillon");
    // A plugin from outside: what its host keeps for it.
    super::describe_with(
        OUTSIDE,
        vec![Catalog::new(
            "fr",
            "plugin-title = Compte (essai)\nrun = Lancer (essai)\n",
        )],
    );
    let english = [Element::Text(B("Count"))];
    let title = Words::of_catalog(OUTSIDE, "plugin-title", Some(&english[..]), vec![]).describing();
    let english = [Element::Text(B("Run"))];
    let run = Words::of_catalog(OUTSIDE, "run", Some(&english[..]), vec![]);
    assert_eq!(title.say_in(&fr), "Compte (essai)");
    assert_eq!(title.english(), "Count");
    assert_eq!(run.say_in(&fr), "Run", "nothing else it says");
    assert_eq!(super::described_indexed(OUTSIDE), Some(false));
    // Shipped again, a plugin's are read anew; a host's stay.
    super::ship(&[]);
    assert_eq!(
        Words::message(&DESCRIBED_TITLE, vec![])
            .describing()
            .say_in(&fr),
        "Sample"
    );
    assert_eq!(title.say_in(&fr), "Compte (essai)");
}
