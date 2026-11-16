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
use skwir_core::words;
use skwir_core::words::{Name, Words};

use skwir_core::notes::parse_tags;

use crate::quoted_tags as listed;

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
/// hidden ones included, as its tag places count them, with their ids and
/// the tags the search read of them, as it finds them. Blocks until it
/// completes: call on a worker.
fn tagged(
    app: &App,
    tag: &str,
    root: Option<&PathBuf>,
    cancel: &CancelToken,
) -> anyhow::Result<Vec<Found>> {
    let mut args = serde_json::json!({ "facet": "tag", "value": tag, "hidden": true });
    if let Some(root) = root {
        args["root"] = serde_json::json!(PathArg::from(root.as_path()));
    }
    let no_location = || {
        words!(
            tags_search_no_location = "the search gave no location",
            note = "An error: the search that finds a tag's files did not answer as it should."
        )
    };
    let Outcome::Value(v) = app.dispatch(&Invocation::with_args("query.location", &args))? else {
        anyhow::bail!(no_location());
    };
    let location = v["location"]
        .as_str()
        .ok_or_else(|| anyhow::Error::from(no_location()))?;
    let open = app.open_location(&Location::parse(location))?;
    let mut found = Vec::new();
    let outcome = loop {
        if cancel.is_cancelled() {
            break Err(anyhow::Error::from(words!(
                tags_search_cancelled = "cancelled"
            )));
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
            Ok(ListingEvent::Removed(ids)) => {
                let gone: std::collections::HashSet<_> = ids.into_iter().collect();
                found.retain(|(id, _)| !gone.contains(id));
            }
            Ok(ListingEvent::Complete) => break Ok(found.into_iter().map(|(_, f)| f).collect()),
            Ok(ListingEvent::Error(e)) => break Err(anyhow::anyhow!("{e}")),
            Ok(_) | Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                break Err(anyhow::Error::from(words!(
                    tags_search_stopped = "the search stopped"
                )));
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

/// What `renames` does, for the review's title.
fn title_of(renames: &[Rename]) -> Words {
    let name = |tag: &str| Name(tag.to_owned());
    match renames {
        [Rename { from, to: Some(to) }] if from.len() == 1 => {
            let tag = &from[0];
            if tag_leaf(tag) == tag_leaf(to) && tag_parent(tag) != tag_parent(to) {
                match tag_parent(to) {
                    Some(into) => words!(
                        tags_move_into_plan = "move the tag { $tag } into { $into }",
                        tag = name(tag),
                        into = name(into),
                        note = "A plan's title. into: a category of tags (Places/France)."
                    ),
                    None => words!(
                        tags_move_out_plan = "move the tag { $tag } out of its category",
                        tag = name(tag),
                        note = "A plan's title."
                    ),
                }
            } else {
                words!(
                    tags_rename_plan = "rename the tag { $tag } to { $to }",
                    tag = name(tag),
                    to = name(to),
                    note = "A plan's title."
                )
            }
        }
        [Rename { from, to: Some(to) }] => words!(
            tags_merge_plan = "merge the tags { $tags } into { $to }",
            tags = listed(from),
            to = name(to),
            note = "A plan's title. tags: the tags' names, each quoted, the last after “and”."
        ),
        [Rename { from, to: None }] if from.len() == 1 => words!(
            tags_remove_everywhere_plan = "remove the tag { $tag } from every file",
            tag = name(&from[0]),
            note = "A plan's title."
        ),
        [Rename { from, to: None }] => words!(
            tags_remove_several_everywhere_plan = "remove the tags { $tags } from every file",
            tags = listed(from),
            note = "A plan's title. tags: the tags' names, each quoted, the last after “and”."
        ),
        _ => {
            let tags: Vec<String> = renames.iter().flat_map(|r| r.from.clone()).collect();
            match renames[0].to.as_deref().and_then(tag_parent) {
                Some(into)
                    if renames
                        .iter()
                        .all(|r| r.to.as_deref().and_then(tag_parent) == Some(into)) =>
                {
                    words!(
                        tags_move_several_into_plan = "move the tags { $tags } into { $into }",
                        tags = listed(&tags),
                        into = name(into),
                        note = "A plan's title. tags: the tags' names, each quoted, the last after “and”; into: a category of tags (Places/France)."
                    )
                }
                _ => words!(
                    tags_move_several_plan = "move the tags { $tags }",
                    tags = listed(&tags),
                    note =
                        "A plan's title. tags: the tags' names, each quoted, the last after “and”."
                ),
            }
        }
    }
}

/// Why a tag cannot be put inside `tag`: it is within it.
fn inside_itself(tag: &str) -> Words {
    words!(
        tags_inside_itself = "{ $tag } cannot go inside itself",
        tag = Name(tag.to_owned()),
        note = "An error: a tag moved or renamed into a category within it."
    )
}

/// The new name `to` as typed, made one and checked against `from`.
fn new_name(from: &[String], to: &str) -> anyhow::Result<String> {
    let mut names = parse_tags(to);
    anyhow::ensure!(
        names.len() <= 1,
        words!(tags_no_comma = "a tag's name has no comma")
    );
    let to = names
        .pop()
        .ok_or_else(|| anyhow::Error::from(words!(tags_what_new_name = "what new name?")))?;
    anyhow::ensure!(
        from.len() > 1 || to != from[0],
        words!(
            tags_already_called = "the tag is already called { $to }",
            to = Name(to.clone())
        )
    );
    if let Some(f) = from
        .iter()
        .find(|f| !same_tag(f, &to) && tag_within(&to, f))
    {
        anyhow::bail!(inside_itself(f));
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
    anyhow::ensure!(!tags.is_empty(), crate::which_tag());
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
    anyhow::ensure!(!tags.is_empty(), crate::which_tag());
    let into = into
        .map(skwir_core::metadata::clean_tag)
        .filter(|c| !c.is_empty());
    anyhow::ensure!(
        into.as_deref().is_none_or(|c| !c.contains(',')),
        words!(tags_category_no_comma = "a category's name has no comma")
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
            anyhow::bail!(inside_itself(&tag));
        }
        renames.push(Rename {
            from: vec![tag],
            to: Some(to),
        });
    }
    anyhow::ensure!(
        !renames.is_empty(),
        into.map_or_else(
            || words!(tags_in_no_category = "these tags are in no category"),
            |c| words!(
                tags_in_category_already = "these tags are in { $category } already",
                category = Name(c),
                note = "An error: the tags moved into a category are there already."
            )
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
        words!(tags_needs_search = "finding the tagged files needs the Search plugin")
    );
    // Where the files are looked for: the home folder, else a path as it is.
    let place = root.as_ref().map(|r| r.display().to_string());
    let tags: Vec<String> = renames.iter().flat_map(|r| r.from.clone()).collect();
    let names = listed(&tags);
    app.notify(
        NotifyLevel::Info,
        match &place {
            None => words!(
                tags_looking_home = "Looking for the files tagged { $tags } in the home folder…",
                tags = names.as_str(),
                note = "tags: the tags' names, each quoted, the last after “and”."
            ),
            Some(root) => words!(
                tags_looking_in = "Looking for the files tagged { $tags } in “{ $root }”…",
                tags = names.as_str(),
                root = root.as_str(),
                note = "tags: the tags' names, each quoted, the last after “and”; root: a folder's path."
            ),
        },
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
                            words!(
                                tags_files_not_found = "The files tagged { $tag } were not found: { $error }",
                                tag = Name(tag.clone()),
                                error = error
                            ),
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
                    match &place {
                        None => words!(
                            tags_none_tagged_home = "No file in the home folder is tagged { $tags }",
                            tags = names.as_str(),
                            note = "tags: the tags' names, each quoted, the last after “and”."
                        ),
                        Some(root) => words!(
                            tags_none_tagged_in = "No file in “{ $root }” is tagged { $tags }",
                            root = root.as_str(),
                            tags = names.as_str(),
                            note = "tags: the tags' names, each quoted, the last after “and”; root: a folder's path."
                        ),
                    },
                );
                return;
            }
            // How many files kept one tag where they had two, said after
            // what the plan holds.
            let merged = match renames.as_slice() {
                _ if merged == 0 => None,
                [Rename { to: Some(to), .. }] => Some(words!(
                    tags_merged_into = "{ $merged } already had { $to } or several of them: they keep it once.",
                    merged = merged,
                    to = Name(to.clone()),
                    note = "Said of a plan renaming tags. merged: how many files; to: the tags' new name."
                )),
                _ => Some(words!(
                    tags_merged_names = "{ $merged } had two of these names: they keep it once.",
                    merged = merged,
                    note = "Said of a plan renaming tags. merged: how many files."
                )),
            };
            let note = match (&place, merged) {
                (None, None) => words!(
                    tags_plan_note_home = "{ $count ->
                            [one] { $count } file
                           *[other] { $count } files
                        } in the home folder; what your exclusions leave out is left out.",
                    count = n,
                    note = "Said of a plan renaming or removing tags on every file: how many files it changes."
                ),
                (None, Some(merged)) => words!(
                    tags_plan_note_home_merged = "{ $count ->
                            [one] { $count } file
                           *[other] { $count } files
                        } in the home folder; what your exclusions leave out is left out. { $merged }",
                    count = n,
                    merged = merged,
                    note = "As tags_plan_note_home, then merged: how many files kept one tag of two (tags_merged_into, tags_merged_names)."
                ),
                (Some(root), None) => words!(
                    tags_plan_note_in = "{ $count ->
                            [one] { $count } file
                           *[other] { $count } files
                        } in “{ $root }”; what your exclusions leave out is left out.",
                    count = n,
                    root = root.as_str(),
                    note = "Said of a plan renaming or removing tags on every file under a folder: how many files it changes. root: the folder's path."
                ),
                (Some(root), Some(merged)) => words!(
                    tags_plan_note_in_merged = "{ $count ->
                            [one] { $count } file
                           *[other] { $count } files
                        } in “{ $root }”; what your exclusions leave out is left out. { $merged }",
                    count = n,
                    root = root.as_str(),
                    merged = merged,
                    note = "As tags_plan_note_in, then merged: how many files kept one tag of two (tags_merged_into, tags_merged_names)."
                ),
            };
            // Said now: a plan's note is text until it is words.
            app.propose(plan.with_note(note.say()));
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
            title_of(&[r(&["Paris"], Some("Places/Paris"))]).english(),
            "move the tag “Paris” into “Places”"
        );
        assert_eq!(
            title_of(&[r(&["Places/Paris"], Some("Paris"))]).english(),
            "move the tag “Places/Paris” out of its category"
        );
        assert_eq!(
            title_of(&[r(&["a"], Some("x/a")), r(&["b"], Some("x/b"))]).english(),
            "move the tags “a” and “b” into “x”"
        );
        assert_eq!(
            title_of(&[r(&["a"], Some("b"))]).english(),
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
