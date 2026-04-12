//! Prompt cache break detection.
//!
//! TS: services/api/promptCacheBreakDetection.ts — two-phase detection:
//!   Phase 1 (pre-call): `record_prompt_state()` snapshots system/tool hashes.
//!   Phase 2 (post-call): `check_response_for_cache_break()` compares cache tokens.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Cache state
// ---------------------------------------------------------------------------

/// Observed cache state for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheState {
    /// First call — no baseline to compare against.
    Cold,
    /// Cache read tokens are stable (within 5% of previous and above min threshold).
    Warm,
    /// Significant drop in cache read tokens detected.
    Broken,
}

// ---------------------------------------------------------------------------
// Prompt state snapshot
// ---------------------------------------------------------------------------

/// Hashed snapshot of prompt state for cache break detection.
///
/// TS: PreviousState. We store hashes rather than full content to keep memory low.
#[derive(Debug, Clone)]
struct PromptSnapshot {
    /// Hash of the system prompt (with cache_control stripped).
    system_hash: u64,
    /// Hash of tool schemas (with cache_control stripped).
    tools_hash: u64,
    /// Hash of cache_control metadata on system blocks.
    cache_control_hash: u64,
    /// Ordered tool names.
    tool_names: Vec<String>,
    /// Per-tool schema hashes for pinpointing which tool changed.
    per_tool_hashes: HashMap<String, u64>,
    /// Character count of the system prompt.
    system_char_count: i64,
    /// Model ID.
    model: String,
    /// Whether fast mode was active.
    fast_mode: bool,
    /// Call count for this source.
    call_count: i64,
    /// Previous cache read tokens (None on first call).
    prev_cache_read_tokens: Option<i64>,
    /// Whether a cache deletion was pending (expected drop).
    cache_deletion_pending: bool,
}

// ---------------------------------------------------------------------------
// Detected changes
// ---------------------------------------------------------------------------

/// Changes detected between consecutive prompt states.
#[derive(Debug, Clone, Default)]
pub struct PendingChanges {
    pub system_prompt_changed: bool,
    pub tool_schemas_changed: bool,
    pub model_changed: bool,
    pub fast_mode_changed: bool,
    pub cache_control_changed: bool,
    pub added_tool_count: i64,
    pub removed_tool_count: i64,
    pub system_char_delta: i64,
    pub added_tools: Vec<String>,
    pub removed_tools: Vec<String>,
    pub changed_tool_schemas: Vec<String>,
    pub previous_model: String,
    pub new_model: String,
}

impl PendingChanges {
    /// Build a human-readable explanation of what changed.
    pub fn explain(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if self.model_changed {
            parts.push(format!(
                "model changed ({} -> {})",
                self.previous_model, self.new_model
            ));
        }
        if self.system_prompt_changed {
            let delta = self.system_char_delta;
            let info = if delta == 0 {
                String::new()
            } else if delta > 0 {
                format!(" (+{delta} chars)")
            } else {
                format!(" ({delta} chars)")
            };
            parts.push(format!("system prompt changed{info}"));
        }
        if self.tool_schemas_changed {
            let diff = if self.added_tool_count > 0 || self.removed_tool_count > 0 {
                format!(
                    " (+{}/-{} tools)",
                    self.added_tool_count, self.removed_tool_count
                )
            } else {
                " (tool prompt/schema changed, same tool set)".to_string()
            };
            parts.push(format!("tools changed{diff}"));
        }
        if self.fast_mode_changed {
            parts.push("fast mode toggled".into());
        }
        if self.cache_control_changed && !self.system_prompt_changed {
            parts.push("cache_control changed (scope or TTL)".into());
        }
        parts
    }
}

// ---------------------------------------------------------------------------
// Cache break result
// ---------------------------------------------------------------------------

/// Result of checking a response for a cache break.
#[derive(Debug, Clone)]
pub struct CacheBreakResult {
    pub state: CacheState,
    /// Human-readable reason for the break (empty if Warm/Cold).
    pub reason: String,
    /// Changes from phase 1, if any were detected.
    pub changes: Option<PendingChanges>,
    /// Previous cache read tokens.
    pub prev_cache_read_tokens: Option<i64>,
    /// Current cache read tokens.
    pub cache_read_tokens: i64,
    /// Cache creation tokens from the response.
    pub cache_creation_tokens: i64,
}

// ---------------------------------------------------------------------------
// Detector
// ---------------------------------------------------------------------------

/// Minimum absolute token drop to trigger a cache break warning.
const MIN_CACHE_MISS_TOKENS: i64 = 2_000;

/// 5-minute TTL threshold.
const CACHE_TTL_5MIN_MS: i64 = 5 * 60 * 1000;

/// 1-hour TTL threshold.
const CACHE_TTL_1HOUR_MS: i64 = 60 * 60 * 1000;

/// Maximum number of tracked sources to prevent unbounded memory growth.
const MAX_TRACKED_SOURCES: usize = 10;

/// Tracked source prefixes. Only these query sources are monitored for cache breaks.
const TRACKED_SOURCE_PREFIXES: &[&str] = &[
    "repl_main_thread",
    "sdk",
    "agent:custom",
    "agent:default",
    "agent:builtin",
];

/// Input for recording prompt state (phase 1).
pub struct PromptStateInput {
    /// Hash of system prompt content (without cache_control).
    pub system_hash: u64,
    /// Hash of tool schemas (without cache_control).
    pub tools_hash: u64,
    /// Hash of cache_control metadata.
    pub cache_control_hash: u64,
    /// Ordered tool names.
    pub tool_names: Vec<String>,
    /// Per-tool schema hashes.
    pub per_tool_hashes: HashMap<String, u64>,
    /// Character count of system prompt.
    pub system_char_count: i64,
    /// Model ID.
    pub model: String,
    /// Query source identifier.
    pub query_source: String,
    /// Whether fast mode is active.
    pub fast_mode: bool,
}

/// Tracks prompt cache state across calls to detect cache breaks.
///
/// TS: promptCacheBreakDetection.ts — previousStateBySource map + two-phase API.
pub struct CacheBreakDetector {
    states: HashMap<String, PromptSnapshot>,
    pending_changes: HashMap<String, PendingChanges>,
}

impl CacheBreakDetector {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            pending_changes: HashMap::new(),
        }
    }

    /// Get the tracking key for a query source, or None if untracked.
    ///
    /// "compact" shares the same server-side cache as "repl_main_thread".
    fn tracking_key(&self, query_source: &str) -> Option<String> {
        if query_source == "compact" {
            return Some("repl_main_thread".to_string());
        }
        for prefix in TRACKED_SOURCE_PREFIXES {
            if query_source.starts_with(prefix) {
                return Some(query_source.to_string());
            }
        }
        None
    }

    /// Phase 1: Record prompt state before an API call.
    ///
    /// Detects what changed from the previous call and stores pending changes
    /// for phase 2 to use.
    pub fn record_prompt_state(&mut self, input: PromptStateInput) {
        let key = match self.tracking_key(&input.query_source) {
            Some(k) => k,
            None => return,
        };

        let prev = match self.states.get(&key) {
            Some(p) => p,
            None => {
                // Evict oldest entries if at capacity
                while self.states.len() >= MAX_TRACKED_SOURCES {
                    if let Some(oldest) = self.states.keys().next().cloned() {
                        self.states.remove(&oldest);
                        self.pending_changes.remove(&oldest);
                    }
                }

                self.states.insert(
                    key,
                    PromptSnapshot {
                        system_hash: input.system_hash,
                        tools_hash: input.tools_hash,
                        cache_control_hash: input.cache_control_hash,
                        tool_names: input.tool_names,
                        per_tool_hashes: input.per_tool_hashes,
                        system_char_count: input.system_char_count,
                        model: input.model,
                        fast_mode: input.fast_mode,
                        call_count: 1,
                        prev_cache_read_tokens: None,
                        cache_deletion_pending: false,
                    },
                );
                return;
            }
        };

        let system_prompt_changed = input.system_hash != prev.system_hash;
        let tool_schemas_changed = input.tools_hash != prev.tools_hash;
        let model_changed = input.model != prev.model;
        let fast_mode_changed = input.fast_mode != prev.fast_mode;
        let cache_control_changed = input.cache_control_hash != prev.cache_control_hash;

        let anything_changed = system_prompt_changed
            || tool_schemas_changed
            || model_changed
            || fast_mode_changed
            || cache_control_changed;

        if anything_changed {
            let prev_tool_set: std::collections::HashSet<&str> =
                prev.tool_names.iter().map(String::as_str).collect();
            let new_tool_set: std::collections::HashSet<&str> =
                input.tool_names.iter().map(String::as_str).collect();
            let added: Vec<String> = input
                .tool_names
                .iter()
                .filter(|n| !prev_tool_set.contains(n.as_str()))
                .cloned()
                .collect();
            let removed: Vec<String> = prev
                .tool_names
                .iter()
                .filter(|n| !new_tool_set.contains(n.as_str()))
                .cloned()
                .collect();
            let changed_schemas: Vec<String> = if tool_schemas_changed {
                input
                    .tool_names
                    .iter()
                    .filter(|n| {
                        prev_tool_set.contains(n.as_str())
                            && input.per_tool_hashes.get(n.as_str())
                                != prev.per_tool_hashes.get(n.as_str())
                    })
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };

            self.pending_changes.insert(
                key.clone(),
                PendingChanges {
                    system_prompt_changed,
                    tool_schemas_changed,
                    model_changed,
                    fast_mode_changed,
                    cache_control_changed,
                    added_tool_count: added.len() as i64,
                    removed_tool_count: removed.len() as i64,
                    system_char_delta: input.system_char_count - prev.system_char_count,
                    added_tools: added,
                    removed_tools: removed,
                    changed_tool_schemas: changed_schemas,
                    previous_model: prev.model.clone(),
                    new_model: input.model.clone(),
                },
            );
        } else {
            self.pending_changes.remove(&key);
        }

        // Update the snapshot in place
        let snapshot = self.states.get_mut(&key).expect("state must exist");
        snapshot.system_hash = input.system_hash;
        snapshot.tools_hash = input.tools_hash;
        snapshot.cache_control_hash = input.cache_control_hash;
        snapshot.tool_names = input.tool_names;
        snapshot.per_tool_hashes = input.per_tool_hashes;
        snapshot.system_char_count = input.system_char_count;
        snapshot.model = input.model;
        snapshot.fast_mode = input.fast_mode;
        snapshot.call_count += 1;
    }

    /// Phase 2: Check the API response for a cache break.
    ///
    /// Compares current cache read tokens against the previous value to detect
    /// significant drops (>5% and >2000 tokens).
    pub fn check_response_for_cache_break(
        &mut self,
        query_source: &str,
        cache_read_tokens: i64,
        cache_creation_tokens: i64,
        time_since_last_assistant_ms: Option<i64>,
    ) -> CacheBreakResult {
        let key = match self.tracking_key(query_source) {
            Some(k) => k,
            None => {
                return CacheBreakResult {
                    state: CacheState::Cold,
                    reason: "untracked source".into(),
                    changes: None,
                    prev_cache_read_tokens: None,
                    cache_read_tokens,
                    cache_creation_tokens,
                };
            }
        };

        let snapshot = match self.states.get_mut(&key) {
            Some(s) => s,
            None => {
                return CacheBreakResult {
                    state: CacheState::Cold,
                    reason: "no prior state".into(),
                    changes: None,
                    prev_cache_read_tokens: None,
                    cache_read_tokens,
                    cache_creation_tokens,
                };
            }
        };

        let prev_cache_read = snapshot.prev_cache_read_tokens;
        snapshot.prev_cache_read_tokens = Some(cache_read_tokens);

        // Handle cache deletion (expected drop)
        if snapshot.cache_deletion_pending {
            snapshot.cache_deletion_pending = false;
            self.pending_changes.remove(&key);
            return CacheBreakResult {
                state: CacheState::Warm,
                reason: "cache deletion applied (expected drop)".into(),
                changes: None,
                prev_cache_read_tokens: prev_cache_read,
                cache_read_tokens,
                cache_creation_tokens,
            };
        }

        // First call — no previous value to compare
        let prev = match prev_cache_read {
            Some(p) => p,
            None => {
                self.pending_changes.remove(&key);
                return CacheBreakResult {
                    state: CacheState::Cold,
                    reason: "first call".into(),
                    changes: None,
                    prev_cache_read_tokens: None,
                    cache_read_tokens,
                    cache_creation_tokens,
                };
            }
        };

        // Check for cache break: >5% drop AND absolute drop > threshold
        let token_drop = prev - cache_read_tokens;
        if cache_read_tokens >= (prev as f64 * 0.95) as i64 || token_drop < MIN_CACHE_MISS_TOKENS {
            let changes = self.pending_changes.remove(&key);
            return CacheBreakResult {
                state: CacheState::Warm,
                reason: String::new(),
                changes,
                prev_cache_read_tokens: Some(prev),
                cache_read_tokens,
                cache_creation_tokens,
            };
        }

        // Cache break detected — build explanation
        let changes = self.pending_changes.remove(&key);
        let reason = match &changes {
            Some(c) => {
                let parts = c.explain();
                if parts.is_empty() {
                    build_ttl_reason(time_since_last_assistant_ms)
                } else {
                    parts.join(", ")
                }
            }
            None => build_ttl_reason(time_since_last_assistant_ms),
        };

        CacheBreakResult {
            state: CacheState::Broken,
            reason,
            changes,
            prev_cache_read_tokens: Some(prev),
            cache_read_tokens,
            cache_creation_tokens,
        }
    }

    /// Notify that a cache deletion (e.g. cached microcompact) occurred.
    /// The next response will legitimately have lower cache read tokens.
    pub fn notify_cache_deletion(&mut self, query_source: &str) {
        if let Some(key) = self.tracking_key(query_source) {
            if let Some(snapshot) = self.states.get_mut(&key) {
                snapshot.cache_deletion_pending = true;
            }
        }
    }

    /// Notify that compaction occurred. Reset the baseline so the expected
    /// drop in cache tokens doesn't trigger a false positive.
    pub fn notify_compaction(&mut self, query_source: &str) {
        if let Some(key) = self.tracking_key(query_source) {
            if let Some(snapshot) = self.states.get_mut(&key) {
                snapshot.prev_cache_read_tokens = None;
            }
        }
    }

    /// Clean up tracking state for a specific agent.
    pub fn cleanup_agent(&mut self, agent_id: &str) {
        self.states.remove(agent_id);
        self.pending_changes.remove(agent_id);
    }

    /// Reset all tracking state.
    pub fn reset(&mut self) {
        self.states.clear();
        self.pending_changes.clear();
    }
}

impl Default for CacheBreakDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a TTL-based reason when no client-side changes explain the break.
fn build_ttl_reason(time_since_last_assistant_ms: Option<i64>) -> String {
    match time_since_last_assistant_ms {
        Some(t) if t > CACHE_TTL_1HOUR_MS => "possible 1h TTL expiry (prompt unchanged)".into(),
        Some(t) if t > CACHE_TTL_5MIN_MS => "possible 5min TTL expiry (prompt unchanged)".into(),
        Some(_) => "likely server-side (prompt unchanged, <5min gap)".into(),
        None => "unknown cause".into(),
    }
}

#[cfg(test)]
#[path = "cache_detection.test.rs"]
mod tests;
