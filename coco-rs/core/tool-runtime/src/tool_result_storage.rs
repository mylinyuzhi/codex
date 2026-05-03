//! Tool result budget — Level 1 (per-tool persistence) + Level 2
//! (per-message aggregate cap).
//!
//! TS: `utils/toolResultStorage.ts` (1040 LoC).
//!
//! **Level 1** (per-tool): each tool declares
//! [`crate::Tool::max_result_size_chars`]. When a tool result exceeds
//! the declared cap, [`persist_to_disk`] writes the body to
//! `<session_dir>/tool-results/<id>.{txt,json}` and returns a
//! [`PersistedToolResult`] reference the runtime substitutes for the
//! original content. Tools that don't override the trait method opt
//! out (default `i64::MAX`).
//!
//! **Level 2** (per-message): `apply_tool_result_budget` walks recent
//! tool results in a message and replaces older ones with a
//! placeholder when the cumulative content exceeds
//! [`crate::tool_result_storage::ContentReplacementState`]'s
//! `per_message_chars` budget. The replacement uses the canonical
//! `[Old tool result content cleared]` string TS uses, keyed by
//! `tool_use_id` so subsequent re-renders pick the same replacement
//! (prompt-cache stable).
//!
//! Both levels are **inert by default** (matching TS feature-gated
//! behaviour) — `apply_tool_result_budget` returns the input
//! unchanged when `state.per_message_chars` is `i64::MAX` (the
//! "feature off" sentinel) and `persist_to_disk` is only called by
//! callers that know their tool opted in.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;
use tokio::sync::RwLock;

/// Default per-tool persistence threshold (TS `DEFAULT_MAX_RESULT_SIZE_CHARS`).
pub const DEFAULT_MAX_RESULT_SIZE_CHARS: i64 = 50_000;

/// Default per-message aggregate cap (TS
/// `MAX_TOOL_RESULTS_PER_MESSAGE_CHARS`).
pub const DEFAULT_MAX_PER_MESSAGE_CHARS: i64 = 200_000;

/// Subdirectory name for tool results within a session (TS `TOOL_RESULTS_SUBDIR`).
pub const TOOL_RESULTS_SUBDIR: &str = "tool-results";

/// XML tag wrapping the persisted-output reference message (TS `PERSISTED_OUTPUT_TAG`).
pub const PERSISTED_OUTPUT_TAG: &str = "<persisted-output>";
pub const PERSISTED_OUTPUT_CLOSING_TAG: &str = "</persisted-output>";

/// Replacement marker for Level 2 budget eviction (TS `TOOL_RESULT_CLEARED_MESSAGE`).
pub const TOOL_RESULT_CLEARED_MESSAGE: &str = "[Old tool result content cleared]";

/// Resolved persistence threshold for one tool. Mirrors TS
/// `getPersistenceThreshold(toolName, declaredMaxResultSizeChars)`:
///
/// - `i64::MAX` declared → opt-out (returned verbatim, never persisted).
/// - Otherwise: clamps `declared` against `DEFAULT_MAX_RESULT_SIZE_CHARS`.
///
/// The TS `tengu_satin_quoll` GrowthBook per-tool override is not
/// modelled here (no equivalent flag system in coco-rs). When the
/// override system lands, an extra `overrides: &HashMap<String, i64>`
/// param can be threaded through.
pub fn resolve_persistence_threshold(declared_max_result_size_chars: i64) -> i64 {
    if declared_max_result_size_chars == i64::MAX {
        return i64::MAX;
    }
    std::cmp::min(
        declared_max_result_size_chars,
        DEFAULT_MAX_RESULT_SIZE_CHARS,
    )
}

/// Per-session tool-result directory (TS `getToolResultsDir`).
pub fn tool_results_dir(session_dir: &Path) -> PathBuf {
    session_dir.join(TOOL_RESULTS_SUBDIR)
}

/// Path where a persisted tool result lives.
pub fn tool_result_path(session_dir: &Path, id: &str, is_json: bool) -> PathBuf {
    let ext = if is_json { "json" } else { "txt" };
    tool_results_dir(session_dir).join(format!("{id}.{ext}"))
}

/// Outcome of persisting a tool result to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedToolResult {
    pub filepath: PathBuf,
    pub original_size: i64,
    pub is_json: bool,
    pub preview: String,
    pub has_more: bool,
}

/// Preview size in bytes for the reference message (TS `PREVIEW_SIZE_BYTES`).
pub const PREVIEW_SIZE_BYTES: usize = 2000;

/// Persist a tool result to disk and return a structured reference
/// the caller substitutes for the inline content. Caller decides
/// whether to invoke based on `content.len() > resolve_persistence_threshold(...)`.
///
/// `content` is the raw tool result body. `is_json` selects extension
/// (`.json` vs `.txt`) and is informational only — content is written
/// verbatim. TS parity: `persistToolResultToDisk`.
pub async fn persist_to_disk(
    session_dir: &Path,
    id: &str,
    content: &str,
    is_json: bool,
) -> std::io::Result<PersistedToolResult> {
    let dir = tool_results_dir(session_dir);
    tokio::fs::create_dir_all(&dir).await?;
    let filepath = tool_result_path(session_dir, id, is_json);
    tokio::fs::write(&filepath, content).await?;
    let preview_end = std::cmp::min(PREVIEW_SIZE_BYTES, content.len());
    let preview = content[..preview_end].to_string();
    Ok(PersistedToolResult {
        filepath,
        original_size: content.len() as i64,
        is_json,
        preview,
        has_more: content.len() > PREVIEW_SIZE_BYTES,
    })
}

/// Render the `<persisted-output>` reference message body (TS
/// `formatPersistedReference`).
pub fn render_persisted_reference(persisted: &PersistedToolResult) -> String {
    format!(
        "{open}\nfilepath: {path}\noriginal_size: {size} bytes\nhas_more: {more}\n\nPreview (first {preview_size} bytes):\n{preview}\n{close}",
        open = PERSISTED_OUTPUT_TAG,
        path = persisted.filepath.display(),
        size = persisted.original_size,
        more = persisted.has_more,
        preview_size = PREVIEW_SIZE_BYTES,
        preview = persisted.preview,
        close = PERSISTED_OUTPUT_CLOSING_TAG,
    )
}

/// Per-session content-replacement state for Level 2 budget. Tracks:
///
/// - `replacements`: tool_use_id → replacement string (always
///   `TOOL_RESULT_CLEARED_MESSAGE` today; field is keyed for prompt-
///   cache stability — same id → same replacement across re-renders).
/// - `seen_ids`: tool_use_ids the budget has already considered. Once
///   seen, a result is "frozen" — never re-replaced even if it
///   shrinks under the cap (matches TS behaviour).
/// - `per_message_chars`: budget cap. `i64::MAX` ⇒ feature off
///   (`apply_tool_result_budget` returns input unchanged).
#[derive(Debug, Default, Clone)]
pub struct ContentReplacementState {
    pub replacements: HashMap<String, String>,
    pub seen_ids: std::collections::HashSet<String>,
    pub per_message_chars: i64,
}

impl ContentReplacementState {
    pub fn new(per_message_chars: i64) -> Self {
        Self {
            per_message_chars,
            ..Default::default()
        }
    }

    pub fn is_active(&self) -> bool {
        self.per_message_chars != i64::MAX
    }
}

/// Shared handle for engine wiring.
pub type ContentReplacementStateRef = Arc<RwLock<ContentReplacementState>>;

/// One tool-result candidate for budget evaluation. Caller projects
/// from their message representation (typically `tool_result` blocks
/// inside a user message). The runtime consumes a flat list because
/// the engine's message types live in `coco-messages` (a higher
/// layer) — passing typed refs here would require depending on it.
#[derive(Debug, Clone)]
pub struct ToolResultCandidate {
    pub tool_use_id: String,
    pub content_chars: i64,
    /// Tool name when known — drives Level 1 per-tool opt-out
    /// (`is_persistence_opted_out`). `None` ⇒ apply Level 2 only.
    pub tool_name: Option<String>,
    /// Whether this candidate's tool opted out of persistence
    /// (declared `i64::MAX` for `max_result_size_chars`). When `true`,
    /// the budget pipeline skips it (TS uses the same opt-out for
    /// canonical-content tools like `Read` on a tracked file).
    pub persistence_opted_out: bool,
}

/// Outcome of running [`apply_tool_result_budget`].
#[derive(Debug, Clone, Default)]
pub struct BudgetOutcome {
    /// Tool-use IDs that got newly replaced this pass (caller
    /// substitutes the canonical placeholder for each).
    pub newly_replaced: Vec<String>,
    /// Total chars freed from the in-message aggregate.
    pub freed_chars: i64,
}

/// Decide which tool-result candidates to evict to fit the per-
/// message char budget. Mirrors TS `enforceToolResultBudget`:
///
/// 1. Compute aggregate size (sum of `content_chars` for unreplaced
///    candidates).
/// 2. If aggregate ≤ `state.per_message_chars`, return empty outcome.
/// 3. Walk candidates oldest-first, replacing each (mark in
///    `state.replacements` with `TOOL_RESULT_CLEARED_MESSAGE`) until
///    the aggregate fits OR only the most recent N (TS uses 1) are
///    left unreplaced.
/// 4. Skip candidates whose tool opted out of persistence (TS:
///    `skipToolNames` set).
/// 5. Mark every candidate as `seen` so subsequent passes don't
///    re-evaluate (frozen-once semantics, matches TS).
///
/// Caller applies `state.replacements` to the actual message content
/// (lookup by `tool_use_id`).
pub async fn apply_tool_result_budget(
    candidates: &[ToolResultCandidate],
    state: &ContentReplacementStateRef,
) -> BudgetOutcome {
    let mut state = state.write().await;
    if !state.is_active() {
        return BudgetOutcome::default();
    }

    // Mark all candidate IDs as seen — TS behaviour: once a result
    // appears in a message, the budget freezes it (replaced or not).
    for c in candidates {
        state.seen_ids.insert(c.tool_use_id.clone());
    }

    // Aggregate considers only candidates that are NOT already
    // replaced and haven't opted out (opted-out tools occupy budget
    // but the budget pipeline can't act on them — TS treats them as
    // immovable).
    let aggregate: i64 = candidates
        .iter()
        .filter(|c| !state.replacements.contains_key(&c.tool_use_id))
        .map(|c| c.content_chars)
        .sum();
    if aggregate <= state.per_message_chars {
        return BudgetOutcome::default();
    }

    // Always preserve the most recent candidate (TS keeps at least
    // one tool result intact). The rest are evictable, oldest-first.
    let evictable: Vec<&ToolResultCandidate> = candidates
        .iter()
        .take(candidates.len().saturating_sub(1))
        .filter(|c| !c.persistence_opted_out && !state.replacements.contains_key(&c.tool_use_id))
        .collect();

    let mut outcome = BudgetOutcome::default();
    let mut still_over = aggregate;
    for cand in evictable {
        if still_over <= state.per_message_chars {
            break;
        }
        state
            .replacements
            .insert(cand.tool_use_id.clone(), TOOL_RESULT_CLEARED_MESSAGE.into());
        outcome.newly_replaced.push(cand.tool_use_id.clone());
        outcome.freed_chars += cand.content_chars;
        still_over -= cand.content_chars;
    }
    outcome
}

#[cfg(test)]
#[path = "tool_result_storage.test.rs"]
mod tests;
