//! A plugin from outside's commands, in the core's registry like any: its
//! arguments as its manifest's schema says, and the files it acts on as
//! every command takes them (`entries`, else a pane's selection,
//! `App::targets`). Asked from the window, its answer is waited for on a
//! worker: a plan is proposed for review, a value or a text said in a
//! notification. Asked from anywhere else (an agent, a script, a worker),
//! it is waited for where it is and answered as any command is.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use skwir_core::app::NotifyLevel;
use skwir_core::command::{Effect, Invocation, Outcome, Topic, is_nothing_selected};
use skwir_core::op::{self, EntryArg, EntryRef, Operation, Plan};
use skwir_core::permissions::Capability;
use skwir_core::scheduler::Priority;
use skwir_core::words;
use skwir_core::words::Words;
use skwir_core::workspace::PaneId;
use skwir_core::{App, CancelToken};
use skwir_plugin_api::manifest::Effect as Declared;
use skwir_plugin_api::protocol::{CommandAnswer, CommandParams, Step, method};

use crate::call::Call;
use crate::external::{External, Runner};

/// Register `plugin`'s commands.
pub(crate) fn register(app: &App, plugin: &External) {
    for declared in &plugin.manifest().commands {
        let mut schema = declared
            .arguments
            .clone()
            .unwrap_or_else(|| json!({ "type": "object" }));
        if !schema.get("properties").is_some_and(Value::is_object) {
            schema["properties"] = json!({});
        }
        // What the host takes for every command.
        schema["properties"]["entries"] = json!({
            "type": "array",
            "description": "The files to act on, by absolute path; the pane's selection when omitted.",
            "items": { "type": "string" },
        });
        schema["properties"]["pane"] = json!({ "type": "integer" });
        let runner = Arc::clone(&plugin.runner);
        // Its own words: as written, or messages of its catalogs, said in
        // the window's language and to agents in English (protocol 2).
        let title = runner.catalogs.said(&declared.title);
        let description = runner.catalogs.said(&declared.description);
        let name = declared.name.clone();
        let said = title.clone();
        app.commands
            .register_dynamic(
                runner.id,
                topic_of(plugin, declared.topic),
                &declared.name,
                title,
                description,
                schema,
                move |args, app| run(app, &runner, &name, &said, args),
            )
            // Not offered in a window answering another program's dialog
            // (ADR-144, 1), whatever it is about: a program or a module may
            // change files or start programs, which the host cannot tell,
            // and its manifest cannot yet say a command is fit for a dialog.
            .not_in_dialogs()
            .effect(match declared.effect {
                Declared::Reads => Effect::Reads,
                Declared::Shows => Effect::Shows,
                Declared::Plans => Effect::Plans,
                Declared::Acts => Effect::Acts,
            });
    }
}

/// What a command is about: what its manifest says, else what its
/// plugin's category is about.
fn topic_of(plugin: &External, said: Option<skwir_plugin_api::manifest::Topic>) -> Topic {
    use skwir_plugin_api::manifest::{Category, Topic as Said};
    match said {
        Some(Said::Go) => Topic::Go,
        Some(Said::Select) => Topic::Select,
        Some(Said::Files) => Topic::Files,
        Some(Said::Find) => Topic::Find,
        Some(Said::Notes) => Topic::Notes,
        Some(Said::Tabs) => Topic::Tabs,
        Some(Said::View) => Topic::View,
        Some(Said::Preview) => Topic::Preview,
        Some(Said::Player) => Topic::Player,
        Some(Said::Terminal) => Topic::Terminal,
        Some(Said::Places) => Topic::Places,
        Some(Said::Window) => Topic::Window,
        Some(Said::Skwir) => Topic::Skwir,
        None => match plugin.manifest().category {
            Category::Browsing => Topic::Go,
            Category::Files | Category::Desktop => Topic::Files,
            Category::Previews => Topic::Preview,
        },
    }
}

/// Run the command `name` with `args`: on a worker when asked from the
/// window, here otherwise.
fn run(
    app: &App,
    runner: &Arc<Runner>,
    name: &str,
    title: &Words,
    mut args: Value,
) -> anyhow::Result<Outcome> {
    let object = args
        .as_object_mut()
        .ok_or_else(|| {
            anyhow::anyhow!(words!(
                extend_arguments_object = "the arguments are an object",
                note = "An error for agents and scripts: a command of a plugin from outside Skwir takes its arguments as a JSON object. In lower case."
            ))
        })?;
    let entries: Vec<EntryArg> = object
        .remove("entries")
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    let pane: Option<PaneId> = object
        .remove("pane")
        .map(serde_json::from_value)
        .transpose()?;
    // A command that needs no file runs with none.
    let refs = match app.targets(pane, entries) {
        Err(error) if is_nothing_selected(&error) => Vec::new(),
        refs => refs?,
    };
    if !skwir_core::scheduler::on_window_thread() {
        return ask(app, runner, name, args, &refs, CancelToken::new());
    }
    let (weak, runner, name, title) = (
        app.weak(),
        Arc::clone(runner),
        name.to_owned(),
        title.clone(),
    );
    let cancel = app.lifetime().child();
    // What it asks reads the files it is handed (their types, privacy):
    // in their remote's lane, where a server that stops answering holds
    // only its own (ADR-077).
    let lane = refs
        .iter()
        .map(|r| skwir_core::fs::reach::lane(&r.path))
        .find(|lane| !lane.is_shared())
        .unwrap_or(skwir_core::scheduler::Lane::SHARED);
    app.scheduler.spawn_in(
        lane,
        Priority::Normal,
        "plugin-command",
        cancel.clone(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            // Its command's title and what it answers are its own words.
            match ask(&app, &runner, &name, args, &refs, cancel) {
                Ok(Outcome::Plan(plan)) => app.propose(plan),
                Ok(Outcome::Value(value)) => {
                    app.notify(
                        NotifyLevel::Info,
                        words!(
                            extend_answered = "{ $command }: { $value }",
                            command = title,
                            value = said(&value),
                            note = "A notification: what a command of a plugin from outside Skwir answered. Its title (the plugin's words), then its answer, as the plugin wrote it."
                        ),
                    );
                }
                Ok(Outcome::Clipboard { text, said }) => app.notify_with_action(
                    NotifyLevel::Info,
                    said,
                    words!(
                        extend_copy_answer = "Copy",
                        note = "A notification's button: copies the text a plugin from outside Skwir answered."
                    ),
                    Invocation::with_args("ui.copy_text", &json!({ "text": text })),
                ),
                // A plugin from outside answers no files (`CommandAnswer`).
                Ok(Outcome::Done | Outcome::Files { .. }) => {}
                Err(error) => app.notify(
                    NotifyLevel::Error,
                    words!(
                        extend_command_failed = "{ $command }: { $error }",
                        command = title,
                        error = error,
                        note = "A notification: a command of a plugin from outside Skwir failed. Its title (the plugin's words), then why."
                    ),
                ),
            }
        },
    );
    Ok(Outcome::Done)
}

/// A value in a line, for a notification: as the plugin wrote it, its
/// start when long.
fn said(value: &Value) -> Words {
    let text = match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    if text.chars().count() > 200 {
        words!(
            extend_answer_cut = "{ $start }…",
            start = text.chars().take(200).collect::<String>(),
            note = "What a plugin from outside Skwir answered, too long for a notification: its start, then a mark that more was cut."
        )
    } else {
        Words::as_written(text)
    }
}

/// Ask the plugin, and its answer as the command's outcome. Nothing private
/// reaches it: the markers are read here, never on the window's thread.
fn ask(
    app: &App,
    runner: &Runner,
    name: &str,
    args: Value,
    refs: &[EntryRef],
    cancel: CancelToken,
) -> anyhow::Result<Outcome> {
    let privacy = app.privacy();
    let mut check = privacy.checker();
    anyhow::ensure!(
        !refs.iter().any(|r| check.holds(&r.path)) && check.named_in(&args).is_none(),
        words!(
            extend_private_file =
                "a private file: a plugin from outside Skwir does not see it (Settings, Privacy)",
            note = "An error: a command of a plugin from outside Skwir was given a file the user keeps private. Settings, Privacy is where. In lower case."
        )
    );
    let paths: Vec<PathBuf> = refs.iter().map(|r| r.path.clone()).collect();
    let call = Call::new(paths, runner.granted, cancel);
    let entries = (0..refs.len())
        .map(|n| call.describe(n, crate::type_of(&refs[n].path)))
        .collect();
    let params = CommandParams {
        command: name.to_owned(),
        args,
        entries,
    };
    let answer = runner.call(method::COMMAND, serde_json::to_value(params)?, &call)?;
    Ok(match serde_json::from_value(answer)? {
        CommandAnswer::Done => Outcome::Done,
        CommandAnswer::Value { value } => Outcome::Value(value),
        // What a plugin from outside says: as it wrote it, or a message of
        // its catalogs.
        CommandAnswer::Text { text, said } => Outcome::Clipboard {
            text,
            said: runner.catalogs.said(&said),
        },
        CommandAnswer::Plan { title, note, steps } => {
            anyhow::ensure!(
                runner.granted.has(Capability::Write),
                words!(
                    extend_may_not_write = "this plugin may not change files",
                    note = "An error: a plugin from outside Skwir answered a plan, but did not declare it changes files. In lower case."
                )
            );
            // Its own words: as it wrote them, or messages of its catalogs.
            let mut plan =
                Plan::with_steps(runner.catalogs.said(&title), plan_steps(app, &steps, refs)?)
                    .proposed_by(runner.title.clone());
            if let Some(note) = note {
                plan = plan.with_note(runner.catalogs.said(&note));
            }
            Outcome::Plan(plan)
        }
    })
}

/// A plan's steps as the core's operations: a file by its handle, a
/// folder by an absolute path that is not private.
fn plan_steps(
    app: &App,
    steps: &[Step],
    refs: &[EntryRef],
) -> anyhow::Result<Vec<Box<dyn Operation>>> {
    let privacy = app.privacy();
    let mut check = privacy.checker();
    let file = |handle: u32| -> anyhow::Result<EntryRef> {
        usize::try_from(handle)
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|n| refs.get(n))
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!(words!(
                    extend_no_such_file = "no file { $handle } in this call",
                    handle = handle.to_string(),
                    note = "An error: a plan answered by a plugin from outside Skwir names a file by a number (its handle) the call gave it none. In lower case."
                ))
            })
    };
    let mut folder = |to: &str| -> anyhow::Result<PathBuf> {
        let path = PathBuf::from(to);
        skwir_core::os_path::absolute(&path)?;
        anyhow::ensure!(
            !check.holds(&path),
            words!(
                extend_private_folder = "a private folder: a plugin does not reach it",
                note = "An error: a plan answered by a plugin from outside Skwir names a folder the user keeps private. In lower case."
            )
        );
        Ok(path)
    };
    steps
        .iter()
        .map(|step| -> anyhow::Result<Box<dyn Operation>> {
            Ok(match step {
                Step::Rename { file: handle, name } => {
                    op::valid_file_name(OsStr::new(name))?;
                    Box::new(op::Rename {
                        entry: file(*handle)?,
                        new_name: name.into(),
                    })
                }
                Step::Trash { file: handle } => Box::new(op::Trash {
                    entry: file(*handle)?,
                }),
                Step::Move { file: handle, to } => Box::new(op::Move {
                    entry: file(*handle)?,
                    dest_dir: folder(to)?,
                    new_name: None,
                }),
                Step::Copy { file: handle, to } => Box::new(op::Copy::from_settings(
                    file(*handle)?,
                    folder(to)?,
                    &app.settings,
                )),
                Step::Mkdir { path } => Box::new(op::Mkdir {
                    path: folder(path)?,
                }),
            })
        })
        .collect()
}
