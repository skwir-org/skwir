//! The notes' own commands, the core's because the notes are: a rating, a
//! comment, the stores made to agree. Each is a plan of one `set_notes`
//! step per file, whichever plugins' stores keep them. Nothing is read to
//! make it: the command runs where it is dispatched, the window's thread
//! for a key, and a step checks what its file holds when the plan is
//! applied (`SetNotes::check`: one that already has it is done), so a
//! file already rated is skipped there, not read here.

use schemars::JsonSchema;
use serde::Deserialize;

use crate::app::App;
use crate::app::TargetsArgs as Targets;
use crate::command::Outcome;
use crate::command::Topic;
use crate::op::{EntryRef, Plan};
use crate::words;
use crate::words::Words;

#[derive(Debug, Deserialize, JsonSchema)]
struct RateArgs {
    /// From 1 to 5 stars; 0 removes the rating.
    stars: u8,
    #[serde(flatten)]
    targets: Targets,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CommentArgs {
    /// The comment; empty removes it.
    #[serde(default)]
    text: String,
    #[serde(flatten)]
    targets: Targets,
}

/// A plan of one step per target; none at all is `NothingSelected`, from
/// `App::targets`.
fn per_entry(
    app: &App,
    targets: Targets,
    title: Words,
    step: impl Fn(EntryRef) -> super::SetNotes,
) -> anyhow::Result<Outcome> {
    let refs = app.targets(targets.pane, targets.entries)?;
    let mut plan = Plan::new(title);
    for r in refs {
        plan.push(step(r));
    }
    Ok(Outcome::Plan(plan))
}

pub(crate) fn register(app: &App) {
    let c = &app.commands;
    c.register::<RateArgs, _>(
        "core", Topic::Notes,
        "notes.rate",
        words!(notes_rate_command = "Rate"),
        words!(
            notes_rate_description = "Rate the entries given, or the selection, from 1 to 5 stars, or remove their rating with 0 (plan)",
            note = "Read by agents and in F1's list of actions."
        ),
        |a, app| {
            anyhow::ensure!(
                a.stars <= 5,
                words!(notes_rating_stars = "a rating is from 1 to 5 stars")
            );
            let rating = (a.stars > 0).then_some(u64::from(a.stars) * 2);
            let title = rating.map_or_else(
                || {
                    words!(
                        notes_rate_none_title = "remove the rating",
                        note = "A plan's title: the rating of the files is removed. Begins in lower case."
                    )
                },
                |r| {
                    words!(
                        notes_rate_title = "rate { $stars }",
                        stars = super::stars(r),
                        note = "A plan's title: the files are rated, in stars (rate 4 stars). Begins in lower case."
                    )
                },
            );
            per_entry(app, a.targets, title, |r| app.notes.edit(r).rating(rating))
        },
    )
    .plans();
    c.register::<CommentArgs, _>(
        "core", Topic::Notes,
        "notes.comment",
        words!(notes_comment_command = "Comment"),
        words!(
            notes_comment_description = "Set the comment of the entries given, or the selection's, or remove it with an empty text (plan)",
            note = "Read by agents and in F1's list of actions."
        ),
        |a, app| {
            let text = a.text.trim();
            let comment = (!text.is_empty()).then(|| text.to_owned());
            let title = if comment.is_some() {
                words!(
                    notes_comment_title = "set the comment",
                    note = "A plan's title: the files' comment is set. Begins in lower case."
                )
            } else {
                words!(
                    notes_uncomment_title = "remove the comment",
                    note = "A plan's title: the files' comment is removed. Begins in lower case."
                )
            };
            per_entry(app, a.targets, title, |r| {
                app.notes.edit(r).comment(comment.clone())
            })
        },
    )
    .plans();
    c.register::<Targets, _>(
        "core", Topic::Notes,
        "notes.agree",
        words!(notes_agree_command = "Make notes agree"),
        words!(
            notes_agree_description = "Write the notes of the entries given, or the selection's, as read from every store, to each store that keeps them, so the stores that differ agree again (plan)",
            note = "Read by agents and in F1's list of actions."
        ),
        |a, app| {
            // What the stores say together is written to each when the
            // plan is applied, as it is read then (`SetNotes::applied_to`).
            let title = words!(
                notes_agree_title = "make the notes agree",
                note = "A plan's title: the notes of the files are written alike to every place that keeps them. Begins in lower case."
            );
            per_entry(app, a, title, |r| {
                app.notes.edit(r).sync()
            })
        },
    )
        .plans();
}
