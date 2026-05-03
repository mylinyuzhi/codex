//! Team Memory Sync data types.
//!
//! TS: `services/teamMemorySync/types.ts` — Zod schemas + types.
//! API contract: anthropic/anthropic#250711, #283027, #293258.

use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

/// Content portion of team memory data — flat key-value storage.
/// Keys are file paths relative to the team memory dir
/// (e.g. `"MEMORY.md"`, `"patterns.md"`). Values are UTF-8 string
/// content (typically Markdown).
///
/// TS: `TeamMemoryContentSchema`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamMemoryContent {
    pub entries: HashMap<String, String>,
    /// Per-key SHA-256 of entry content (`sha256:<hex>`). Added in
    /// anthropic/anthropic#283027. Optional for forward-compat with
    /// older server deployments.
    #[serde(default, rename = "entryChecksums")]
    pub entry_checksums: HashMap<String, String>,
}

/// Full response from `GET /api/claude_code/team_memory`.
/// TS: `TeamMemoryDataSchema`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMemoryData {
    #[serde(rename = "organizationId")]
    pub organization_id: String,
    pub repo: String,
    pub version: i64,
    /// ISO 8601 timestamp.
    #[serde(rename = "lastModified")]
    pub last_modified: String,
    /// SHA256 with `sha256:` prefix.
    pub checksum: String,
    pub content: TeamMemoryContent,
}

/// A file skipped during push because it contains a detected secret.
/// Path is relative to the team memory directory. Only the matched
/// rule ID is recorded — never the secret value.
///
/// TS: `SkippedSecretFile`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedSecretFile {
    pub path: String,
    /// Gitleaks rule ID (e.g. `"github-pat"`, `"aws-access-token"`).
    #[serde(rename = "ruleId")]
    pub rule_id: String,
    /// Human-readable label derived from rule ID.
    pub label: String,
}

/// Result from a sync fetch (pull) operation.
/// TS: `TeamMemorySyncFetchResult`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamMemorySyncFetchResult {
    pub success: bool,
    pub data: Option<TeamMemoryData>,
    /// `true` when the server returned 404 (no data exists yet).
    #[serde(default, rename = "isEmpty")]
    pub is_empty: bool,
    pub error: Option<String>,
}

/// Result from a sync push (delta upload) operation.
/// TS: `TeamMemorySyncPushResult`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamMemorySyncPushResult {
    pub success: bool,
    /// Count of entries actually uploaded (excludes unchanged keys).
    #[serde(default, rename = "uploadedCount")]
    pub uploaded_count: i32,
    /// Files skipped because they contained a detected secret.
    #[serde(default, rename = "skippedSecrets")]
    pub skipped_secrets: Vec<SkippedSecretFile>,
    pub error: Option<String>,
}

/// Result from a low-level upload chunk.
/// TS: `TeamMemorySyncUploadResult`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamMemorySyncUploadResult {
    pub success: bool,
    pub status: i32,
    pub error: Option<String>,
}

/// Hashes-only response from `GET ?view=hashes` — metadata + per-key
/// checksums without entry bodies. TS: `TeamMemoryHashesResult`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMemoryHashesResult {
    #[serde(rename = "organizationId")]
    pub organization_id: String,
    pub repo: String,
    pub version: i64,
    #[serde(rename = "lastModified")]
    pub last_modified: String,
    pub checksum: String,
    #[serde(default, rename = "entryChecksums")]
    pub entry_checksums: HashMap<String, String>,
}

/// Mutable per-session state threaded through every sync call.
///
/// TS: `SyncState` from `index.ts:100-119`. Created once per session
/// by the watcher and passed to all sync functions. Tests instantiate
/// fresh per-test for isolation.
#[derive(Debug, Default)]
pub struct SyncState {
    /// Last known server checksum (ETag) for conditional requests.
    pub last_known_checksum: Option<String>,
    /// Per-key content hash (`sha256:<hex>`) of what we believe the
    /// server holds. Populated from `entryChecksums` on pull and from
    /// local hashes on successful push. Drives delta computation —
    /// only keys whose local hash differs are uploaded.
    pub server_checksums: HashMap<String, String>,
    /// Server-enforced max_entries cap, learned from a structured 413
    /// response (anthropic/anthropic#293258 adds error_code +
    /// extra_details.max_entries). Stays `None` until a 413 is
    /// observed — the server's cap is per-org and there's no correct
    /// client-side default. While `None`, push sends everything and
    /// lets the server be authoritative.
    pub server_max_entries: Option<i32>,
}

/// Per-entry size cap (TS `MAX_FILE_SIZE_BYTES`).
pub const MAX_FILE_SIZE_BYTES: usize = 250_000;

/// Gateway body-size cap (TS `MAX_PUT_BODY_BYTES`). Batches larger
/// than this get split into sequential PUTs (server upsert-merge
/// makes that safe).
pub const MAX_PUT_BODY_BYTES: usize = 200_000;

/// Sync request timeout (TS `TEAM_MEMORY_SYNC_TIMEOUT_MS`).
pub const SYNC_TIMEOUT_MS: u64 = 30_000;
