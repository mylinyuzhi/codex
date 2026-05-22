//! Auto memory configuration resolution.
//!
//! Resolves whether auto memory is enabled and what directory to use,
//! following a 5-level priority chain aligned with Claude Code.

use std::path::PathBuf;

use cocode_protocol::AutoMemoryConfig;

use crate::directory;

// Environment variable names — config-only vars defined here,
// shared vars imported from directory.rs (canonical definitions).
const ENV_DISABLE_AUTO_MEMORY: &str = "COCODE_DISABLE_AUTO_MEMORY";
const ENV_DISABLE_AUTO_MEMORY_COMPAT: &str = "CLAUDE_CODE_DISABLE_AUTO_MEMORY";
const ENV_REMOTE: &str = "COCODE_REMOTE";
const ENV_REMOTE_COMPAT: &str = "CLAUDE_CODE_REMOTE";
use crate::directory::ENV_COWORK_MEMORY_PATH_OVERRIDE;
use crate::directory::ENV_REMOTE_MEMORY_DIR;
use crate::directory::ENV_REMOTE_MEMORY_DIR_COMPAT;

/// Reason why auto memory is disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisableReason {
    /// Disabled via `COCODE_DISABLE_AUTO_MEMORY` environment variable.
    EnvVar,
    /// Disabled via user setting (`autoMemory.enabled = false`).
    UserSetting,
    /// Remote mode without `COCODE_REMOTE_MEMORY_DIR` set.
    RemoteNoDir,
    /// Feature flag `auto_memory` is not enabled.
    FeatureDisabled,
}

/// Resolved auto memory configuration.
#[derive(Debug, Clone)]
pub struct ResolvedAutoMemoryConfig {
    /// Whether auto memory is enabled.
    pub enabled: bool,
    /// Resolved memory directory path.
    pub directory: PathBuf,
    /// Maximum lines for MEMORY.md.
    pub max_lines: i32,
    /// Maximum relevant memory files per turn.
    pub max_relevant_files: i32,
    /// Maximum lines per relevant memory file.
    pub max_lines_per_file: i32,
    /// Timeout for relevant memories search (ms).
    pub relevant_search_timeout_ms: i64,
    /// Whether the `RelevantMemories` feature flag is enabled.
    /// Gated independently from `enabled` — both must be true for
    /// the relevant memories generator to run.
    pub relevant_memories_enabled: bool,
    /// Whether the `MemoryExtraction` feature flag is enabled.
    /// When true, the main agent gets a read-only prompt and a
    /// background extraction agent handles memory saves.
    pub memory_extraction_enabled: bool,
    /// Maximum lines to scan for YAML frontmatter in memory files.
    pub max_frontmatter_lines: i32,
    /// Days after which a memory triggers a staleness warning.
    pub staleness_warning_days: i32,
    /// Minimum turns between relevant memories generation.
    pub relevant_memories_throttle_turns: i32,
    /// Maximum memory files to scan when searching for relevant memories.
    pub max_files_to_scan: i32,
    /// Minimum word length for keyword relevance matching.
    pub min_keyword_length: i32,
    /// Reason for being disabled (if not enabled).
    pub disable_reason: Option<DisableReason>,
    /// Whether team memory is enabled.
    pub team_memory_enabled: bool,
    /// Team memory directory path ({auto_memory_dir}/team/).
    pub team_memory_directory: PathBuf,
    /// Maximum characters for MEMORY.md (soft limit, TUI warning only).
    pub max_memory_chars: i64,
    /// Whether cowork mode is active (disables write bypass).
    pub is_cowork_mode: bool,
    /// Model role for LLM-based memory selection.
    pub memory_selection_model_role: cocode_protocol::ModelRole,
}

/// Resolve auto memory configuration.
///
/// Priority chain (highest priority first):
/// 1. `COCODE_DISABLE_AUTO_MEMORY=1` env var → disable
/// 2. `COCODE_DISABLE_AUTO_MEMORY=0` env var → enable
/// 3. Remote mode without `COCODE_REMOTE_MEMORY_DIR` → disable
/// 4. `AutoMemoryConfig.enabled` user setting → use setting
/// 5. `Feature::AutoMemory` flag → use flag
///
/// Directory resolution (highest priority first):
/// 1. `COCODE_COWORK_MEMORY_PATH_OVERRIDE` env var
/// 2. `COCODE_AUTO_MEMORY_DIR` env var
/// 3. `AutoMemoryConfig.directory` user setting
/// 4. Default: `{home}/.cocode/projects/{hash}/memory/`
pub fn resolve_auto_memory_config(
    cwd: &std::path::Path,
    json_config: &AutoMemoryConfig,
    feature_enabled: bool,
    relevant_memories_enabled: bool,
    memory_extraction_enabled: bool,
    team_memory_enabled: bool,
) -> ResolvedAutoMemoryConfig {
    let (enabled, disable_reason) = resolve_enabled(json_config, feature_enabled);
    let dir = directory::get_auto_memory_directory(cwd, json_config.directory.as_deref());

    // Sub-feature flags are only active when auto memory itself is enabled.
    let relevant_memories_enabled = enabled && relevant_memories_enabled;
    let memory_extraction_enabled = enabled && memory_extraction_enabled;
    let team_memory_enabled = enabled && team_memory_enabled;

    let team_memory_directory = directory::get_team_memory_directory(&dir);
    let is_cowork_mode = std::env::var(ENV_COWORK_MEMORY_PATH_OVERRIDE)
        .ok()
        .is_some_and(|v| !v.is_empty());

    ResolvedAutoMemoryConfig {
        enabled,
        directory: dir,
        max_lines: json_config.max_lines,
        max_relevant_files: json_config.max_relevant_files,
        max_lines_per_file: json_config.max_lines_per_file,
        relevant_search_timeout_ms: json_config.relevant_search_timeout_ms,
        relevant_memories_enabled,
        memory_extraction_enabled,
        max_frontmatter_lines: json_config.max_frontmatter_lines,
        staleness_warning_days: json_config.staleness_warning_days,
        relevant_memories_throttle_turns: json_config.relevant_memories_throttle_turns,
        max_files_to_scan: json_config.max_files_to_scan,
        min_keyword_length: json_config.min_keyword_length,
        disable_reason,
        team_memory_enabled,
        team_memory_directory,
        max_memory_chars: json_config.max_memory_chars,
        is_cowork_mode,
        memory_selection_model_role: json_config
            .memory_selection_model_role
            .parse()
            .unwrap_or(cocode_protocol::ModelRole::Fast),
    }
}

/// Resolve whether auto memory is enabled.
fn resolve_enabled(
    json_config: &AutoMemoryConfig,
    feature_enabled: bool,
) -> (bool, Option<DisableReason>) {
    // Priority 1: Explicit disable via env var (truthy: "1", "true", "yes")
    if let Ok(val) = std::env::var(ENV_DISABLE_AUTO_MEMORY) {
        if is_truthy(&val) {
            return (false, Some(DisableReason::EnvVar));
        }
        // Priority 2: Explicit enable via env var (falsy non-empty: "0", "false", "no")
        if is_falsy(&val) {
            return (true, None);
        }
    }

    // Also check Claude Code compat prefix
    if let Ok(val) = std::env::var(ENV_DISABLE_AUTO_MEMORY_COMPAT) {
        if is_truthy(&val) {
            return (false, Some(DisableReason::EnvVar));
        }
        if is_falsy(&val) {
            return (true, None);
        }
    }

    // Priority 3: Remote mode without memory dir → disable
    let is_remote = std::env::var(ENV_REMOTE)
        .or_else(|_| std::env::var(ENV_REMOTE_COMPAT))
        .is_ok_and(|v| is_truthy(&v));
    let has_remote_dir = std::env::var(ENV_REMOTE_MEMORY_DIR)
        .or_else(|_| std::env::var(ENV_REMOTE_MEMORY_DIR_COMPAT))
        .is_ok();
    if is_remote && !has_remote_dir {
        return (false, Some(DisableReason::RemoteNoDir));
    }

    // Priority 4: User setting from config
    if let Some(enabled) = json_config.enabled {
        if !enabled {
            return (false, Some(DisableReason::UserSetting));
        }
        return (true, None);
    }

    // Priority 5: Feature flag
    if feature_enabled {
        (true, None)
    } else {
        (false, Some(DisableReason::FeatureDisabled))
    }
}

fn is_truthy(val: &str) -> bool {
    matches!(val.to_lowercase().as_str(), "1" | "true" | "yes")
}

fn is_falsy(val: &str) -> bool {
    matches!(val.to_lowercase().as_str(), "0" | "false" | "no")
}

#[cfg(test)]
#[path = "config.test.rs"]
mod tests;
