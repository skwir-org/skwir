//! A folder narrowed: the Narrow by bar's choices over a folder ("images
//! only", "2024"), kept while the pane shows it or a folder under it, as
//! a locked filter is. Its words are a query's, written by
//! [`facets::refine`]; the core keeps them (`workspace::Narrowed`) with
//! the pills they read as and a test of each file, which reads what the
//! listing knows of the file, never the file: what needs it read (what it
//! contains, where it was made) is a search's.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use skwir_core::command::Invocation;
use skwir_core::entry::Entry;
use skwir_core::workspace::{Narrowed, Pill};

use crate::facets;
use crate::lang::{self, Candidate, Expr, Verdict};

/// What `expr` narrows a folder to, set in `root`; `None` when it says
/// nothing, so the folder shows everything again.
pub fn narrowed(expr: &str, root: PathBuf) -> Result<Option<Narrowed>, String> {
    let q = lang::parse(expr).map_err(|e| e.to_string())?;
    let Some(test) = q.expr.clone() else {
        return Ok(None);
    };
    if q.reads_files() {
        return Err(
            "a folder is narrowed by what is known of its files: search for what they contain"
                .into(),
        );
    }
    let (parts, set) = facets::parts(expr)?;
    let pills = parts
        .into_iter()
        .map(|part| Pill {
            label: part.label,
            remove: Invocation::with_args(
                "query.narrow",
                &serde_json::json!({ "facet": part.facet, "value": part.value }),
            ),
            tag: part.tag,
        })
        .collect();
    Ok(Some(Narrowed {
        root,
        expr: expr.to_owned(),
        pills,
        set,
        keeps: Arc::new(move |entry| keeps(&test, entry)),
    }))
}

/// Whether `entry` is what `test` asks, from what the listing knows of
/// it: until its metadata comes, what depends on it is not known, and
/// the file is not shown.
fn keeps(test: &Expr, entry: &Entry) -> bool {
    let mut c = Candidate::of_entry(entry, Path::new(&entry.name));
    if !entry.metadata.is_hydrated() {
        c.metadata = None;
    }
    test.eval(&c, false) == Verdict::Yes
}
