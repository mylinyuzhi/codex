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

// ─── Reminder coverage (bare-engine layer) ───────────────────────────

#[tokio::test]
async fn test_cli_reminder_plan_mode_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::reminders::plan_mode::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_reminder_auto_mode_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::reminders::auto_mode::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_reminder_critical_instruction_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::reminders::critical_instruction::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_reminder_token_usage_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::reminders::token_usage::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_reminder_budget_usd_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::reminders::budget_usd::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_reminder_ultrathink_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::reminders::ultrathink_effort::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_reminder_output_style_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::reminders::output_style::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_reminder_skill_listing_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::reminders::skill_listing::run("deepseek-openai", MODEL).await
}

// ─── Round B: failure-mode coverage ──────────────────────────────────

#[tokio::test]
async fn test_cli_max_turns_one_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    cli::suite::max_turns_one::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_tool_error_recovery_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    cli::suite::tool_error_recovery::run("deepseek-openai", MODEL).await
}

// ─── Round D: Tier-1/2 architectural seam coverage ───────────────────

#[tokio::test]
async fn test_cli_usage_consistency_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::usage_consistency::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_bash_oversize_truncation_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    cli::suite::bash_oversize_truncation::run("deepseek-openai", MODEL).await
}

// CLAUDE.md auto-discovery is exercised by the higher-level
// `coco_cli_deepseek::test_coco_cli_claude_md_discovery` which routes
// through `run_chat_with_options { cwd }` — the only parallel-safe path
// that sets the process-effective cwd. The bare-engine `QueryEngine`
// reads memory files via `std::env::current_dir()` directly
// (engine_prompt.rs:48), so there's no in-process injection point for
// per-test isolation. Keeping the coverage at the cli layer is correct.

// `tool_input_validation` was attempted here but trips a wire-shape
// bug: when `validate_input` rejects a tool call, the orphan
// `role: "tool"` message is committed to history without a matching
// preceding `assistant.tool_calls`, which OpenAI-protocol providers
// (incl. DeepSeek) reject with HTTP 400. Filed as architectural
// follow-up; revisit once the engine surfaces validation failures as
// in-band assistant text rather than synthetic tool-results.

// `hook_pretooluse_blocks` was attempted but trips the same wire-shape
// bug as `tool_input_validation`: the engine commits the synthetic
// blocked-tool result without a matching preceding `assistant.tool_calls`
// in the API payload, so OpenAI-protocol providers reject the next
// turn with HTTP 400. Tracked as the same architectural follow-up.

#[tokio::test]
async fn test_cli_hook_posttooluse_injects_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    cli::suite::hook_posttooluse_injects::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_mid_turn_injection_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    cli::suite::mid_turn_injection::run("deepseek-openai", MODEL).await
}

// ─── Round E: invariant pinning ──────────────────────────────────────
//
// `compaction_phase_summarize` was attempted: assert
// `CompactionPhase::Summarizing` fires during the existing `compact`
// scenario. Dropped because compaction firing is "best effort" with
// LLM-driven turns (the existing `compact` test only asserts recall,
// uses `eprintln!` for the compaction signal). Pinning a specific
// inner phase would require deterministic compaction triggering,
// which the harness doesn't currently expose.

#[tokio::test]
async fn test_cli_tool_use_completed_carries_name_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    cli::suite::tool_use_completed_carries_name::run("deepseek-openai", MODEL).await
}

// ─── Round C: streaming + concurrency + prompt-override ──────────────

#[tokio::test]
async fn test_cli_streaming_deltas_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::streaming_deltas::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_parallel_reads_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "tools");
    cli::suite::parallel_reads::run("deepseek-openai", MODEL).await
}

#[tokio::test]
async fn test_cli_system_prompt_override_deepseek_openai() -> Result<()> {
    let _t = require_live!("deepseek-openai", MODEL, "text");
    cli::suite::system_prompt_override::run("deepseek-openai", MODEL).await
}

// ─── Token-usage report (alphabetically last) ────────────────────────

#[tokio::test]
async fn zzz_emit_token_usage_report() -> Result<()> {
    common::usage_report::flush("cli_deepseek")?;
    Ok(())
}
