use serde::Deserialize;
use serde::Serialize;

/// Output mode for the Grep tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrepOutputMode {
    /// Show matching lines with context.
    Content,
    /// Show only file paths containing matches.
    #[default]
    FilesWithMatches,
    /// Show match counts per file.
    Count,
}

/// Action for the Config tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigAction {
    Get,
    Set,
    List,
    Reset,
}

/// Action for the LSP tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspAction {
    Definition,
    References,
    Diagnostics,
    Symbols,
    Hover,
}
