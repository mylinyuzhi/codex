use std::path::Path;
use std::path::PathBuf;

use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;

/// Records subagent interactions to a JSONL file.
///
/// Each entry is written as a single JSON line, suitable for post-hoc analysis
/// and debugging.
pub struct TranscriptRecorder {
    path: PathBuf,
}

impl TranscriptRecorder {
    /// Create a new recorder that writes to the given path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Append an entry to the transcript file.
    ///
    /// Each entry is serialized as a single JSON line followed by a newline.
    /// O_APPEND guarantees atomic writes for JSONL entries (<4096 bytes on most POSIX systems).
    pub async fn record(&self, entry: &serde_json::Value) -> std::io::Result<()> {
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;

        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        Ok(())
    }

    /// Returns the path to the transcript file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read all entries from a transcript file.
    ///
    /// Each line is parsed as a JSON value. Empty and invalid lines are skipped.
    pub async fn read_transcript(path: &Path) -> std::io::Result<Vec<serde_json::Value>> {
        let content = tokio::fs::read_to_string(path).await?;
        Ok(content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect())
    }

    /// Read transcript entries from this recorder's path.
    pub async fn read_entries(&self) -> std::io::Result<Vec<serde_json::Value>> {
        Self::read_transcript(&self.path).await
    }

    /// Record an incremental progress update for a background agent.
    ///
    /// Appends a timestamped `"type": "progress"` entry to the transcript.
    pub async fn record_progress(&self, agent_id: &str, message: &str) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "type": "progress",
            "agent_id": agent_id,
            "message": message,
            "timestamp": unix_timestamp(),
        });
        self.record(&entry).await
    }

    /// Record a per-turn result for a background agent.
    ///
    /// Appends a timestamped `"type": "turn_result"` entry to the transcript.
    pub async fn record_turn_result(
        &self,
        agent_id: &str,
        turn: i32,
        text: &str,
    ) -> std::io::Result<()> {
        let entry = serde_json::json!({
            "type": "turn_result",
            "agent_id": agent_id,
            "turn": turn,
            "text": text,
            "timestamp": unix_timestamp(),
        });
        self.record(&entry).await
    }
}

/// Read JSONL entries from a file starting at a byte offset.
///
/// Returns the parsed entries and the new byte offset (for subsequent delta reads).
/// This enables incremental output reading without re-reading the entire file.
pub async fn read_from_offset(
    path: &Path,
    byte_offset: u64,
) -> std::io::Result<(Vec<serde_json::Value>, u64)> {
    let mut file = tokio::fs::File::open(path).await?;
    let metadata = file.metadata().await?;
    let file_len = metadata.len();

    if byte_offset >= file_len {
        return Ok((Vec::new(), byte_offset));
    }

    file.seek(std::io::SeekFrom::Start(byte_offset)).await?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).await?;

    let entries: Vec<serde_json::Value> = buf
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    Ok((entries, file_len))
}

/// Filter transcript entries with empty or whitespace-only output.
///
/// Removes entries where the `"output"` field is empty, whitespace-only,
/// or missing. This sanitizes transcripts for resume by stripping entries
/// that provide no meaningful context (analogous to Claude Code's
/// `filterWhitespaceAssistant` and `filterThinkingOnlyAssistant`).
pub fn filter_empty_entries(entries: &[serde_json::Value]) -> Vec<serde_json::Value> {
    entries
        .iter()
        .filter(|entry| {
            // Keep entries that have no "output" field (e.g., progress, prompts)
            let Some(output) = entry.get("output") else {
                return true;
            };
            // Keep entries with non-empty, non-whitespace output
            match output.as_str() {
                Some(s) => !s.trim().is_empty(),
                // Non-string output (arrays, objects, etc.) — keep
                None => !output.is_null(),
            }
        })
        .cloned()
        .collect()
}

/// Unix epoch timestamp (seconds.millis) for transcript entries.
fn unix_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", now.as_secs(), now.subsec_millis())
}

#[cfg(test)]
#[path = "transcript.test.rs"]
mod tests;
