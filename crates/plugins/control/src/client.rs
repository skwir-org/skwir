//! The window's channel from outside: `skwir mcp`, which carries an agent's
//! standard input and output to it, and `skwir cmd`, which runs one
//! command through it (ADR-043).

use rmcp::ServiceExt as _;
use rmcp::model::{CallToolRequestParams, MetaObject, RequestMetaObject};
use serde_json::Value;

use crate::channel::{self, Address, NotListening};
use crate::{Answer, PLAN_META, PlanMode, tool_name};

fn runtime() -> anyhow::Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

/// `skwir mcp`: the agent's standard input to the window's channel and
/// the channel to its standard output. The window ending ends it at once;
/// the agent's input ending first, the window's last answers still reach
/// its output before it ends. `NotListening` when no window listens.
pub fn bridge(address: &Address) -> anyhow::Result<()> {
    let runtime = runtime()?;
    let bridged = runtime.block_on(async {
        let stream = channel::connect(address).await?;
        let (mut from_window, mut to_window) = tokio::io::split(stream);
        let (mut stdin, mut stdout) = (tokio::io::stdin(), tokio::io::stdout());
        let up = async {
            tokio::io::copy(&mut stdin, &mut to_window).await?;
            tokio::io::AsyncWriteExt::shutdown(&mut to_window).await
        };
        let down = tokio::io::copy(&mut from_window, &mut stdout);
        tokio::pin!(up, down);
        tokio::select! {
            result = &mut up => {
                result?;
                down.await?;
            }
            result = &mut down => {
                result?;
            }
        }
        anyhow::Ok(())
    });
    // Standard input is read by a blocking task nothing can cancel: the
    // runtime is not waited on, or the window gone would leave the agent
    // waiting until it wrote another line.
    runtime.shutdown_background();
    bridged
}

/// `skwir cmd`: run `command` with `arguments` in the window, a plan as
/// `mode` says. `Err(NotListening)` when no window listens.
pub fn call(
    address: &Address,
    command: &str,
    arguments: serde_json::Map<String, Value>,
    mode: PlanMode,
) -> anyhow::Result<Answer> {
    let endpoint = match mode {
        PlanMode::Propose => address.clone(),
        PlanMode::Print | PlanMode::Apply => address.scripts(),
    };
    runtime()?.block_on(async {
        let stream = match channel::connect(&endpoint).await {
            Ok(stream) => stream,
            Err(error) => {
                // An older window has no script endpoint. Do not silently
                // execute in a separate app with a different undo history.
                anyhow::ensure!(
                    mode == PlanMode::Propose || channel::connect(address).await.is_err(),
                    "the window has no script endpoint: restart skwir before running this command"
                );
                return Err(error.into());
            }
        };
        let client = ().serve(stream).await?;
        let mut meta = serde_json::Map::new();
        meta.insert(PLAN_META.into(), Value::String(mode.as_str().into()));
        let mut request = CallToolRequestParams::new(tool_name(command)).with_arguments(arguments);
        request.meta = Some(RequestMetaObject(MetaObject(meta)));
        let result = client.call_tool(request).await?;
        let text: String = result
            .content
            .iter()
            .filter_map(|block| block.as_text().map(|text| text.text.clone()))
            .collect();
        let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
        let _ = client.cancel().await;
        Ok(Answer {
            value,
            failed: result.is_error.unwrap_or(false),
        })
    })
}

/// Whether `error` says no window listens.
pub fn not_listening(error: &anyhow::Error) -> bool {
    error.is::<NotListening>()
}
