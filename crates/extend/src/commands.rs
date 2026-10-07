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

use bibli_core::app::NotifyLevel;
use bibli_core::command::{Effect, Invocation, Outcome, is_nothing_selected};
use bibli_core::op::{self, EntryArg, EntryRef, Operation, Plan};
use bibli_core::permissions::Capability;
use bibli_core::scheduler::Priority;
use bibli_core::workspace::PaneId;
use bibli_core::{App, CancelToken};
use bibli_plugin_api::manifest::Effect as Declared;
use bibli_plugin_api::protocol::{CommandAnswer, CommandParams, Step, method};
use serde_json::{Value, json};

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
        let (name, title) = (declared.name.clone(), declared.title.clone());
        app.commands
            .register_dynamic(
                runner.id,
                &declared.name,
                &declared.title,
                &declared.description,
                schema,
                move |args, app| run(app, &runner, &name, &title, args),
            )
            .effect(match declared.effect {
                Declared::Reads => Effect::Reads,
                Declared::Shows => Effect::Shows,
                Declared::Plans => Effect::Plans,
                Declared::Acts => Effect::Acts,
            });
    }
}

/// Run the command `name` with `args`: on a worker when asked from the
/// window, here otherwise.
fn run(
    app: &App,
    runner: &Arc<Runner>,
    name: &str,
    title: &str,
    mut args: Value,
) -> anyhow::Result<Outcome> {
    let object = args
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("the arguments are an object"))?;
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
    let privacy = app.privacy();
    let mut check = privacy.checker();
    anyhow::ensure!(
        !refs.iter().any(|r| check.holds(&r.path)) && check.named_in(&args).is_none(),
        "a private file: a plugin from outside bibli does not see it (Settings, Privacy)"
    );
    if !bibli_core::scheduler::on_window_thread() {
        return ask(app, runner, name, args, &refs, CancelToken::new());
    }
    let (weak, runner, name, title) = (
        app.weak(),
        Arc::clone(runner),
        name.to_owned(),
        title.to_owned(),
    );
    let cancel = app.lifetime().child();
    app.scheduler.spawn(
        Priority::Normal,
        "plugin-command",
        cancel.clone(),
        move || {
            let Some(app) = weak.upgrade() else { return };
            match ask(&app, &runner, &name, args, &refs, cancel) {
                Ok(Outcome::Plan(plan)) => app.propose(plan),
                Ok(Outcome::Value(value)) => {
                    app.notify(NotifyLevel::Info, format!("{title}: {}", said(&value)));
                }
                Ok(Outcome::Clipboard { text, said }) => app.notify_with_action(
                    NotifyLevel::Info,
                    said,
                    "Copy",
                    Invocation::with_args("ui.copy_text", &json!({ "text": text })),
                ),
                Ok(Outcome::Done) => {}
                Err(error) => app.notify(NotifyLevel::Error, format!("{title}: {error:#}")),
            }
        },
    );
    Ok(Outcome::Done)
}

/// A value in a line, for a notification.
fn said(value: &Value) -> String {
    let text = match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    if text.chars().count() > 200 {
        format!("{}…", text.chars().take(200).collect::<String>())
    } else {
        text
    }
}

/// Ask the plugin, and its answer as the command's outcome.
fn ask(
    app: &App,
    runner: &Runner,
    name: &str,
    args: Value,
    refs: &[EntryRef],
    cancel: CancelToken,
) -> anyhow::Result<Outcome> {
    let paths: Vec<PathBuf> = refs.iter().map(|r| r.path.clone()).collect();
    let call = Call::new(paths, runner.granted, cancel);
    let entries = (0..refs.len())
        .map(|n| {
            let mime = refs[n]
                .path
                .file_name()
                .and_then(bibli_platform::mime::by_name)
                .map(|m| m.0);
            call.describe(n, mime)
        })
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
        CommandAnswer::Text { text, said } => Outcome::Clipboard { text, said },
        CommandAnswer::Plan { title, note, steps } => {
            anyhow::ensure!(
                runner.granted.has(Capability::Write),
                "this plugin may not change files"
            );
            let mut plan = Plan::with_steps(title, plan_steps(app, &steps, refs)?);
            if let Some(note) = note {
                plan = plan.with_note(note);
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
            .ok_or_else(|| anyhow::anyhow!("no file {handle} in this call"))
    };
    let mut folder = |to: &str| -> anyhow::Result<PathBuf> {
        let path = PathBuf::from(to);
        bibli_core::os_path::absolute(&path)?;
        anyhow::ensure!(
            !check.holds(&path),
            "a private folder: a plugin does not reach it"
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
