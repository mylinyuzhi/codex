//! HTTP push/pull pipeline for team memory sync.
//!
//! TS: `services/teamMemorySync/index.ts` (1256 LoC). Ports the core
//! contract:
//!
//! - `pull(state, repo, etag)` — `GET /api/claude_code/team_memory?repo=...`,
//!   server wins per-key. Updates `state.last_known_checksum` from the
//!   response/`ETag`. Handles 200 / 304 / 404 / auth failures.
//! - `push(state, repo, entries)` — delta-only PUT. Drops entries
//!   whose local SHA matches `state.server_checksums[key]`. Splits
//!   batches over `MAX_PUT_BODY_BYTES`. Pre-scans each entry for
//!   secrets (skipped + reported on `skipped_secrets`).
//! - `compute_content_hash(s)` — `sha256:<hex>` matching the server's
//!   `entryChecksums` format (anthropic/anthropic#283027).
//!
//! Auth: caller-provided `Authorization` value (typically a Bearer
//! token from Claude.ai OAuth). The HTTP layer keeps no token state
//! itself — the caller refreshes via `coco_inference::auth` (or the
//! keyring store) and passes the live token per-call.
//!
//! Watcher integration is the next-step port (debounced file events
//! → push). The HTTP surface here is callable directly from a CLI
//! command or REPL slash-command for one-shot sync.

use std::collections::HashMap;
use std::time::Duration;

use sha2::Digest;
use sha2::Sha256;

use super::secret_scanner::scan_for_secrets;
use super::types::MAX_FILE_SIZE_BYTES;
use super::types::MAX_PUT_BODY_BYTES;
use super::types::SYNC_TIMEOUT_MS;
use super::types::SkippedSecretFile;
use super::types::SyncState;
use super::types::TeamMemoryContent;
use super::types::TeamMemoryData;
use super::types::TeamMemorySyncFetchResult;
use super::types::TeamMemorySyncPushResult;

/// Compute `sha256:<hex>` over the UTF-8 bytes of `content`. Format
/// matches the server's `entryChecksums` (TS `hashContent`).
pub fn compute_content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(7 + digest.len() * 2);
    out.push_str("sha256:");
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Build the team-memory sync endpoint URL. `base_url` is the
/// Anthropic API base (e.g. `https://api.anthropic.com`); TS reads
/// `process.env.TEAM_MEMORY_SYNC_URL ?? getOauthConfig().BASE_API_URL`.
pub fn endpoint(base_url: &str, repo_slug: &str) -> String {
    let encoded = urlencoding_encode(repo_slug);
    format!("{base_url}/api/claude_code/team_memory?repo={encoded}")
}

/// Minimal RFC-3986 query-component encoder for the `repo` slug.
/// Avoids pulling `urlencoding` for one call site.
fn urlencoding_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Pull team memory data from the server. Mirrors TS
/// `fetchTeamMemoryOnce`. Updates `state.last_known_checksum` on
/// success; resets it to `None` on 404. The caller is responsible for
/// retry policy on `success: false` results.
pub async fn pull(
    state: &mut SyncState,
    base_url: &str,
    repo_slug: &str,
    bearer_token: &str,
    if_none_match: Option<&str>,
) -> TeamMemorySyncFetchResult {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(SYNC_TIMEOUT_MS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return TeamMemorySyncFetchResult {
                success: false,
                error: Some(format!("http client: {e}")),
                ..Default::default()
            };
        }
    };

    let mut req = client
        .get(endpoint(base_url, repo_slug))
        .header("Authorization", format!("Bearer {bearer_token}"));
    if let Some(etag) = if_none_match {
        req = req.header("If-None-Match", format!("\"{}\"", etag.trim_matches('"')));
    }

    let response = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "team-memory-sync: pull request failed");
            return TeamMemorySyncFetchResult {
                success: false,
                error: Some(format!("network: {e}")),
                ..Default::default()
            };
        }
    };

    let status = response.status();
    if status.as_u16() == 404 {
        state.last_known_checksum = None;
        return TeamMemorySyncFetchResult {
            success: true,
            is_empty: true,
            ..Default::default()
        };
    }
    if status.as_u16() == 304 {
        return TeamMemorySyncFetchResult {
            success: true,
            ..Default::default()
        };
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return TeamMemorySyncFetchResult {
            success: false,
            error: Some(format!("http {status}: {body}")),
            ..Default::default()
        };
    }

    let etag_header = response
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim_matches('"').to_string());

    let data: TeamMemoryData = match response.json().await {
        Ok(d) => d,
        Err(e) => {
            return TeamMemorySyncFetchResult {
                success: false,
                error: Some(format!("invalid response: {e}")),
                ..Default::default()
            };
        }
    };

    if !data.checksum.is_empty() {
        state.last_known_checksum = Some(data.checksum.clone());
    } else if let Some(etag) = etag_header {
        state.last_known_checksum = Some(etag);
    }

    state.server_checksums = data.content.entry_checksums.clone();

    TeamMemorySyncFetchResult {
        success: true,
        data: Some(data),
        is_empty: false,
        ..Default::default()
    }
}

/// One entry to consider for push. The caller (typically the watcher
/// or a CLI command) reads files off disk + provides them here; the
/// service stays unaware of filesystem layout to keep tests
/// hermetic.
#[derive(Debug, Clone)]
pub struct PushEntry {
    /// Path relative to the team memory dir (e.g. `"MEMORY.md"`).
    pub path: String,
    /// UTF-8 entry content.
    pub content: String,
}

/// Push delta entries. Mirrors TS `pushTeamMemory` core path:
///
/// 1. Pre-scan each entry for secrets — skipped entries land on
///    `result.skipped_secrets`, never sent to the server.
/// 2. Drop entries whose local SHA matches `state.server_checksums`
///    (delta upload).
/// 3. Split the batch over `MAX_PUT_BODY_BYTES` into sequential PUTs
///    (server upsert-merge makes that safe).
/// 4. Update `state.server_checksums` for every successfully-pushed
///    key so the next push round computes a smaller delta.
///
/// Returns the count actually uploaded. On HTTP error the function
/// returns early — partial uploads still update `state` for the
/// successful chunks.
pub async fn push(
    state: &mut SyncState,
    base_url: &str,
    repo_slug: &str,
    bearer_token: &str,
    entries: &[PushEntry],
) -> TeamMemorySyncPushResult {
    let mut result = TeamMemorySyncPushResult::default();

    // Step 1: secret pre-scan + per-entry size cap.
    //
    // Oversized entries are also dropped here — the server enforces the
    // same cap but a 250 KB+ entry would land in our batch and trip the
    // gateway 413 unstructured response (vs the app's structured
    // too-many-entries 413). Better to filter client-side.
    let mut clean: Vec<&PushEntry> = Vec::new();
    for entry in entries {
        if entry.content.len() > MAX_FILE_SIZE_BYTES {
            tracing::warn!(
                path = %entry.path,
                size = entry.content.len(),
                cap = MAX_FILE_SIZE_BYTES,
                "team-memory-sync: dropping oversized entry from push batch"
            );
            continue;
        }
        if let Some(skipped) = scan_for_secrets(&entry.path, &entry.content) {
            result.skipped_secrets.push(skipped);
            continue;
        }
        clean.push(entry);
    }

    // Step 2: delta filter.
    let mut to_push: Vec<(&PushEntry, String)> = Vec::new();
    for entry in clean {
        let local_hash = compute_content_hash(&entry.content);
        let same = state
            .server_checksums
            .get(&entry.path)
            .is_some_and(|h| h == &local_hash);
        if !same {
            to_push.push((entry, local_hash));
        }
    }
    if to_push.is_empty() {
        result.success = true;
        return result;
    }

    // Step 3: split into batches under MAX_PUT_BODY_BYTES.
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(SYNC_TIMEOUT_MS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            result.error = Some(format!("http client: {e}"));
            return result;
        }
    };

    let mut batches: Vec<HashMap<String, String>> = Vec::new();
    let mut current: HashMap<String, String> = HashMap::new();
    let mut current_bytes = 2; // `{}` overhead
    for (entry, _hash) in &to_push {
        // Rough body estimate: key + value + JSON quoting overhead.
        let entry_bytes = entry.path.len() + entry.content.len() + 8;
        if current_bytes + entry_bytes > MAX_PUT_BODY_BYTES && !current.is_empty() {
            batches.push(std::mem::take(&mut current));
            current_bytes = 2;
        }
        current.insert(entry.path.clone(), entry.content.clone());
        current_bytes += entry_bytes;
    }
    if !current.is_empty() {
        batches.push(current);
    }

    // Step 4: PUT each batch sequentially.
    //
    // `If-Match: "<last_known_checksum>"` is sent on every batch when
    // we know the server's last checksum. TS parity: PUTs always
    // include `If-Match` so the server can reject (412) stale writes
    // that would clobber a teammate's concurrent push. Without this
    // header, two teammates pushing simultaneously silently
    // last-writer-wins and the loser's changes are lost.
    let mut uploaded = 0i32;
    for batch in batches {
        let body = serde_json::json!({
            "entries": batch,
        });
        let mut req = client
            .put(endpoint(base_url, repo_slug))
            .header("Authorization", format!("Bearer {bearer_token}"))
            .header("Content-Type", "application/json")
            .json(&body);
        if let Some(etag) = &state.last_known_checksum {
            req = req.header("If-Match", format!("\"{}\"", etag.trim_matches('"')));
        }
        let put = req.send().await;
        match put {
            Ok(resp) if resp.status().is_success() => {
                uploaded += batch.len() as i32;
                // Update server_checksums for the successfully pushed keys.
                for (path, content) in &batch {
                    state
                        .server_checksums
                        .insert(path.clone(), compute_content_hash(content));
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                result.error = Some(format!("http {status}: {body}"));
                result.uploaded_count = uploaded;
                return result;
            }
            Err(e) => {
                tracing::warn!(error = %e, "team-memory-sync: push request failed");
                result.error = Some(format!("network: {e}"));
                result.uploaded_count = uploaded;
                return result;
            }
        }
    }

    result.success = true;
    result.uploaded_count = uploaded;
    result
}

/// Convenience: filter `entries` through the secret scanner only,
/// returning the clean set + skipped report. Useful for callers that
/// want to inspect what would be pushed before actually sending.
pub fn scan_only(entries: &[PushEntry]) -> (Vec<&PushEntry>, Vec<SkippedSecretFile>) {
    let mut clean = Vec::new();
    let mut skipped = Vec::new();
    for e in entries {
        if let Some(s) = scan_for_secrets(&e.path, &e.content) {
            skipped.push(s);
        } else {
            clean.push(e);
        }
    }
    (clean, skipped)
}

/// Apply pulled `TeamMemoryContent` to the local file tree under
/// `dir`. Server wins per-key — every entry gets written verbatim.
/// Existing local-only files are NOT removed (TS parity: deletions
/// don't propagate). Errors during individual file writes are logged
/// but don't abort the operation.
///
/// Three guarantees per entry, mirroring TS `writeRemoteEntriesToLocal`:
///
/// 1. **Path validation** via [`crate::path::team::validate_team_mem_key`]
///    — null bytes, UNC `\\` / `//`, drive-root, unexpanded tilde,
///    URL-encoded `%2e%2e`, fullwidth-NFKC traversal, planted symlinks
///    pointing outside `dir`. Defense-in-depth against a malicious or
///    compromised server.
/// 2. **Per-entry size cap** — refuse any entry larger than
///    [`MAX_FILE_SIZE_BYTES`] (250 KB). The server has the same cap
///    but a bug or rogue server could deliver an oversized blob.
/// 3. **Skip-if-equal** — read the existing file first and skip the
///    write when the byte content already matches. Preserves mtime so
///    a wired-up file-watcher doesn't trigger a spurious push-back
///    (ping-pong: pull → watcher → push → 412 conflict loop).
pub async fn apply_pulled_content(dir: &std::path::Path, content: &TeamMemoryContent) {
    if let Err(e) = tokio::fs::create_dir_all(dir).await {
        tracing::warn!(error = %e, dir = %dir.display(), "team-memory-sync: mkdir failed");
        return;
    }
    for (rel_path, body) in &content.entries {
        // (1) Path validation — fails closed on any taxonomy hit.
        let target = match crate::path::validate_team_mem_key(rel_path, dir) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(
                    path = %rel_path,
                    error = ?e,
                    "team-memory-sync: rejected key (path validation)"
                );
                continue;
            }
        };
        // (2) Size cap.
        if body.len() > MAX_FILE_SIZE_BYTES {
            tracing::warn!(
                path = %rel_path,
                size = body.len(),
                cap = MAX_FILE_SIZE_BYTES,
                "team-memory-sync: skipping oversized pulled entry"
            );
            continue;
        }
        // (3) Skip-if-equal — read current bytes, compare, skip write
        // if matching. Missing file = no skip. Errors fall through to
        // the write attempt.
        if let Ok(existing) = tokio::fs::read(&target).await
            && existing == body.as_bytes()
        {
            continue;
        }
        if let Some(parent) = target.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Err(e) = tokio::fs::write(&target, body).await {
            tracing::warn!(error = %e, path = %target.display(), "team-memory-sync: write failed");
        }
    }
}

#[cfg(test)]
#[path = "service.test.rs"]
mod tests;
