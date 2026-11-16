use std::borrow::Cow::Borrowed as B;

use super::{
    Arg, Catalog, Element, Key, Kind, Message, Name, PSEUDO, Size, Variant, Words, register,
    withdraw,
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
