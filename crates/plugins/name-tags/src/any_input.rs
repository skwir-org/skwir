//! Names as anyone writes them, cut into a base, tags and an extension
//! by each convention and joined back: never a panic.

use proptest::prelude::*;
use skwir_core::testing::mutated_text;

use crate::Convention;

proptest! {
    #[test]
    fn a_name(name in prop_oneof![
        mutated_text("Holiday café[beach 2026 family].tar.gz"),
        mutated_text("report -- draft work.pdf"),
        any::<String>(),
    ]) {
        for convention in [Convention::Tagspaces, Convention::Filetags] {
            let (base, tags, ext) = convention.split(&name);
            let _ = convention.join(&base, &tags, ext.as_deref());
            let _ = convention.holds(&name);
        }
    }
}
