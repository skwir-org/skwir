//! A tag renamed, merged into another or taken away on every file that
//! has it: the files are found by the search a `tag:` term runs (the
//! query plugin's, through a location, as the walk answers where indexes
//! only hint: ADR-026), and the edit is a plan of one step per file,
//! proposed for review, then undoable as one.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use skwir_core::app::{App, NotifyLevel};
use skwir_core::cancel::CancelToken;
use skwir_core::command::{Invocation, Outcome};
use skwir_core::location::{ListingEvent, Location};
use skwir_core::metadata::{same_tag, tag_leaf, tag_parent, tag_within};
use skwir_core::op::{EntryRef, Plan};
use skwir_core::os_path::PathArg;
use skwir_core::scheduler::Priority;

use skwir_core::notes::parse_tags;

/// Tags renamed (`from`, several merging) to `to`, or taken away.
#[derive(Clone, Debug)]
pub struct Rename {
    pub from: Vec<String>,
    pub to: Option<String>,
}

/// A file the search found, with its tags as the search read them, when
/// it did.
type Found = (EntryRef, Option<Vec<String>>);

/// Add to `files` those of `found` not in it yet (by path, as `seen`
/// keeps them), in the order found: a file with several of the tags is
/// edited once. A rename of a tag on a hundred thousand files compared
/// each with every one before it.
fn gather(files: &mut Vec<Found>, seen: &mut HashSet<PathBuf>, found: Vec<Found>) {
    for file in found {
        if seen.insert(file.0.path.clone()) {
            files.push(file);
        }
    }
}

/// The files under `root` (the home folder when `None`) tagged `tag`,
/// with their ids and the tags the search read of them, as it finds them.
/// Blocks until it completes: call on a worker.
fn tagged(
    app: &App,
    tag: &str,
    root: Option<&PathBuf>,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<Found>> {
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
                        // A `tag:` search read each file's tags to find it.
                        let tags = crate::tags_of(&e.metadata);
                        found.push((e.id, (EntryRef::new(e.file(), path), tags)));
                    }
                }
            }
            Ok(ListingEvent::Removed(ids)) => found.retain(|(id, _)| !ids.contains(id)),
            Ok(ListingEvent::Complete) => break Ok(found.into_iter().map(|(_, f)| f).collect()),
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

/// The tags a command names: `tag` and `tags`, trimmed, each once.
pub fn named(tag: String, tags: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in std::iter::once(tag).chain(tags) {
        let t = t.trim();
        if !t.is_empty() && !out.iter().any(|o| same_tag(o, t)) {
            out.push(t.to_owned());
        }
    }
    out
}

/// “a”, “b” and “c”.
fn listed(tags: &[String]) -> String {
    let quoted: Vec<String> = tags.iter().map(|t| format!("“{t}”")).collect();
    match quoted.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => quoted.concat(),
    }
}

/// What `renames` does, for the review's title.
fn title_of(renames: &[Rename]) -> String {
    match renames {
        [Rename { from, to: Some(to) }] if from.len() == 1 => {
            let tag = &from[0];
            if tag_leaf(tag) == tag_leaf(to) && tag_parent(tag) != tag_parent(to) {
                match tag_parent(to) {
                    Some(into) => format!("move the tag “{tag}” into “{into}”"),
                    None => format!("move the tag “{tag}” out of its category"),
                }
            } else {
                format!("rename the tag “{tag}” to “{to}”")
            }
        }
        [Rename { from, to: Some(to) }] => {
            format!("merge the tags {} into “{to}”", listed(from))
        }
        [Rename { from, to: None }] if from.len() == 1 => {
            format!("remove the tag “{}” from every file", from[0])
        }
        [Rename { from, to: None }] => {
            format!("remove the tags {} from every file", listed(from))
        }
        _ => {
            let tags: Vec<String> = renames.iter().flat_map(|r| r.from.clone()).collect();
            match renames[0].to.as_deref().and_then(tag_parent) {
                Some(into)
                    if renames
                        .iter()
                        .all(|r| r.to.as_deref().and_then(tag_parent) == Some(into)) =>
                {
                    format!("move the tags {} into “{into}”", listed(&tags))
                }
                _ => format!("move the tags {}", listed(&tags)),
            }
        }
    }
}

/// The new name `to` as typed, made one and checked against `from`.
fn new_name(from: &[String], to: &str) -> anyhow::Result<String> {
    let mut names = parse_tags(to);
    anyhow::ensure!(names.len() <= 1, "a tag's name has no comma");
    let to = names
        .pop()
        .ok_or_else(|| anyhow::anyhow!("what new name?"))?;
    anyhow::ensure!(
        from.len() > 1 || to != from[0],
        "the tag is already called “{to}”"
    );
    if let Some(f) = from
        .iter()
        .find(|f| !same_tag(f, &to) && tag_within(&to, f))
    {
        anyhow::bail!("“{f}” cannot go inside itself");
    }
    Ok(to)
}

/// Start renaming `tags` to `to` (merging them into it when several;
/// taking them away when `None`) on every file under `root`: checks what
/// it can now, finds the files on a worker, and proposes the plan.
pub fn start(
    app: &App,
    lifetime: &CancelToken,
    follows: &crate::follow::Ctx,
    tags: Vec<String>,
    to: Option<&str>,
    root: Option<PathBuf>,
) -> anyhow::Result<()> {
    anyhow::ensure!(!tags.is_empty(), "which tag?");
    let to = to.map(|to| new_name(&tags, to)).transpose()?;
    start_all(
        app,
        lifetime,
        follows,
        vec![Rename { from: tags, to }],
        root,
    )
}

/// Move each of `tags` into the category `into` (out of its category
/// when `None`), keeping its own name: one rename each, in one plan.
pub fn move_into(
    app: &App,
    lifetime: &CancelToken,
    follows: &crate::follow::Ctx,
    tags: Vec<String>,
    into: Option<&str>,
    root: Option<PathBuf>,
) -> anyhow::Result<()> {
    anyhow::ensure!(!tags.is_empty(), "which tag?");
    let into = into
        .map(skwir_core::metadata::clean_tag)
        .filter(|c| !c.is_empty());
    anyhow::ensure!(
        into.as_deref().is_none_or(|c| !c.contains(',')),
        "a category's name has no comma"
    );
    let mut renames = Vec::new();
    for tag in tags {
        let to = match &into {
            Some(into) => format!("{into}/{}", tag_leaf(&tag)),
            None => tag_leaf(&tag).to_owned(),
        };
        if to == tag {
            continue;
        }
        if into.as_deref().is_some_and(|into| tag_within(into, &tag)) {
            anyhow::bail!("“{tag}” cannot go inside itself");
        }
        renames.push(Rename {
            from: vec![tag],
            to: Some(to),
        });
    }
    anyhow::ensure!(
        !renames.is_empty(),
        "{}",
        into.map_or_else(
            || "these tags are in no category".to_owned(),
            |c| format!("these tags are in “{c}” already")
        )
    );
    start_all(app, lifetime, follows, renames, root)
}

/// Start `renames` on every file under `root`, in one plan.
fn start_all(
    app: &App,
    lifetime: &CancelToken,
    follows: &crate::follow::Ctx,
    renames: Vec<Rename>,
    root: Option<PathBuf>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        app.commands.get("query.location").is_some(),
        "finding the tagged files needs the Search plugin"
    );
    let place = root.as_ref().map_or_else(
        || "the home folder".to_owned(),
        |r| format!("“{}”", r.display()),
    );
    let tags: Vec<String> = renames.iter().flat_map(|r| r.from.clone()).collect();
    let names = listed(&tags);
    app.notify(
        NotifyLevel::Info,
        format!("Looking for the files tagged {names} in {place}…"),
    );
    let weak = app.weak();
    let cancel = lifetime.child();
    // On every file a tag names, its pin and color follow it.
    let follow: Vec<crate::follow::Follow> = crate::follow::everywhere(root.as_deref())
        .map(|home| {
            renames
                .iter()
                .map(|r| {
                    crate::follow::Follow::new(follows, r.from.clone(), r.to.clone(), home.clone())
                })
                .collect()
        })
        .unwrap_or_default();
    let title = title_of(&renames);
    app.scheduler.spawn(
        Priority::Normal,
        "tags.everywhere",
        cancel.clone(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            let mut files: Vec<Found> = Vec::new();
            let mut seen = HashSet::new();
            for tag in &tags {
                match tagged(&app, tag, root.as_ref(), &cancel) {
                    Ok(found) => gather(&mut files, &mut seen, found),
                    Err(_) if cancel.is_cancelled() => return,
                    Err(error) => {
                        app.notify(
                            NotifyLevel::Error,
                            format!("The files tagged “{tag}” were not found: {error:#}"),
                        );
                        return;
                    }
                }
            }
            // The change, applied to what each file holds when its step
            // is: what else it was given or lost meanwhile stays
            // (notes.md, "Writing").
            let change = skwir_core::notes::TagChange {
                rename: renames
                    .iter()
                    .flat_map(|r| {
                        r.from.iter().map(|from| skwir_core::notes::TagRename {
                            from: from.clone(),
                            to: r.to.clone(),
                        })
                    })
                    .collect(),
                ..Default::default()
            };
            let mut plan = Plan::new(title);
            let mut merged = 0;
            for (r, read) in files {
                // What it holds, to leave out a file it would not change,
                // and to say how many it merges: as the search read it, or
                // read now.
                let current = read.unwrap_or_else(|| {
                    app.notes
                        .read_path(&r.path, &app.notes.policy())
                        .tag_names()
                });
                let changed = change.applied(&current);
                if changed == current {
                    continue;
                }
                // Two tags became one: the file had several of them, or
                // the new name already.
                if changed.len() < current.len() && renames.iter().all(|r| r.to.is_some()) {
                    merged += 1;
                }
                plan.push(app.notes.edit(r).change_tags(change.clone()));
            }
            let n = plan.steps.len();
            if n > 0 {
                for f in follow {
                    if f.target(&app).is_some() {
                        plan.steps.push(Box::new(f));
                    }
                }
            }
            if n == 0 {
                app.notify(
                    NotifyLevel::Info,
                    format!("No file in {place} is tagged {names}"),
                );
                return;
            }
            let files = if n == 1 {
                "1 file".to_owned()
            } else {
                format!("{n} files")
            };
            let merged = match renames.as_slice() {
                _ if merged == 0 => String::new(),
                [Rename { to: Some(to), .. }] => {
                    format!(" {merged} already had “{to}” or several of them: they keep it once.")
                }
                _ => format!(" {merged} had two of these names: they keep it once."),
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
    use skwir_core::metadata::renamed_tags as renamed;

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
        // A category takes what is within it.
        let file = tags(&["Places/France/Paris", "Placesque", "places"]);
        assert_eq!(
            renamed(&file, "Places", Some("Lieux")),
            ["Lieux/France/Paris", "Placesque", "Lieux"]
        );
        assert_eq!(
            renamed(&file, "places/france", None),
            ["Placesque", "places"]
        );
        assert_eq!(
            renamed(&tags(&["Paris"]), "Paris", Some("Places/France/Paris")),
            ["Places/France/Paris"]
        );
    }

    #[test]
    fn a_review_says_what_moves_where() {
        let r = |from: &[&str], to: Option<&str>| Rename {
            from: tags(from),
            to: to.map(str::to_owned),
        };
        assert_eq!(
            title_of(&[r(&["Paris"], Some("Places/Paris"))]),
            "move the tag “Paris” into “Places”"
        );
        assert_eq!(
            title_of(&[r(&["Places/Paris"], Some("Paris"))]),
            "move the tag “Places/Paris” out of its category"
        );
        assert_eq!(
            title_of(&[r(&["a"], Some("x/a")), r(&["b"], Some("x/b"))]),
            "move the tags “a” and “b” into “x”"
        );
        assert_eq!(
            title_of(&[r(&["a"], Some("b"))]),
            "rename the tag “a” to “b”"
        );
        assert!(
            new_name(&tags(&["Places"]), "Places/Old").is_err(),
            "inside itself"
        );
        assert_eq!(new_name(&tags(&["a"]), " x / b ").unwrap(), "x/b");
    }

    /// The files of several tags, each once in the order found, at a size
    /// where comparing each with every one before it would not end.
    #[test]
    fn the_files_of_several_tags_are_gathered_once_each() {
        const N: u64 = 100_000;
        let refs = |from: u64| -> Vec<Found> {
            (from..from + N)
                .map(|i| {
                    let path = PathBuf::from(format!("/synthetic/tagged/file {i}.txt"));
                    (EntryRef::new(skwir_core::FileId::new(1, i), path), None)
                })
                .collect()
        };
        let (mut files, mut seen) = (Vec::new(), HashSet::new());
        gather(&mut files, &mut seen, refs(0));
        // A second tag on the same files, and on as many others.
        gather(&mut files, &mut seen, refs(0));
        gather(&mut files, &mut seen, refs(N));
        assert_eq!(files.len(), 2 * N as usize);
        assert!(
            files
                .iter()
                .zip(refs(0).into_iter().chain(refs(N)))
                .all(|(a, b)| a.0.path == b.0.path),
            "in the order found"
        );
    }

    #[test]
    fn tags_named_once_and_listed_in_words() {
        assert_eq!(named(" a ".into(), tags(&["A", "b", ""])), ["a", "b"]);
        assert_eq!(listed(&tags(&["a"])), "“a”");
        assert_eq!(listed(&tags(&["a", "b", "c"])), "“a”, “b” and “c”");
    }
}
