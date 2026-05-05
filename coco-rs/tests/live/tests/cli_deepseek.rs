//! In-process CLI-layer live tests for DeepSeek.
//!
//! These mirror the user-visible behavior of `coco -p "<prompt>"` but
//! drive the full agent loop in-process via `QueryEngine::run_with_events`,
//! capture the structured `CoreEvent` stream, and assert on it.
//!
//! Each test runs with `permission_mode = BypassPermissions` inside a
//! fresh tempdir so file-modifying tools execute without prompts.
//!
//! # Skip behavior
//!
//! Auto-skips when `DEEPSEEK_API_KEY` is unset or the provider /
//! capability is excluded by `COCO_LIVE_TEST_*` gates. Returns `Ok(())`
//! on skip so unconfigured CI stays green.
//!
//! # Capabilities
//!
//! - `text` — `one_shot`
//! - `tools` — `tool_chain`
//! - `compact` — `compact`
//! - `cross_protocol` — `cross_protocol`
//!
//! # Running
//!
//! ```bash
//! cargo test -p coco-tests-live --test cli_deepseek -- --test-threads=1 --nocapture
//! cargo test -p coco-tests-live --test cli_deepseek tool_chain
//! cargo test -p coco-tests-live --test cli_deepseek compact
//! ```

mod cli;
mod common;

use anyhow::Result;

const MODEL: &str = "deepseek-v4-flash";

// ─── deepseek-openai ─────────────────────────────────────────────────

#[tokio::test]
async fn test_cli_one_shot_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::one_shot::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_tool_chain_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    cli::suite::tool_chain::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_compact_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "compact");
    cli::suite::compact::run("deepseek-openai", MODEL).await
}

// ─── deepseek-anthropic ──────────────────────────────────────────────

#[tokio::test]
async fn test_cli_one_shot_deepseek_anthropic() -> Result<()> {
    let _t = require_live!("deepseek-anthropic", MODEL, "text");
    cli::suite::one_shot::run("deepseek-anthropic", MODEL).await
}

#[tokio::test]
async fn test_cli_tool_chain_deepseek_anthropic() -> Result<()> {
    let _t = require_live!("deepseek-anthropic", MODEL, "tools");
    cli::suite::tool_chain::run("deepseek-anthropic", MODEL).await
}

#[tokio::test]
async fn test_cli_compact_deepseek_anthropic() -> Result<()> {
    let _t = require_live!("deepseek-anthropic", MODEL, "compact");
    cli::suite::compact::run("deepseek-anthropic", MODEL).await
}

// ─── Cross-protocol ──────────────────────────────────────────────────

#[tokio::test]
async fn test_cli_cross_protocol_deepseek() -> Result<()> {
    let _open = require_live!("deepseek-openai", MODEL, "cross_protocol");
    let _anth = require_live!("deepseek-anthropic", MODEL, "cross_protocol");
    cli::suite::cross_protocol::run(MODEL).await
}

// ─── Token-usage report (alphabetically last) ────────────────────────

#[tokio::test]
async fn zzz_emit_token_usage_report() -> Result<()> {
    common::usage_report::flush("cli_deepseek")?;
    Ok(())
}
