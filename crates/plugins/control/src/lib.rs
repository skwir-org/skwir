//! Agents and scripts drive the running window (ADR-043). While this
//! plugin is on, the window listens on a channel of the user's alone
//! (`channel`) and speaks MCP there, through `rmcp`, on a runtime of its
//! own: each command of the registry is a tool. A command that gives a
//! plan proposes it for review, and the tool answers once the user has:
//! applied, with what applied, or dismissed. `bibli mcp` carries an
//! agent's standard input and output to the channel, and `bibli cmd` runs
//! one command through it (`client`).

pub mod channel;
pub mod client;

use std::sync::{Arc, Weak};
use std::time::Duration;

use bibli_core::app::NotifyLevel;
use bibli_core::command::{Invocation, NoArgs, Outcome};
use bibli_core::op::Verdict;
use bibli_core::plugin::{Category, Plugin};
use bibli_core::{App, Plan};
use parking_lot::Mutex;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt as _};
use serde_json::{Value, json};

pub const ID: &str = "control";

/// The `_meta` key a call says what to do with a plan by: `propose` (an
/// agent's, the default), `print` or `apply` (`bibli cmd`, the user's own).
pub const PLAN_META: &str = "bibli/plan";

/// How long a tool waits for the user to review its plan before saying
/// it still waits: the plan stays in the window.
const PATIENCE: Duration = Duration::from_secs(10 * 60);

/// A command's tool: its dots written `__`, which MCP clients accept.
pub fn tool_name(command: &str) -> String {
    command.replace('.', "__")
}

/// The command a tool runs.
pub fn command_name(tool: &str) -> String {
    tool.replace("__", ".")
}

/// What a call does with a plan its command gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanMode {
    /// For review in the window; the answer waits for the user.
    Propose,
    /// Described, never applied: what each step would change.
    Print,
    /// Applied whole, as the user's own action.
    Apply,
}

impl PlanMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PlanMode::Propose => "propose",
            PlanMode::Print => "print",
            PlanMode::Apply => "apply",
        }
    }

    fn of(meta: Option<&Value>) -> PlanMode {
        match meta.and_then(Value::as_str) {
            Some("print") => PlanMode::Print,
            Some("apply") => PlanMode::Apply,
            _ => PlanMode::Propose,
        }
    }
}

/// The plugin: off until the user turns it on (ADR-043: until the
/// permission layer keeps folders from an agent).
#[derive(Default)]
pub struct Control {
    running: Mutex<Option<Running>>,
}

/// The thread that listens, and what ends it.
struct Running {
    stop: tokio::sync::watch::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

/// What the window's channel is doing: listening, or why not.
#[derive(Default)]
struct Status {
    listening: bool,
    /// Why it does not listen: another bibli does, or the system refused.
    not: Option<String>,
}

type Shared = Arc<Mutex<Status>>;

impl Plugin for Control {
    fn id(&self) -> &'static str {
        ID
    }
    fn title(&self) -> &'static str {
        "Agents and scripts"
    }
    fn description(&self) -> &'static str {
        "Let AI agents (over MCP: bibli mcp) and scripts (bibli cmd) drive this window. What they would change is proposed for your review. Any program running as you can then use it."
    }
    fn category(&self) -> Category {
        Category::Desktop
    }
    fn enabled_by_default(&self) -> bool {
        false
    }
    fn register(&self, app: &App) {
        let address = channel::address();
        let shared = Shared::default();
        let status = (address.clone(), Arc::clone(&shared));
        app.commands.register::<NoArgs, _>(
            ID,
            "control.status",
            "Agents and scripts",
            "Whether this window listens for agents and scripts, where, and how an agent reaches it",
            move |_, _| {
                let (address, shared) = &status;
                let status = shared.lock();
                Ok(Outcome::Value(json!({
                    "listening": status.listening,
                    "not_listening": status.not,
                    "address": address.as_ref().map(|a| a.0.display().to_string()),
                    "agent": "an MCP server started as: bibli mcp",
                    "script": "bibli cmd [--apply] <command> [<arguments, a JSON object>]",
                })))
            },
        );
        let Some(address) = address else {
            tracing::warn!("no channel for agents and scripts on this system");
            return;
        };
        if let Some(running) = start(app.weak(), address, shared) {
            *self.running.lock() = Some(running);
        }
    }
    fn on_disable(&self, _app: &App) {
        if let Some(running) = self.running.lock().take() {
            stop(running);
        }
    }
}

/// Listen at `address` for `app`, on a thread with a runtime of its own,
/// until the returned handle is stopped.
fn start(app: Weak<App>, address: channel::Address, shared: Shared) -> Option<Running> {
    let (stop, stopped) = tokio::sync::watch::channel(());
    let thread = std::thread::Builder::new()
        .name("bibli-control".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    tracing::warn!("agents and scripts cannot start: {error}");
                    return;
                }
            };
            runtime.block_on(listen(app, address, stopped, &shared));
            shared.lock().listening = false;
        })
        .ok()?;
    Some(Running { stop, thread })
}

fn stop(running: Running) {
    drop(running.stop);
    let _ = running.thread.join();
}

/// A server listening at an address of the caller's, until dropped: for
/// tests, which never use the user's own channel.
pub struct Server(Option<Running>);

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(running) = self.0.take() {
            stop(running);
        }
    }
}

/// Listen at `address` for `app` until the returned server is dropped.
pub fn serve(app: &Arc<App>, address: channel::Address) -> Server {
    Server(start(Arc::downgrade(app), address, Shared::default()))
}

async fn listen(
    app: Weak<App>,
    address: channel::Address,
    mut stopped: tokio::sync::watch::Receiver<()>,
    shared: &Mutex<Status>,
) {
    let mut listener = match channel::listen(&address) {
        Ok(Some(listener)) => listener,
        Ok(None) => {
            tracing::info!("another bibli listens for agents and scripts");
            shared.lock().not = Some("another bibli window listens".into());
            return;
        }
        Err(error) => {
            // Turned on and refused: the user is told, not only the log.
            let error = format!("{error:#}");
            tracing::warn!("agents and scripts cannot listen: {error}");
            if let Some(app) = app.upgrade() {
                app.notify(
                    NotifyLevel::Warning,
                    format!("Agents and scripts cannot listen: {error}"),
                );
            }
            shared.lock().not = Some(error);
            return;
        }
    };
    *shared.lock() = Status {
        listening: true,
        not: None,
    };
    loop {
        tokio::select! {
            _ = stopped.changed() => break,
            accepted = listener.accept() => {
                let stream = match accepted {
                    Ok(stream) => stream,
                    Err(error) => {
                        tracing::warn!("agents and scripts: {error}");
                        continue;
                    }
                };
                let tools = Tools { app: app.clone() };
                tokio::spawn(async move {
                    if let Ok(service) = tools.serve(stream).await {
                        let _ = service.waiting().await;
                    }
                });
            }
        }
    }
}

/// The commands, as MCP tools.
struct Tools {
    app: Weak<App>,
}

impl Tools {
    /// Every command, as a tool.
    fn tools(&self) -> ListToolsResult {
        let Some(app) = self.app.upgrade() else {
            return ListToolsResult::default();
        };
        let tools = app
            .commands
            .list()
            .into_iter()
            .map(|spec| {
                let mut tool = Tool::new(
                    tool_name(&spec.name),
                    spec.description.clone(),
                    input_schema(&spec.args_schema),
                );
                tool.title = Some(spec.title.clone());
                tool
            })
            .collect();
        ListToolsResult::with_all_items(tools)
    }
}

impl ServerHandler for Tools {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info = Implementation::new("bibli", env!("CARGO_PKG_VERSION"));
        info.instructions = Some(
            "bibli, a file manager. Each tool is one of its commands, its dots written as two underscores. Name the files a command acts on in its `entries`, by absolute path: the selection is the user's, and can change between two calls (the `entries` tool lists what a pane shows). A command that would change files proposes a plan the user reviews in bibli's window; its answer says whether the user applied it, and how, or dismissed it."
                .into(),
        );
        info
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        std::future::ready(Ok(self.tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let Some(app) = self.app.upgrade() else {
            return Ok(failed("bibli is closing").into());
        };
        let meta = request
            .meta
            .as_ref()
            .and_then(|meta| meta.0.0.get(PLAN_META))
            .or_else(|| context.meta.0.0.get(PLAN_META))
            .cloned();
        let mode = PlanMode::of(meta.as_ref());
        let invocation = Invocation {
            name: command_name(&request.name).into(),
            args: request.arguments.map_or_else(|| json!({}), Value::Object),
        };
        let cancelled = context.ct.clone();
        let answer = tokio::task::spawn_blocking(move || {
            run(&app, &invocation, mode, &|| cancelled.is_cancelled())
        })
        .await;
        Ok(match answer {
            Ok(Ok(answer)) => answered(&answer),
            Ok(Err(error)) => failed(&format!("{error:#}")),
            Err(error) => failed(&error.to_string()),
        }
        .into())
    }
}

/// A command's argument schema as MCP wants a tool's: an object.
fn input_schema(schema: &rmcp::schemars::Schema) -> serde_json::Map<String, Value> {
    let mut object = match serde_json::to_value(schema) {
        Ok(Value::Object(object)) => object,
        _ => serde_json::Map::new(),
    };
    object
        .entry("type")
        .or_insert_with(|| Value::String("object".into()));
    object
}

/// What a call answers: the outcome as JSON, and whether it failed.
#[derive(Debug, PartialEq)]
pub struct Answer {
    pub value: Value,
    pub failed: bool,
}

fn answered(answer: &Answer) -> CallToolResult {
    let text = serde_json::to_string_pretty(&answer.value).unwrap_or_default();
    if answer.failed {
        CallToolResult::error(vec![ContentBlock::text(text)])
    } else {
        CallToolResult::success(vec![ContentBlock::text(text)])
    }
}

fn failed(message: &str) -> CallToolResult {
    answered(&Answer {
        value: json!({ "error": message }),
        failed: true,
    })
}

/// Run `invocation` in `app`, and say what came of it: a value, a text for
/// the clipboard, or what became of its plan, as `mode` says. Blocks while
/// the user reviews a plan, until `cancelled` says to stop waiting.
/// Shared by the window's tools and `bibli cmd` without a window.
pub fn run(
    app: &App,
    invocation: &Invocation,
    mode: PlanMode,
    cancelled: &dyn Fn() -> bool,
) -> anyhow::Result<Answer> {
    app.commands.check(invocation)?;
    let outcome = match app.dispatch(invocation) {
        // The selection is the window's: a script or an agent names files.
        Err(error) if bibli_core::command::is_nothing_selected(&error) => {
            anyhow::bail!("nothing is selected: name the files in `entries`, by absolute path")
        }
        outcome => outcome?,
    };
    let value = match outcome {
        Outcome::Done => json!({ "done": true }),
        Outcome::Value(value) => value,
        Outcome::Clipboard { text, said } => json!({ "text": text, "said": said }),
        Outcome::Plan(plan) => return Ok(planned(app, plan, mode, cancelled)),
    };
    Ok(Answer {
        value,
        failed: false,
    })
}

/// What became of a plan, as `mode` says.
fn planned(app: &App, plan: Plan, mode: PlanMode, cancelled: &dyn Fn() -> bool) -> Answer {
    let title = plan.title.clone();
    let verdict = match mode {
        PlanMode::Print => {
            return Answer {
                value: serde_json::to_value(plan.diff()).unwrap_or(Value::Null),
                failed: false,
            };
        }
        PlanMode::Apply => {
            let (reply, verdict) = crossbeam_channel::bounded(1);
            app.executor.watch(plan.id, reply);
            app.apply_whole(plan);
            verdict
        }
        PlanMode::Propose => app.propose_awaiting(plan),
    };
    let deadline = std::time::Instant::now() + PATIENCE;
    loop {
        match verdict.recv_timeout(Duration::from_millis(200)) {
            Ok(verdict) => {
                let failed = matches!(verdict, Verdict::Applied { error: Some(_), .. });
                let mut value = serde_json::to_value(&verdict).unwrap_or(Value::Null);
                value["plan"] = Value::String(title);
                return Answer { value, failed };
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Answer {
                    value: json!({ "verdict": "unanswered", "plan": title, "error": "bibli closed before the plan was answered" }),
                    failed: true,
                };
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout)
                if cancelled() || std::time::Instant::now() > deadline =>
            {
                return Answer {
                    value: json!({ "verdict": "waiting", "plan": title, "note": "the plan waits for the user's review in bibli" }),
                    failed: false,
                };
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
    }
}
