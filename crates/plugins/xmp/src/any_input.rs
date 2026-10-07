//! XMP as other applications write it, given what no file should hold:
//! a sidecar mutated, and bytes at random. Reading or editing it may
//! fail; it never panics.

use bibli_core::testing::{bytes, mutated};
use proptest::prelude::*;

use crate::packet::{edit, notes_of, packet_in, tests::DARKTABLE};

proptest! {
    #[test]
    fn a_sidecar_or_a_file_holding_a_packet(input in prop_oneof![
        mutated(DARKTABLE),
        mutated([&b"\xff\xd8\xff\xe1 JPEG "[..], DARKTABLE.as_bytes(), b" more"].concat()),
        bytes(2048),
    ]) {
        let _ = packet_in(&input);
        let text = String::from_utf8_lossy(&input);
        let known = notes_of(DARKTABLE).expect("the sample's notes");
        if let Some(notes) = notes_of(&text) {
            let _ = edit(&text, &notes);
        }
        let _ = edit(&text, &known);
    }
}
