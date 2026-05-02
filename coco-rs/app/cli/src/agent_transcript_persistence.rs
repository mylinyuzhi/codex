//! Production [`coco_tool_runtime::AgentTranscriptStore`] backed by
//! [`coco_session::TranscriptStore`].
//!
//! Bridges `coco-coordinator` (root layer, where bg AgentTool
//! spawns register) to `coco-session` (app layer, where transcript
//! files actually live) without a layer-rule violation. The trait
//! lives in `coco-tool-runtime`; this impl wraps the `TranscriptStore`
//! and translates its sync API to the trait's async surface.
//!
//! ## Storage layout
//!
//! For each background agent:
//!
//! ```
//! <sessions_dir>/<session_id>/subagents/agent-<id>.jsonl    # full message history
//! <sessions_dir>/<session_id>/subagents/agent-<id>.meta.json # AgentSpawnMetadata
//! ```
//!
//! Mirrors TS `getAgentTranscriptPath` /
//! `getAgentMetadataPath` (`utils/sessionStorage.ts:247-262`).
//!
//! ## Why a separate file from the per-task `.output`
//!
//! `coco_cli::disk_task_output` writes raw text deltas to
//! `<config_home>/cache/tasks/<session>/<task_id>.output` for the
//! `TaskOutput` model-facing tool. That file is text — TS-faithful
//! `initTaskOutputAsSymlink` would point it at the JSONL transcript
//! so `TaskOutput` reads structured entries instead. coco-rs keeps
//! them as separate streams: text for the model's progress view,
//! JSONL for resume. The deliberate divergence preserves
//! `TaskOutput`'s text contract while still giving `agent/resume`
//! a clean conversation log to rehydrate from.

use std::sync::Arc;

use async_trait::async_trait;
use coco_session::{AgentMetadata, TranscriptStore};
use coco_tool_runtime::{AgentSpawnMetadata, AgentTranscriptStore};

/// Trait impl for the production transcript store. Cloning is
/// cheap (`Arc<TranscriptStore>` underneath); same instance is
/// shared between `SwarmAgentHandle` (writer side) and the resume
/// entry point (reader side).
pub struct SessionAgentTranscriptStore {
    store: Arc<TranscriptStore>,
}

impl SessionAgentTranscriptStore {
    pub fn new(store: Arc<TranscriptStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl AgentTranscriptStore for SessionAgentTranscriptStore {
    async fn append_agent_messages(
        &self,
        session_id: &str,
        agent_id: &str,
        messages: Vec<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let store = self.store.clone();
        let session_id = session_id.to_string();
        let agent_id = agent_id.to_string();
        // `TranscriptStore::append_agent_messages` is sync (std::fs).
        // Hop to the blocking pool so the tokio worker isn't stalled
        // by disk I/O — small writes are fine but the agent's full
        // history can be megabytes.
        tokio::task::spawn_blocking(move || {
            store.append_agent_messages(&session_id, &agent_id, &messages)
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking join: {e}"))?
    }

    async fn load_agent_messages(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> anyhow::Result<Option<Vec<serde_json::Value>>> {
        let store = self.store.clone();
        let session_id = session_id.to_string();
        let agent_id = agent_id.to_string();
        tokio::task::spawn_blocking(move || store.load_agent_messages(&session_id, &agent_id))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking join: {e}"))?
    }

    async fn write_agent_metadata(
        &self,
        session_id: &str,
        agent_id: &str,
        metadata: &AgentSpawnMetadata,
    ) -> anyhow::Result<()> {
        let store = self.store.clone();
        let session_id = session_id.to_string();
        let agent_id = agent_id.to_string();
        // Translate the trait DTO into the session-crate's struct.
        // Both are 1:1 in shape — separate types only because the
        // trait can't reach into `coco-session` from a lower layer.
        let session_meta = AgentMetadata {
            agent_type: metadata.agent_type.clone(),
            worktree_path: metadata.worktree_path.clone(),
            description: metadata.description.clone(),
        };
        tokio::task::spawn_blocking(move || {
            store.write_agent_metadata(&session_id, &agent_id, &session_meta)
        })
        .await
        .map_err(|e| anyhow::anyhow!("spawn_blocking join: {e}"))?
    }

    async fn read_agent_metadata(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> anyhow::Result<Option<AgentSpawnMetadata>> {
        let store = self.store.clone();
        let session_id = session_id.to_string();
        let agent_id = agent_id.to_string();
        let session_meta: Option<AgentMetadata> =
            tokio::task::spawn_blocking(move || store.read_agent_metadata(&session_id, &agent_id))
                .await
                .map_err(|e| anyhow::anyhow!("spawn_blocking join: {e}"))??;
        Ok(session_meta.map(|m| AgentSpawnMetadata {
            agent_type: m.agent_type,
            worktree_path: m.worktree_path,
            description: m.description,
        }))
    }
}

#[cfg(test)]
#[path = "agent_transcript_persistence.test.rs"]
mod tests;
