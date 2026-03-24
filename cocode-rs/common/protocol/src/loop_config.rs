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
    /// Maximum budget in cents before pausing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_budget_cents: Option<i32>,
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
    /// Prompt cache strategy configuration.
    #[serde(default)]
    pub prompt_caching: PromptCacheConfig,
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

/// Prompt cache strategy configuration.
///
/// Controls how cache breakpoints are placed in prompts sent to providers
/// that support explicit cache markers (currently Anthropic only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptCacheConfig {
    /// Whether prompt caching is enabled.
    #[serde(default = "crate::default_true")]
    pub enabled: bool,
    /// Skip cache writes (for compaction agents).
    ///
    /// When true, the breakpoint index shifts from the last message to the
    /// second-to-last, avoiding caching content that will be immediately replaced.
    /// This preserves the existing cache prefix for post-compaction turns.
    #[serde(default)]
    pub skip_cache_write: bool,
}

impl Default for PromptCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            skip_cache_write: false,
        }
    }
}

/// Cache scope for system prompt blocks.
///
/// Determines the sharing level of cached content in the Anthropic API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheScope {
    /// Shared across organization.
    Org,
    /// Shared globally across all users.
    Global,
}

#[cfg(test)]
#[path = "loop_config.test.rs"]
mod tests;
