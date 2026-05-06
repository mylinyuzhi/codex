//! TUI integration test suites. Each module is one focused scenario
//! that drives the [`TuiHarness`] through a specific code path.
//!
//! Top-level `tui_mock.rs` wires each suite to a `#[tokio::test]` entry.

pub mod boot_render;
pub mod hook_verify;
pub mod keyboard_dispatch;
pub mod multi_turn;
pub mod one_shot;
pub mod tool_chain;
