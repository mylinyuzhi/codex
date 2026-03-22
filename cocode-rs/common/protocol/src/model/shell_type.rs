//! Shell execution type configuration.

use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::EnumIter;

/// Shell execution capability for a model.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Display, EnumIter,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ConfigShellToolType {
    /// Shell command mode — single string command, executed via `bash -c`.
    /// Used by BashTool and most modern models (GPT-5.x, codex series).
    #[default]
    ShellCommand,

    /// Basic shell mode — array-based command format (e.g. `["bash", "-lc", "ls"]`).
    /// Used by legacy models (o3, gpt-4.x) in codex-rs.
    Shell,

    /// Shell execution disabled — no shell tool sent to model.
    Disabled,
}

#[cfg(test)]
#[path = "shell_type.test.rs"]
mod tests;
