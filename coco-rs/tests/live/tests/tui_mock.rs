//! In-process TUI integration tests against a scripted (no-API-key)
//! [`ScriptedModel`].
//!
//! Each `#[tokio::test]` boots a fresh `TuiHarness`, drives one
//! conversational scenario through the real TUI state machine + render
//! pipeline, and asserts on `AppState` and the rendered terminal buffer.
//! See `tui/mod.rs` for the architecture diagram and `tui/harness.rs`
//! for the in-process harness.
//!
//! No live provider is involved — these run in default `cargo test`
//! without any environment setup. They share `tests/common/` only for
//! the `/tmp` tempdir helper; the `require_live!` macro is not used.
//!
//! # Running
//!
//! ```bash
//! cargo test -p coco-tests-live --test tui_mock
//! cargo test -p coco-tests-live --test tui_mock -- one_shot
//! ```

mod common;
mod tui;

use anyhow::Result;

#[tokio::test]
async fn test_tui_boot_render() -> Result<()> {
    tui::suite::boot_render::run().await
}

#[tokio::test]
async fn test_tui_one_shot() -> Result<()> {
    tui::suite::one_shot::run().await
}

#[tokio::test]
async fn test_tui_multi_turn() -> Result<()> {
    tui::suite::multi_turn::run().await
}

#[tokio::test]
async fn test_tui_tool_chain() -> Result<()> {
    tui::suite::tool_chain::run().await
}

#[tokio::test]
async fn test_tui_hook_verify() -> Result<()> {
    tui::suite::hook_verify::run().await
}

#[tokio::test]
async fn test_tui_keyboard_dispatch() -> Result<()> {
    tui::suite::keyboard_dispatch::run().await
}
