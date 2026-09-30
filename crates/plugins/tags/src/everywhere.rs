//! A tag renamed, merged into another or taken away on every file that
//! has it: the files are found by the search a `tag:` term runs (the
//! query plugin's, through a location, as the walk answers where indexes
//! only hint: ADR-026), and the edit is a plan of one step per file,
//! proposed for review, then undoable as one.

use std::path::PathBuf;
use std::time::Duration;

use bibli_core::app::{App, NotifyLevel};
use bibli_core::cancel::CancelToken;
use bibli_core::command::{Invocation, Outcome};
use bibli_core::location::{ListingEvent, Location};
use bibli_core::metadata::same_tag;
use bibli_core::op::{EntryRef, Plan};
use bibli_core::os_path::PathArg;
use bibli_core::scheduler::Priority;

use crate::attrs::{parse_tags, read_tags};
use crate::ops::SetTags;

/// `tags` with `from` renamed `to`, or taken away without one; a tag met
/// twice (the file had both) is kept once, where it came first.
pub fn renamed(tags: &[String], from: &str, to: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags {
        let tag = if same_tag(tag, from) {
            match to {
                Some(to) => to,
                None => continue,
            }
        } else {
            tag.as_str()
        };
        if !out.iter().any(|t| same_tag(t, tag)) {
            out.push(tag.to_owned());
        }
    }
    out
}

/// The files under `root` (the home folder when `None`) tagged `tag`,
/// with their ids, as the search finds them. Blocks until it completes:
/// call on a worker.
fn tagged(
    app: &App,
    tag: &str,
    root: Option<&PathBuf>,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<EntryRef>> {
    let mut args = serde_json::json!({ "facet": "tag", "value": tag });
    if let Some(root) = root {
        args["root"] = serde_json::json!(PathArg::from(root.as_path()));
    }
    let Outcome::Value(v) = app.dispatch(&Invocation::with_args("query.location", &args))? else {
        anyhow::bail!("the search gave no location");
    };
    let location = v["location"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("the search gave no location"))?;
    let open = app.open_location(&Location::parse(location))?;
    let mut found = Vec::new();
    let outcome = loop {
        if cancel.is_cancelled() {
            break Err(anyhow::anyhow!("cancelled"));
        }
        match open.events().recv_timeout(Duration::from_millis(100)) {
            Ok(ListingEvent::Added(entries)) => {
                for e in entries {
                    if let Some(path) = open.path_of(e.id) {
                        found.push(EntryRef::new(e.id, path));
                    }
                }
            }
            Ok(ListingEvent::Removed(ids)) => found.retain(|r| !ids.contains(&r.id)),
            Ok(ListingEvent::Complete) => break Ok(found),
            Ok(ListingEvent::Error(e)) => break Err(anyhow::anyhow!("{e}")),
            Ok(_) | Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                break Err(anyhow::anyhow!("the search stopped"));
            }
        }
    };
    open.close();
    outcome
}

/// Start renaming `tag` to `to` (taking it away when `None`) on every file
/// under `root`: checks what it can now, finds the files on a worker, and
/// proposes the plan.
pub fn start(
    app: &App,
    lifetime: &CancelToken,
    tag: &str,
    to: Option<&str>,
    root: Option<PathBuf>,
) -> anyhow::Result<()> {
    let tag = tag.trim().to_owned();
    anyhow::ensure!(!tag.is_empty(), "which tag?");
    let to = match to {
        Some(to) => {
            let mut names = parse_tags(to);
            anyhow::ensure!(names.len() <= 1, "a tag's name has no comma");
            let to = names
                .pop()
                .ok_or_else(|| anyhow::anyhow!("what new name?"))?;
            anyhow::ensure!(to != tag, "the tag is already called “{to}”");
            Some(to)
        }
        None => None,
    };
    anyhow::ensure!(
        app.commands.get("query.location").is_some(),
        "finding the tagged files needs the Search plugin"
    );
    let place = root.as_ref().map_or_else(
        || "the home folder".to_owned(),
        |r| format!("“{}”", r.display()),
    );
    app.notify(
        NotifyLevel::Info,
        format!("Looking for the files tagged “{tag}” in {place}…"),
    );
    let weak = app.weak();
    let cancel = lifetime.child();
    app.scheduler.spawn(
        Priority::Normal,
        "tags.everywhere",
        cancel.clone(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let files = match tagged(&app, &tag, root.as_ref(), &cancel) {
                Ok(files) => files,
                Err(_) if cancel.is_cancelled() => return,
                Err(error) => {
                    app.notify(
                        NotifyLevel::Error,
                        format!("The files tagged “{tag}” were not found: {error:#}"),
                    );
                    return;
                }
            };
            let title = match &to {
                Some(to) => format!("rename the tag “{tag}” to “{to}”"),
                None => format!("remove the tag “{tag}” from every file"),
            };
            let mut plan = Plan::new(title);
            let mut merged = 0;
            for r in files {
                let current = read_tags(&r.path);
                let tags = renamed(&current, &tag, to.as_deref());
                if tags == current {
                    continue;
                }
                if tags.len() < current.len() && to.is_some() {
                    merged += 1;
                }
                plan.push(SetTags {
                    entry: r,
                    tags,
                    finder_colors: Vec::new(),
                });
            }
            let n = plan.steps.len();
            if n == 0 {
                app.notify(
                    NotifyLevel::Info,
                    format!("No file in {place} is tagged “{tag}”"),
                );
                return;
            }
            let files = if n == 1 {
                "1 file".to_owned()
            } else {
                format!("{n} files")
            };
            let merged = match &to {
                Some(to) if merged > 0 => {
                    format!(" {merged} already had “{to}”: they keep it once.")
                }
                _ => String::new(),
            };
            let note =
                format!("{files} in {place}; hidden files and folders are left out.{merged}");
            app.propose(plan.with_note(note));
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(t: &[&str]) -> Vec<String> {
        t.iter().map(|&s| s.to_owned()).collect()
    }

    #[test]
    fn a_renamed_tag_takes_its_place_and_a_merged_one_is_kept_once() {
        let file = tags(&["a", "Work", "b"]);
        assert_eq!(renamed(&file, "work", Some("Job")), ["a", "Job", "b"]);
        assert_eq!(renamed(&file, "WORK", Some("work")), ["a", "work", "b"]);
        assert_eq!(renamed(&file, "work", Some("B")), ["a", "B"]);
        assert_eq!(renamed(&file, "work", None), ["a", "b"]);
        assert_eq!(renamed(&file, "other", Some("x")), file);
    }
}
