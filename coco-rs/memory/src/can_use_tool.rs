//! Per-fork canUseTool policies for memory-related forks.
//!
//! TS source: `services/extractMemories/extractMemories.ts::createAutoMemCanUseTool`
//! and `services/SessionMemory/sessionMemory.ts::createSessionMemCanUseTool`.
//!
//! Two production policies + one shared helper:
//!
//! - [`create_auto_mem_handle`] — used by ExtractMemories and AutoDream.
//!   Allows `Read` / `Glob` / `Grep` unconditionally, read-only `Bash`
//!   via [`coco_shell_parser::safety::is_known_safe_command`], and
//!   `Edit` / `Write` only on paths under `memory_dir`. Everything
//!   else is denied.
//! - [`create_session_mem_handle`] — used by SessionMemory (auto +
//!   manual). Allows `Edit` ONLY on the exact `memory_path`, allows
//!   `Read`, denies everything else. Tighter than auto-mem because
//!   session-memory writes should never sprawl outside the canonical
//!   session-memory file.
//!
//! ## Why path-prefix matters
//!
//! Both policies enforce a write fence so a misbehaving model can't
//! exfiltrate data into arbitrary locations. The fence is checked at
//! tool-execution time (step 3.5), so it composes with the
//! `allowed_write_roots` field on `ToolUseContext` — the callback's
//! check is the inner ring; the field is the outer ring.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use coco_tool_runtime::{
    CanUseToolCallContext, CanUseToolDecision, CanUseToolHandle, CanUseToolHandleRef,
    DecisionReason,
};
use serde_json::Value;

/// Tool name constants used by the policies. Matches the canonical
/// `ToolName` strings via [`coco_types::ToolName::as_str`].
const TOOL_READ: &str = "Read";
const TOOL_GLOB: &str = "Glob";
const TOOL_GREP: &str = "Grep";
const TOOL_BASH: &str = "Bash";
const TOOL_EDIT: &str = "Edit";
const TOOL_WRITE: &str = "Write";

/// Build the auto-mem canUseTool handle.
///
/// TS: `services/extractMemories/extractMemories.ts::createAutoMemCanUseTool`.
///
/// Policy:
/// - `Read` / `Glob` / `Grep` ⇒ Allow unconditionally.
/// - `Bash` ⇒ Allow when [`coco_shell_parser::safety::is_known_safe_command`]
///   returns `true`; else Deny.
/// - `Edit` / `Write` ⇒ Allow when `input.file_path` resolves under
///   `memory_dir`; else Deny.
/// - Everything else ⇒ Deny.
pub fn create_auto_mem_handle(memory_dir: PathBuf) -> CanUseToolHandleRef {
    Arc::new(AutoMemHandle { memory_dir })
}

#[derive(Debug)]
struct AutoMemHandle {
    memory_dir: PathBuf,
}

#[async_trait]
impl CanUseToolHandle for AutoMemHandle {
    async fn check(
        &self,
        tool_name: &str,
        input: &Value,
        _ctx: &CanUseToolCallContext,
    ) -> CanUseToolDecision {
        match tool_name {
            TOOL_READ | TOOL_GLOB | TOOL_GREP => allow(DecisionReason::Other {
                reason: format!("auto_mem: {tool_name} unrestricted"),
            }),
            TOOL_BASH => {
                if bash_is_read_only(input) {
                    allow(DecisionReason::Other {
                        reason: "auto_mem: read-only bash".into(),
                    })
                } else {
                    deny(
                        "auto_mem: bash command not in known-safe set".to_string(),
                        "auto_mem_bash_mutating",
                    )
                }
            }
            TOOL_EDIT | TOOL_WRITE => {
                if input_path_under_root(input, &self.memory_dir) {
                    allow(DecisionReason::Other {
                        reason: "auto_mem: write within memory_dir".into(),
                    })
                } else {
                    deny(
                        format!(
                            "auto_mem: {tool_name} only allowed under {}",
                            self.memory_dir.display()
                        ),
                        "auto_mem_write_outside_dir",
                    )
                }
            }
            other => deny(
                format!("auto_mem: tool '{other}' not in policy"),
                "auto_mem_unknown_tool",
            ),
        }
    }
}

/// Build the session-mem canUseTool handle.
///
/// TS: `services/SessionMemory/sessionMemory.ts::createSessionMemCanUseTool`.
///
/// Policy:
/// - `Read` ⇒ Allow.
/// - `Edit` ⇒ Allow ONLY when `input.file_path == memory_path` (exact
///   path match — session-memory writes are pinned to the canonical
///   session-memory file).
/// - Everything else ⇒ Deny.
pub fn create_session_mem_handle(memory_path: PathBuf) -> CanUseToolHandleRef {
    Arc::new(SessionMemHandle { memory_path })
}

#[derive(Debug)]
struct SessionMemHandle {
    memory_path: PathBuf,
}

#[async_trait]
impl CanUseToolHandle for SessionMemHandle {
    async fn check(
        &self,
        tool_name: &str,
        input: &Value,
        _ctx: &CanUseToolCallContext,
    ) -> CanUseToolDecision {
        match tool_name {
            TOOL_READ => allow(DecisionReason::Other {
                reason: "session_mem: Read unrestricted".into(),
            }),
            TOOL_EDIT => {
                let path = input.get("file_path").and_then(|v| v.as_str());
                if let Some(p) = path
                    && Path::new(p) == self.memory_path.as_path()
                {
                    return allow(DecisionReason::Other {
                        reason: "session_mem: Edit on canonical file".into(),
                    });
                }
                deny(
                    format!(
                        "session_mem: Edit only allowed on {} (got {:?})",
                        self.memory_path.display(),
                        path
                    ),
                    "session_mem_edit_wrong_path",
                )
            }
            other => deny(
                format!("session_mem: tool '{other}' not in policy"),
                "session_mem_unknown_tool",
            ),
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────

fn allow(reason: DecisionReason) -> CanUseToolDecision {
    CanUseToolDecision::Allow {
        updated_input: None,
        decision_reason: reason,
    }
}

fn deny(message: String, reason_label: &str) -> CanUseToolDecision {
    CanUseToolDecision::Deny {
        message,
        decision_reason: DecisionReason::Other {
            reason: reason_label.to_string(),
        },
    }
}

/// Is the Bash input's `command` known-safe (read-only)?
///
/// Uses `coco_shell_parser::ShellParser::try_extract_safe_commands`
/// to parse the command into a sequence of word-only argv stages
/// (chained with safe operators `&&` / `||` / `;` / `|`). When the
/// parse returns `None` the command has a redirection / subshell /
/// command-substitution — we fail closed.
///
/// Each stage's argv goes through
/// `coco_shell_parser::safety::is_known_safe_command`; ALL stages
/// must pass for the whole pipeline to be allowed. This means
/// `git log --oneline | head -10` (two safe stages joined by a
/// safe operator) is allowed, but `echo bad > /etc/passwd` (which
/// has a redirection) and `rm -rf /` (mutating first stage) are
/// rejected.
///
/// TS parity: `extractMemories.ts::createAutoMemCanUseTool` calls
/// `tool.isReadOnly(parsed.data)` which uses the same full shell
/// parse + per-stage safe-command lookup.
fn bash_is_read_only(input: &Value) -> bool {
    let Some(cmd) = input.get("command").and_then(|v| v.as_str()) else {
        return false;
    };
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return false;
    }
    let mut parser = coco_shell_parser::ShellParser::new();
    let parsed = parser.parse(trimmed);
    let Some(stages) = parsed.try_extract_safe_commands() else {
        // Parse contains a redirection / subshell / command-sub /
        // syntax error — fail closed.
        return false;
    };
    if stages.is_empty() {
        return false;
    }
    stages
        .iter()
        .all(|argv| coco_shell_parser::safety::is_known_safe_command(argv))
}

/// True when `input.file_path` (or `input.notebook_path`) is a
/// descendant of `root`. Path comparisons use canonical-prefix
/// matching to defeat `..` traversal tricks.
fn input_path_under_root(input: &Value, root: &Path) -> bool {
    let path_str = input
        .get("file_path")
        .or_else(|| input.get("notebook_path"))
        .or_else(|| input.get("path"))
        .and_then(|v| v.as_str());
    let Some(p) = path_str else {
        return false;
    };
    let candidate = Path::new(p);
    // Normalize both sides via dunce (or std::fs::canonicalize when
    // the path exists). For the in-memory test case, fall back to
    // lexical comparison via `starts_with` after lexical
    // normalization.
    let canonical_root = lexical_normalize(root);
    let canonical_candidate = lexical_normalize(candidate);
    canonical_candidate.starts_with(&canonical_root)
}

/// Lexically normalize a path: collapse `.` and `..` components.
/// Doesn't touch the filesystem so it works for hypothetical paths
/// the model might propose.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
#[path = "can_use_tool.test.rs"]
mod tests;
