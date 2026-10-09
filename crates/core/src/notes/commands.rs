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
use crate::op::{EntryRef, Plan};

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
    title: String,
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
        "core",
        "notes.rate",
        "Rate",
        "Rate the entries given, or the selection, from 1 to 5 stars, or remove their rating with 0 (plan)",
        |a, app| {
            anyhow::ensure!(a.stars <= 5, "a rating is from 1 to 5 stars");
            let rating = (a.stars > 0).then_some(u64::from(a.stars) * 2);
            let title = rating.map_or_else(
                || "remove the rating".to_owned(),
                |r| format!("rate {}", super::stars(r)),
            );
            per_entry(app, a.targets, title, |r| app.notes.edit(r).rating(rating))
        },
    )
    .plans();
    c.register::<CommentArgs, _>(
        "core",
        "notes.comment",
        "Comment",
        "Set the comment of the entries given, or the selection's, or remove it with an empty text (plan)",
        |a, app| {
            let text = a.text.trim();
            let comment = (!text.is_empty()).then(|| text.to_owned());
            let title = if comment.is_some() {
                "set the comment"
            } else {
                "remove the comment"
            };
            per_entry(app, a.targets, title.to_owned(), |r| {
                app.notes.edit(r).comment(comment.clone())
            })
        },
    )
    .plans();
    c.register::<Targets, _>(
        "core",
        "notes.agree",
        "Make notes agree",
        "Write the notes of the entries given, or the selection's, as read from every store, to each store that keeps them, so the stores that differ agree again (plan)",
        |a, app| {
            // What the stores say together is written to each when the
            // plan is applied, as it is read then (`SetNotes::applied_to`).
            per_entry(app, a, "make the notes agree".to_owned(), |r| {
                app.notes.edit(r).sync()
            })
        },
    )
        .plans();
}
