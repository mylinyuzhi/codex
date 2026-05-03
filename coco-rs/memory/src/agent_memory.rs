//! Per-agent persistent memory — `.../agent-memory/<agentType>/MEMORY.md`.
//!
//! TS: `tools/AgentTool/agentMemory.ts` + `agentMemorySnapshot.ts`.
//!
//! Each agent definition can declare `memory: user|project|local` in its
//! frontmatter (via [`coco_types::MemoryScope`]). When set, the agent's
//! per-type MEMORY.md gets appended to its system prompt at spawn time
//! so the child sees its own scoped, persistent memory body.
//!
//! Resolution by scope (TS `agentMemory.ts:52-65`):
//! - `User`     → `<home>/.coco/agent-memory/<sanitized_type>/`
//! - `Project`  → `<cwd>/.coco/agent-memory/<sanitized_type>/`
//! - `Local`    → `<cwd>/.coco/agent-memory-local/<sanitized_type>/`
//!
//! Sanitization: TS `sanitizeAgentTypeForPath` replaces `:` (used by
//! plugin-namespaced types like `my-plugin:my-agent`) with `-` because
//! `:` is invalid in Windows paths. Mirror that.

use std::path::Path;
use std::path::PathBuf;

use coco_types::MemoryScope;

/// Replace `:` with `-` so plugin-namespaced types map to a valid
/// directory name on every platform. TS: `sanitizeAgentTypeForPath`.
fn sanitize_agent_type_for_path(agent_type: &str) -> String {
    agent_type.replace(':', "-")
}

/// Resolve the per-agent memory directory for the given (type, scope).
///
/// `cwd` is the current project root; `home` is the user's home dir.
/// TS: `getAgentMemoryDir(agentType, scope)`.
pub fn agent_memory_dir(agent_type: &str, scope: MemoryScope, cwd: &Path, home: &Path) -> PathBuf {
    let dir_name = sanitize_agent_type_for_path(agent_type);
    match scope {
        MemoryScope::User => home.join(".coco").join("agent-memory").join(dir_name),
        MemoryScope::Project => cwd.join(".coco").join("agent-memory").join(dir_name),
        MemoryScope::Local => cwd.join(".coco").join("agent-memory-local").join(dir_name),
    }
}

/// Resolve the per-agent `MEMORY.md` entry-point file path.
///
/// TS: `getAgentMemoryEntrypoint(agentType, scope)`.
pub fn agent_memory_entrypoint(
    agent_type: &str,
    scope: MemoryScope,
    cwd: &Path,
    home: &Path,
) -> PathBuf {
    agent_memory_dir(agent_type, scope, cwd, home).join("MEMORY.md")
}

/// Per-scope guidance line appended to the agent-memory prompt block.
/// TS: `loadAgentMemoryPrompt:scope` switch.
fn scope_note(scope: MemoryScope) -> &'static str {
    match scope {
        MemoryScope::User => {
            "- Since this memory is user-scope, keep learnings general since they apply across all projects"
        }
        MemoryScope::Project => {
            "- Since this memory is project-scope and shared with your team via version control, tailor your memories to this project"
        }
        MemoryScope::Local => {
            "- Since this memory is local-scope (not checked into version control), tailor your memories to this project and machine"
        }
    }
}

/// Load the per-agent memory prompt block to append to the agent's
/// system prompt.
///
/// Returns `None` when the per-agent memory directory exists but
/// `MEMORY.md` is empty AND there are no other `.md` files — meaning
/// the agent has nothing to inject yet. The directory itself is NOT
/// created here; that's the agent's job once it starts writing.
///
/// TS: `loadAgentMemoryPrompt(agentType, scope)`.
pub fn load_agent_memory_prompt(
    agent_type: &str,
    scope: MemoryScope,
    cwd: &Path,
    home: &Path,
) -> String {
    let memory_dir = agent_memory_dir(agent_type, scope, cwd, home);
    let memory_file = memory_dir.join("MEMORY.md");

    let body = std::fs::read_to_string(&memory_file).unwrap_or_default();

    let mut sections = Vec::new();
    sections.push("# Persistent Agent Memory".to_string());
    sections.push(format!(
        "You have a persistent, file-based memory system at `{}`. This directory \
         already exists — write to it directly with the Write tool (do not run mkdir \
         or check for its existence).",
        memory_dir.display(),
    ));
    sections.push(scope_note(scope).to_string());

    if body.trim().is_empty() {
        sections.push(
            "## MEMORY.md\n\nYour MEMORY.md is currently empty. When you save new memories, they will appear here."
                .to_string(),
        );
    } else {
        sections.push("## MEMORY.md".to_string());
        sections.push(body);
    }

    sections.join("\n\n")
}

#[cfg(test)]
#[path = "agent_memory.test.rs"]
mod tests;
