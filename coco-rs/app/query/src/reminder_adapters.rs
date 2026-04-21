//! Adapter wrappers that impl `coco_system_reminder::*Source` traits
//! for concrete managers from lower-layer crates.
//!
//! **Why wrappers live here**: the crate layering is
//! `app > root modules > core > services / standalone > common`. A
//! source trait is defined in `core/system-reminder` (core layer).
//! Services/standalone crates (`coco-lsp`, `coco-mcp`, `coco-bridge`)
//! can't depend on core crates without violating the layer order —
//! so their impls can't live in-crate. Root-module crates (`coco-hooks`,
//! `coco-tasks`, `coco-skills`, `coco-memory`) legitimately sit
//! above core and impl their traits directly in-crate.
//!
//! This module hosts the newtype adapters for the crates that can't
//! impl the trait themselves. Each adapter holds an `Arc<ConcreteManager>`
//! and delegates trait calls to plain methods on the concrete type —
//! no logic, just binding.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use coco_system_reminder::AgentPendingMessage;
use coco_system_reminder::DiagnosticFileSummary;
use coco_system_reminder::DiagnosticsSource;
use coco_system_reminder::IdeBridgeSource;
use coco_system_reminder::IdeOpenedFileSnapshot;
use coco_system_reminder::IdeSelectionSnapshot;
use coco_system_reminder::McpResourceEntry;
use coco_system_reminder::McpSource;
use coco_system_reminder::MemorySource;
use coco_system_reminder::NestedMemoryInfo;
use coco_system_reminder::RelevantMemoryInfo;
use coco_system_reminder::SwarmSource;
use coco_system_reminder::TeamContextSnapshot;
use coco_system_reminder::TeammateMailboxInfo;

// ────────────────────────────────────────────────────────────────
// LSP diagnostics adapter
// ────────────────────────────────────────────────────────────────

/// Wraps `coco_lsp::DiagnosticsStore` to provide `DiagnosticsSource`.
///
/// Drains newly-dirty diagnostic entries (one `take_dirty` call per
/// turn) and groups them by file. Matches TS `getLSPDiagnosticAttachments`
/// drain-on-read semantics.
#[derive(Clone, Debug)]
pub struct LspDiagnosticsAdapter {
    store: Arc<coco_lsp::DiagnosticsStore>,
}

impl LspDiagnosticsAdapter {
    pub fn new(store: Arc<coco_lsp::DiagnosticsStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl DiagnosticsSource for LspDiagnosticsAdapter {
    async fn snapshot(&self, _agent_id: Option<&str>) -> Vec<DiagnosticFileSummary> {
        let entries = self.store.take_dirty().await;
        if entries.is_empty() {
            return Vec::new();
        }
        // Group by file, stable-order render.
        let mut by_file: std::collections::BTreeMap<
            std::path::PathBuf,
            Vec<coco_lsp::diagnostics::DiagnosticEntry>,
        > = std::collections::BTreeMap::new();
        for e in entries {
            by_file.entry(e.file.clone()).or_default().push(e);
        }
        by_file
            .into_iter()
            .map(|(path, file_entries)| DiagnosticFileSummary {
                path: path.display().to_string(),
                formatted: format_file_block(&path, &file_entries),
            })
            .collect()
    }
}

fn format_file_block(
    path: &std::path::Path,
    entries: &[coco_lsp::diagnostics::DiagnosticEntry],
) -> String {
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;
    let mut hints = 0usize;
    for e in entries {
        match e.severity {
            coco_lsp::diagnostics::DiagnosticSeverityLevel::Error => errors += 1,
            coco_lsp::diagnostics::DiagnosticSeverityLevel::Warning => warnings += 1,
            coco_lsp::diagnostics::DiagnosticSeverityLevel::Info => infos += 1,
            coco_lsp::diagnostics::DiagnosticSeverityLevel::Hint => hints += 1,
        }
    }
    let mut parts: Vec<String> = Vec::new();
    if errors > 0 {
        parts.push(format!(
            "{errors} error{}",
            if errors == 1 { "" } else { "s" }
        ));
    }
    if warnings > 0 {
        parts.push(format!(
            "{warnings} warning{}",
            if warnings == 1 { "" } else { "s" }
        ));
    }
    if infos > 0 {
        parts.push(format!("{infos} info"));
    }
    if hints > 0 {
        parts.push(format!("{hints} hint"));
    }
    let header = format!("{}: {}", path.display(), parts.join(", "));
    let lines: Vec<String> = entries
        .iter()
        .map(|e| {
            format!(
                "  {line}:{col} [{sev}] {msg}",
                line = e.line + 1,
                col = e.character + 1,
                sev = e.severity.as_str(),
                msg = e.message
            )
        })
        .collect();
    format!("{header}\n{}", lines.join("\n"))
}

// ────────────────────────────────────────────────────────────────
// MCP adapter
// ────────────────────────────────────────────────────────────────

/// Wraps `coco_mcp::McpConnectionManager` for `McpSource`. Surfaces
/// per-server instructions (for the `mcp_instructions_delta` reminder)
/// and resolves MCP resource `@`-mentions in the user prompt (for
/// the `mcp_resources` reminder).
#[derive(Clone)]
pub struct McpAdapter {
    manager: Arc<coco_mcp::McpConnectionManager>,
}

impl std::fmt::Debug for McpAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpAdapter").finish_non_exhaustive()
    }
}

impl McpAdapter {
    pub fn new(manager: Arc<coco_mcp::McpConnectionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl McpSource for McpAdapter {
    async fn instructions(&self, _agent_id: Option<&str>) -> HashMap<String, String> {
        let mut out = HashMap::new();
        for name in self.manager.registered_server_names() {
            if let Some(coco_mcp::McpConnectionState::Connected(server)) =
                self.manager.get_state(&name).await
                && let Some(text) = server.instructions.as_ref().filter(|s| !s.is_empty())
            {
                out.insert(name, text.clone());
            }
        }
        out
    }

    async fn resolve_resources(
        &self,
        _agent_id: Option<&str>,
        input: &str,
    ) -> Vec<McpResourceEntry> {
        // Parse `@server:uri` tokens from the user prompt. We don't
        // call out to MCP here — the reminder just announces which
        // resources were referenced; actual content fetch belongs in
        // a future `mcp_resource` reminder (TS `messages.ts:3877`).
        let mut out = Vec::new();
        let servers = self.manager.registered_server_names();
        for token in input.split_whitespace() {
            let Some(stripped) = token.strip_prefix('@') else {
                continue;
            };
            let Some((server, uri)) = stripped.split_once(':') else {
                continue;
            };
            if servers.iter().any(|n| n == server) {
                out.push(McpResourceEntry {
                    server: server.to_string(),
                    uri: uri.to_string(),
                });
            }
        }
        out
    }
}

// ────────────────────────────────────────────────────────────────
// IDE bridge adapter (stub — bridge crate doesn't yet expose IDE state)
// ────────────────────────────────────────────────────────────────

/// Placeholder IDE bridge adapter. Real impl wires into `coco-bridge`
/// once it exposes selection + opened-file snapshots. Until then the
/// adapter returns `None` for both queries, matching the current
/// "no IDE integration" state.
#[derive(Clone, Debug, Default)]
pub struct IdeBridgeAdapter;

impl IdeBridgeAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl IdeBridgeSource for IdeBridgeAdapter {
    async fn selection(&self, _agent_id: Option<&str>) -> Option<IdeSelectionSnapshot> {
        None
    }
    async fn opened_file(&self, _agent_id: Option<&str>) -> Option<IdeOpenedFileSnapshot> {
        None
    }
}

// ────────────────────────────────────────────────────────────────
// Swarm adapter (stub — app/state swarm surface is per-session
// spread across swarm_{runner_loop, mailbox, teammate, agent_handle})
// ────────────────────────────────────────────────────────────────

/// Placeholder swarm source. Real impl bundles refs to the swarm's
/// `TeamMailbox`, `TeamStore`, and per-agent inbox; populates the
/// three swarm reminders accordingly. Current stub returns `None` /
/// empty, so the reminders silently skip — matches the pre-swarm-wire
/// state.
#[derive(Clone, Debug, Default)]
pub struct SwarmAdapter;

impl SwarmAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SwarmSource for SwarmAdapter {
    async fn teammate_mailbox(&self, _agent_id: Option<&str>) -> Option<TeammateMailboxInfo> {
        None
    }
    async fn team_context(&self, _agent_id: Option<&str>) -> Option<TeamContextSnapshot> {
        None
    }
    async fn agent_pending_messages(&self, _agent_id: Option<&str>) -> Vec<AgentPendingMessage> {
        Vec::new()
    }
}

// ────────────────────────────────────────────────────────────────
// Memory adapter (stub — memory module is at root-modules layer
// and could host the impl directly; this adapter exists for
// consistency and can be swapped for an in-crate impl later)
// ────────────────────────────────────────────────────────────────

/// Placeholder memory source. Real impl calls
/// `coco_memory::prefetch::*` for relevant_memories and
/// `coco_context::memory::discover_nested_claude_md` for
/// nested_memories.
#[derive(Clone, Debug, Default)]
pub struct MemoryAdapter;

impl MemoryAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl MemorySource for MemoryAdapter {
    async fn nested_memories(
        &self,
        _agent_id: Option<&str>,
        _mentioned_paths: &[std::path::PathBuf],
    ) -> Vec<NestedMemoryInfo> {
        Vec::new()
    }
    async fn relevant_memories(
        &self,
        _agent_id: Option<&str>,
        _input: &str,
    ) -> Vec<RelevantMemoryInfo> {
        Vec::new()
    }
}

#[cfg(test)]
#[path = "reminder_adapters.test.rs"]
mod tests;
