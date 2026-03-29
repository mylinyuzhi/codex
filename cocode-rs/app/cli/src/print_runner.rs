//! Headless print mode: single-turn execution with format-aware output.
//!
//! Supports three output formats via `--output-format`:
//! - `text` (default): prints the response text to stdout
//! - `json`: prints a single JSON object with result and token counts
//! - `stream-json`: streams NDJSON events during execution

use std::sync::Arc;

use cocode_config::ConfigManager;
use cocode_config::ConfigOverrides;
use cocode_session::Session;

use crate::CliFlags;
use crate::commands::chat::apply_cli_flags_to_state;

/// Output format parsed from `--output-format` flag.
#[derive(Debug, Clone, Copy, Default)]
enum OutputFormat {
    #[default]
    Text,
    Json,
    StreamJson,
}

impl OutputFormat {
    fn parse(s: &str) -> Self {
        match s {
            "json" => Self::Json,
            "stream-json" | "streaming-json" => Self::StreamJson,
            _ => Self::Text,
        }
    }
}

/// Run a single turn in print mode and exit.
pub async fn run(
    prompt: String,
    max_turns: i32,
    config: &ConfigManager,
    flags: CliFlags,
) -> anyhow::Result<()> {
    let format = flags
        .output_format
        .as_deref()
        .map(OutputFormat::parse)
        .unwrap_or_default();

    // Create session (same pattern as commands/chat.rs)
    let cwd = std::env::current_dir()?;
    let snapshot = Arc::new(config.build_config(ConfigOverrides::default().with_cwd(cwd.clone()))?);
    let mut selections = config.build_all_selections();
    flags.apply_model_overrides(config, &mut selections)?;

    let mut session = Session::with_selections(cwd, selections);
    session.set_max_turns(Some(max_turns));
    let mut state = cocode_session::SessionState::new(session, snapshot).await?;
    apply_cli_flags_to_state(&mut state, &flags).await;

    match format {
        OutputFormat::Text => {
            let result = state.run_turn(&prompt).await?;
            print!("{}", result.final_text);
        }
        OutputFormat::Json => {
            let result = state.run_turn(&prompt).await?;
            let json = serde_json::json!({
                "result": result.final_text,
                "input_tokens": result.usage.input_tokens,
                "output_tokens": result.usage.output_tokens,
            });
            println!("{}", serde_json::to_string(&json)?);
        }
        OutputFormat::StreamJson => {
            // Stream events as NDJSON. Since CoreEvent is not Serialize,
            // we use Debug formatting wrapped in a simple JSON envelope.
            let (tx, mut rx) = tokio::sync::mpsc::channel(32);

            // Spawn a task to drain and print events as they arrive
            let printer = tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    let line = serde_json::json!({
                        "type": "event",
                        "data": format!("{event:?}"),
                    });
                    println!("{line}");
                }
            });

            let result = state
                .run_turn_streaming(&prompt, tx)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let _ = printer.await;

            // Emit final result as last NDJSON line
            let final_json = serde_json::json!({
                "type": "result",
                "result": result.final_text,
                "input_tokens": result.usage.input_tokens,
                "output_tokens": result.usage.output_tokens,
            });
            println!("{final_json}");
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "print_runner.test.rs"]
mod tests;
