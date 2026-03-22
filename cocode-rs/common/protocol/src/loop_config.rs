//! Configuration types for the core loop.
//!
//! These types configure the behavior of the agent's main execution loop.

use std::time::Duration;

use serde::Deserialize;
use serde::Serialize;
use strum::Display;
use strum::IntoStaticStr;

use crate::PermissionMode;

/// Configuration for the core agent loop.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoopConfig {
    /// Maximum number of turns before stopping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<i32>,
    /// Maximum tokens to use before stopping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    /// Permission mode for tool execution.
    #[serde(default)]
    pub permission_mode: PermissionMode,
    /// Enable streaming tool execution.
    #[serde(default)]
    pub enable_streaming_tools: bool,
    /// Enable micro-compaction of tool results.
    #[serde(default)]
    pub enable_micro_compaction: bool,
    /// Fallback model to use when primary model fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_model: Option<String>,
    /// Agent identifier (for sub-agents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Parent agent identifier (for sub-agents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    /// Whether to record sidechain events.
    #[serde(default)]
    pub record_sidechain: bool,
    /// Session memory configuration.
    #[serde(default)]
    pub session_memory: SessionMemoryConfig,
    /// Stall detection configuration.
    #[serde(default)]
    pub stall_detection: StallDetectionConfig,
    /// Prompt caching configuration.
    #[serde(default)]
    pub prompt_caching: PromptCachingConfig,
}

/// Configuration for session memory management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemoryConfig {
    /// Token budget for session memory.
    #[serde(default = "default_budget_tokens")]
    pub budget_tokens: i32,
    /// Priority for file restoration during session recovery.
    #[serde(default)]
    pub restoration_priority: FileRestorationPriority,
    /// Whether session memory is enabled.
    #[serde(default = "crate::default_true")]
    pub enabled: bool,
}

fn default_budget_tokens() -> i32 {
    4096
}

impl Default for SessionMemoryConfig {
    fn default() -> Self {
        Self {
            budget_tokens: default_budget_tokens(),
            restoration_priority: FileRestorationPriority::default(),
            enabled: true,
        }
    }
}

/// Priority for restoring files during session recovery.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Display, IntoStaticStr,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum FileRestorationPriority {
    /// Restore most recently accessed files first.
    #[default]
    MostRecent,
    /// Restore most frequently accessed files first.
    MostAccessed,
}

impl FileRestorationPriority {
    /// Get the priority as a string.
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }
}

/// Configuration for stream stall detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StallDetectionConfig {
    /// Timeout duration before considering a stream stalled.
    #[serde(with = "humantime_serde", default = "default_stall_timeout")]
    pub stall_timeout: Duration,
    /// Recovery action when a stall is detected.
    #[serde(default)]
    pub recovery: StallRecovery,
    /// Whether stall detection is enabled.
    #[serde(default = "crate::default_true")]
    pub enabled: bool,
    /// Two-tier watchdog configuration.
    ///
    /// Provides a warning phase before the abort phase, matching
    /// Claude Code's stream watchdog behavior.
    #[serde(default)]
    pub watchdog: WatchdogConfig,
}

fn default_stall_timeout() -> Duration {
    Duration::from_secs(30)
}

impl Default for StallDetectionConfig {
    fn default() -> Self {
        Self {
            stall_timeout: default_stall_timeout(),
            recovery: StallRecovery::default(),
            enabled: true,
            watchdog: WatchdogConfig::default(),
        }
    }
}

/// Recovery action when a stream stall is detected.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Display, IntoStaticStr,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum StallRecovery {
    /// Retry the request.
    #[default]
    Retry,
    /// Abort the operation.
    Abort,
    /// Fall back to an alternative model.
    Fallback,
}

impl StallRecovery {
    /// Get the recovery action as a string.
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }
}

/// Two-tier stream watchdog configuration.
///
/// Matches Claude Code's watchdog behavior:
/// - **Warning tier** (default 60s): Log warning and emit UI event
/// - **Abort tier** (default 180s): Kill stream, trigger fallback
///
/// Both timeouts are measured from the last received SSE event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WatchdogConfig {
    /// Enable the two-tier watchdog.
    #[serde(default = "crate::default_true")]
    pub enabled: bool,
    /// Warning timeout — emit a warning event after this gap.
    #[serde(with = "humantime_serde", default = "default_warning_timeout")]
    pub warning_timeout: Duration,
    /// Abort timeout — kill the stream after this gap.
    #[serde(with = "humantime_serde", default = "default_abort_timeout")]
    pub abort_timeout: Duration,
}

/// Claude Code uses 30s. We use 60s to tolerate slower models
/// (e.g., self-hosted or high-latency providers) that may take longer
/// between SSE events without actually being stalled.
fn default_warning_timeout() -> Duration {
    Duration::from_secs(60)
}

/// Claude Code uses 60s. We use 180s to tolerate slower models
/// that legitimately need more time per chunk (e.g., large-context
/// requests on constrained infrastructure).
fn default_abort_timeout() -> Duration {
    Duration::from_secs(180)
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            warning_timeout: default_warning_timeout(),
            abort_timeout: default_abort_timeout(),
        }
    }
}

/// Configuration for prompt caching.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptCachingConfig {
    /// Whether prompt caching is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Cache breakpoints in the conversation.
    #[serde(default)]
    pub cache_breakpoints: Vec<CacheBreakpoint>,
}

/// A breakpoint for cache insertion in the conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheBreakpoint {
    /// Position in the message list (0-indexed).
    pub position: i32,
    /// Type of cache to use.
    pub cache_type: CacheType,
}

/// Type of cache for prompt caching.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, Display, IntoStaticStr,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum CacheType {
    /// Ephemeral cache (short-lived).
    #[default]
    Ephemeral,
}

impl CacheType {
    /// Get the cache type as a string.
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }
}

#[cfg(test)]
#[path = "loop_config.test.rs"]
mod tests;
