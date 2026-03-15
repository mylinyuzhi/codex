use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

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
    pub fn record(&self, entry: &serde_json::Value) -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// Returns the path to the transcript file.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Read all entries from a transcript file.
    ///
    /// Each line is parsed as a JSON value. Empty and invalid lines are skipped.
    pub fn read_transcript(path: &Path) -> std::io::Result<Vec<serde_json::Value>> {
        let content = std::fs::read_to_string(path)?;
        Ok(content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect())
    }

    /// Read transcript entries from this recorder's path.
    pub fn read_entries(&self) -> std::io::Result<Vec<serde_json::Value>> {
        Self::read_transcript(&self.path)
    }
}

#[cfg(test)]
#[path = "transcript.test.rs"]
mod tests;
