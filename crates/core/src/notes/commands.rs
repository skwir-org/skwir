//! The notes' own commands, the core's because the notes are: a rating, a
//! comment, the stores made to agree. Each is a plan of one `set_notes`
//! step per file it changes, built from what the file holds now (read
//! from its stores: a listing's field can be stale), whichever plugins'
//! stores keep them. Tags have their own commands, the tags plugin's:
//! a tag is also a place, a color, a pin.

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

/// A plan of one step per target, leaving out those it would not change;
/// none at all is `NothingSelected`, from `App::targets`.
fn per_entry(
    app: &App,
    targets: Targets,
    title: String,
    step: impl Fn(EntryRef) -> Option<super::SetNotes>,
) -> anyhow::Result<Outcome> {
    let refs = app.targets(targets.pane, targets.entries)?;
    let mut plan = Plan::new(title);
    for r in refs {
        if let Some(op) = step(r) {
            plan.push(op);
        }
    }
    Ok(Outcome::Plan(plan))
}

fn notes_of(app: &App, path: &std::path::Path) -> super::Notes {
    app.notes.read_path(path, &app.notes.policy())
}

pub(crate) fn register(app: &App) {
    let c = &app.commands;
    c.register::<RateArgs, _>(
        "core",
        "notes.rate",
        "Rate",
        "Rate the selection from 1 to 5 stars, or remove its rating with 0 (plan)",
        |a, app| {
            anyhow::ensure!(a.stars <= 5, "a rating is from 1 to 5 stars");
            let rating = (a.stars > 0).then_some(u64::from(a.stars) * 2);
            let title = rating.map_or_else(
                || "remove the rating".to_owned(),
                |r| format!("rate {}", super::stars(r)),
            );
            per_entry(app, a.targets, title, |r| {
                (notes_of(app, &r.path).rating != rating).then(|| app.notes.edit(r).rating(rating))
            })
        },
    )
    .plans();
    c.register::<CommentArgs, _>(
        "core",
        "notes.comment",
        "Comment",
        "Set the selection's comment, or remove it with an empty text (plan)",
        |a, app| {
            let text = a.text.trim();
            let comment = (!text.is_empty()).then(|| text.to_owned());
            let title = if comment.is_some() {
                "set the comment"
            } else {
                "remove the comment"
            };
            per_entry(app, a.targets, title.to_owned(), |r| {
                (notes_of(app, &r.path).comment != comment)
                    .then(|| app.notes.edit(r).comment(comment.clone()))
            })
        },
    )
    .plans();
    c.register::<Targets, _>(
        "core",
        "notes.agree",
        "Make notes agree",
        "Write the selection's notes, as read from every store, to each store that keeps them, so the stores that differ agree again (plan)",
        |a, app| {
            per_entry(app, a, "make the notes agree".to_owned(), |r| {
                let read = app.notes.read_one(&r.path, &app.notes.policy());
                (!read.differ.is_empty()).then(|| {
                    app.notes
                        .edit(r)
                        .tags(read.notes.tag_names())
                        .rating(read.notes.rating)
                        .comment(read.notes.comment)
                        .sync()
                })
            })
        },
    )
        .plans();
}
