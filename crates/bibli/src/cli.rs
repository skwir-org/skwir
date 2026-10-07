//! `bibli mcp` and `bibli cmd` (ADR-043): the command line's ways to the
//! running window. Both run before anything logs: `bibli mcp` speaks the
//! protocol on its standard output, and `bibli cmd` prints JSON there.

use std::ffi::OsString;

use bibli_core::App;
use bibli_core::app::AppConfig;
use bibli_core::command::Invocation;
use bibli_plugin_control::{Answer, PlanMode, channel, client};
use serde_json::{Map, Value};

const USAGE: &str = "usage: bibli cmd [--apply] <command> [<arguments, a JSON object>]";

const HELP: &str = "\
bibli, a file manager

usage: bibli [<folder>]        open a window on the folder (the last one, or home)
       bibli cmd [--apply] <command> [<arguments, a JSON object>]
                               run a command in the open window, print its result
       bibli mcp               serve an agent's MCP to the open window
       bibli --version         say which version this is

Agents and scripts reach the window once Settings › Plugins › Agents and
scripts is on. The palette (Ctrl+Shift+P) lists every command.";

/// Run the command line's subcommand, if `args` (after the program's name)
/// name one: its exit code. `None`: the window opens as before.
pub fn run(args: &[OsString]) -> Option<i32> {
    match args.first()?.to_str()? {
        "mcp" => Some(mcp()),
        "cmd" => Some(cmd(&args[1..])),
        "--version" | "-V" => {
            println!("bibli {}", env!("CARGO_PKG_VERSION"));
            Some(0)
        }
        "--help" | "-h" => {
            println!("{HELP}");
            Some(0)
        }
        _ => None,
    }
}

/// `bibli mcp`: what an agent's configuration starts. It carries the
/// agent's standard input and output to the window's channel, and with no
/// window listening says so and starts nothing.
fn mcp() -> i32 {
    let Some(address) = channel::address() else {
        eprintln!("bibli: no channel for agents on this system");
        return 1;
    };
    match client::bridge(&address) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("bibli: {error:#}");
            1
        }
    }
}

/// `bibli cmd [--apply] <command> [<arguments>]`: one command, in the
/// window when it listens, else in a bibli of its own without a window. Its
/// result as JSON on standard output; a plan printed, or applied with
/// `--apply`. Exit code 0, 1 when the command failed, 2 for a wrong usage.
fn cmd(args: &[OsString]) -> i32 {
    let apply = args.iter().any(|arg| arg == "--apply");
    let rest: Vec<&OsString> = args.iter().filter(|arg| *arg != "--apply").collect();
    let (Some(name), true) = (rest.first(), rest.len() <= 2) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let name = name.to_string_lossy().into_owned();
    let mut arguments = Map::new();
    if let Some(text) = rest.get(1) {
        let Ok(Value::Object(given)) = serde_json::from_str::<Value>(&text.to_string_lossy())
        else {
            eprintln!("bibli: the arguments are a JSON object\n{USAGE}");
            return 2;
        };
        arguments = given;
    }
    let mode = if apply {
        PlanMode::Apply
    } else {
        PlanMode::Print
    };
    let in_window =
        channel::address().map(|address| client::call(&address, &name, arguments.clone(), mode));
    let answer = match in_window {
        Some(Ok(answer)) => Ok(answer),
        Some(Err(error)) if !client::not_listening(&error) => Err(error),
        _ => without_window(&name, arguments, mode),
    };
    // A command refused says why on standard error, whether the window or
    // a bibli of its own refused it.
    let answer = answer.and_then(|answer| match &answer.value {
        Value::Object(object) if answer.failed && object.len() == 1 => match object.get("error") {
            Some(Value::String(error)) => Err(anyhow::anyhow!("{error}")),
            _ => Ok(answer),
        },
        _ => Ok(answer),
    });
    match answer {
        Ok(answer) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&answer.value).unwrap_or_default()
            );
            i32::from(answer.failed)
        }
        Err(error) => {
            eprintln!("bibli: {error:#}");
            1
        }
    }
}

/// No window listens: `name` runs in a bibli of its own without a window,
/// its plugins but those that would answer the desktop or listen
/// themselves (agents and scripts, media controls).
fn without_window(
    name: &str,
    arguments: Map<String, Value>,
    mode: PlanMode,
) -> anyhow::Result<Answer> {
    anyhow::ensure!(
        !name.starts_with("ui."),
        "{name} acts on bibli's window: open bibli and turn on Agents and scripts in Settings, Plugins"
    );
    let app = App::new(AppConfig::default())?;
    for plugin in crate::plugins() {
        if !matches!(plugin.id(), bibli_plugin_control::ID | "mpris") {
            app.plugins.add(plugin, &app);
        }
    }
    let invocation = Invocation {
        name: name.into(),
        args: Value::Object(arguments),
    };
    bibli_plugin_control::run(&app, &invocation, mode, &|| false)
}
