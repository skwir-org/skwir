//! What the plugin reads of other programs (the desktop's recently used
//! files, zoxide's list) given what no file should hold: never a panic.

use proptest::prelude::*;
use skwir_core::testing::{bytes, mutated, mutated_text};

const RECENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xbel version="1.0" xmlns:bookmark="http://www.freedesktop.org/standards/desktop-bookmarks"
 xmlns:mime="http://www.freedesktop.org/standards/shared-mime-info">
<bookmark href="file:///home/a/caf%C3%A9%20notes.txt" added="2026-03-12T08:00:00.000Z"
 modified="2026-03-12T08:00:00.000Z" visited="2026-03-12T09:00:00.000Z">
<info><metadata owner="http://freedesktop.org"><mime:mime-type type="text/plain"/>
<bookmark:applications><bookmark:application name="Editor" exec="&apos;editor %u&apos;"
 modified="2026-03-12T08:00:00Z" count="2"/></bookmark:applications></metadata></info>
</bookmark></xbel>"#;

proptest! {
    #[test]
    fn the_recently_used_files(text in prop_oneof![mutated_text(RECENT), any::<String>()]) {
        let _ = crate::xbel::parse(&text);
    }

    #[test]
    fn zoxide_s_list(input in prop_oneof![
        mutated(&b"  12.5 /home/a/projects/skwir\n   3.0 /home/a/My Files/caf\xc3\xa9\n"[..]),
        bytes(512),
    ]) {
        let _ = crate::zoxide::parse(&input);
    }
}
