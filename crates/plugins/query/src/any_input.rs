//! The query language given what no one types on purpose: a query
//! mutated, and any text. It may refuse it; it never panics.

use proptest::prelude::*;
use skwir_core::testing::mutated_text;

use crate::lang::{parse, parse_bytes};

const QUERY: &str = "*.pdf report \"café notes\" modified>=2026-01-01 modified<this-week \
size>10M size<1.5G type:image kind:document tag:Places/France !tag:draft \
rating>=3 in:\"/home/a/My Files\" (a | b) -c content:\"it's\" near:45.0,6.0,10km case:yes";

proptest! {
    #[test]
    fn a_query(text in prop_oneof![mutated_text(QUERY), any::<String>()]) {
        let _ = parse(&text);
    }

    #[test]
    fn a_size(text in prop_oneof![mutated_text("1.5GiB"), mutated_text("10M"), any::<String>()]) {
        let _ = parse_bytes(&text);
    }
}
