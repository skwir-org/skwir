//! What the plugin reads that it did not write (a Mac's `._` files and
//! Finder attributes, the desktop's rating, a typed `tags:` address, its
//! own index as the cache keeps it) given what no file should hold: a
//! sample mutated, and bytes at random. It may refuse it; it never
//! panics.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bibli_core::testing::{bytes, mutated, mutated_text};
use proptest::prelude::*;

use crate::appledouble::AppleDouble;
use crate::finder::{self, Framing, Tag};

fn finder_tags(framing: Framing) -> Vec<u8> {
    finder::encode(
        &[
            Tag {
                name: "Red".into(),
                color: 6,
            },
            Tag {
                name: "Places/Café".into(),
                color: 0,
            },
        ],
        framing,
    )
}

fn sidecar() -> Vec<u8> {
    crate::appledouble::tests::as_a_mac_writes(&[
        (
            "com.apple.metadata:_kMDItemUserTags",
            &finder_tags(Framing::Plain),
        ),
        ("user.xdg.tags", b"draft,Places/Paris"),
    ])
}

proptest! {
    #[test]
    fn a_mac_sidecar(input in prop_oneof![mutated(sidecar()), bytes(4096)]) {
        if let Some(read) = AppleDouble::parse(&input) {
            let _ = read.encode();
        }
    }

    #[test]
    fn a_finder_attribute(input in prop_oneof![
        mutated(finder_tags(Framing::Plain)),
        mutated(finder_tags(Framing::Stream)),
        mutated(finder::encode_comment("A comment, “quoted”", Framing::Plain)),
        bytes(512),
    ]) {
        for framing in [Framing::Plain, Framing::Stream] {
            let _ = finder::parse(&input, framing);
            let _ = finder::parse_comment(&input, framing);
            let _ = finder::parse_where_froms(&input, framing);
        }
    }

    #[test]
    fn a_rating_and_an_address_typed(text in prop_oneof![
        mutated_text("4"),
        mutated_text("/Places/France?in=%2Fhome%2Fa&with=red&deep=1&year=2026"),
        any::<String>(),
    ]) {
        let _ = crate::xdg::parse_rating(&text);
        let _ = crate::location::Address::parse(&text);
    }

    #[test]
    fn the_index_as_the_cache_keeps_it(input in prop_oneof![
        mutated(crate::index::tests::encoded_folders(&BTreeMap::from([
            (PathBuf::from("/home/a/Photos"), 3),
            (PathBuf::from("/home/a/Café"), 1),
        ]))),
        bytes(1024),
    ]) {
        let _ = crate::index::tests::decoded_folders(&input);
        let _ = crate::index::tests::decoded_piece(&input, &[None, Some(0), None]);
    }
}
