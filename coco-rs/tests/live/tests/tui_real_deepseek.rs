//! Real-LLM TUI integration tests against DeepSeek.
//!
//! Drives the **production TUI bootstrap path** (Cli → headless config
//! build → session_bootstrap → SessionRuntime → late-binds → driver
//! loop → AppState fold → render::render) with a real provider HTTP
//! call instead of a scripted model. See `tui_real/mod.rs` for the
//! full pipeline.
//!
//! # Skip behavior
//!
//! Auto-skips when `DEEPSEEK_API_KEY` is unset or the provider /
//! capability is excluded by `COCO_LIVE_TEST_*` gates. Returns
//! `Ok(())` on skip so unconfigured CI stays green.
//!
//! # Capabilities
//!
//! - `text` — `one_shot`, `claude_md`
//! - `tools` — `tool_chain`, `hook_pretooluse`,
//!   `permission_round_trip`, `permission_reject`, `interrupt`
//!
//! # Running
//!
//! ```bash
//! DEEPSEEK_API_KEY=... cargo test -p coco-tests-live --test tui_real_deepseek -- --test-threads=1 --nocapture
//! cargo test -p coco-tests-live --test tui_real_deepseek tool_chain
//! ```

mod common;
mod tui_real;

use anyhow::Result;

const MODEL: &str = "deepseek-v4-flash";

// ─── deepseek-openai ─────────────────────────────────────────────────

#[tokio::test]
async fn test_tui_real_one_shot_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    tui_real::suite::one_shot::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_tui_real_tool_chain_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    tui_real::suite::tool_chain::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_tui_real_hook_pretooluse_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    tui_real::suite::hook_pretooluse::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_tui_real_permission_approve_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    tui_real::suite::permission_round_trip::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_tui_real_permission_reject_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    tui_real::suite::permission_reject::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_tui_real_claude_md_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    tui_real::suite::claude_md::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_tui_real_interrupt_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    tui_real::suite::interrupt::run("deepseek-openai", MODEL).await
}

// ─── Reminder coverage (full TUI prod path) ───────────────────────────

#[tokio::test]
async fn test_tui_real_reminder_at_mention_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    tui_real::suite::reminders::at_mention_file::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_tui_real_reminder_agent_mention_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    tui_real::suite::reminders::agent_mention::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_tui_real_reminder_nested_memory_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    tui_real::suite::reminders::nested_memory::run("deepseek-openai", MODEL).await
}

// ─── deepseek-anthropic (smoke set) ──────────────────────────────────

#[tokio::test]
async fn test_tui_real_one_shot_deepseek_anthropic() -> Result<()> {
    let _t = require_live!("deepseek-anthropic", MODEL, "text");
    tui_real::suite::one_shot::run("deepseek-anthropic", MODEL).await
}

#[tokio::test]
async fn test_tui_real_tool_chain_deepseek_anthropic() -> Result<()> {
    let _t = require_live!("deepseek-anthropic", MODEL, "tools");
    tui_real::suite::tool_chain::run("deepseek-anthropic", MODEL).await
}

// ─── Token-usage report (alphabetically last) ────────────────────────

#[tokio::test]
async fn zzz_emit_token_usage_report() -> Result<()> {
    common::usage_report::flush("tui_real_deepseek")?;
    Ok(())
}
