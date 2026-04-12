//! Task output persistence to disk.
//!
//! TS: utils/task/diskOutput.ts — persists task output to files.

use std::path::Path;
use std::path::PathBuf;

/// Get the output path for a task.
pub fn get_task_output_path(sessions_dir: &Path, session_id: &str, task_id: &str) -> PathBuf {
    sessions_dir
        .join(session_id)
        .join("tasks")
        .join(format!("{task_id}.output"))
}

/// Write task output to disk.
pub fn write_task_output(path: &Path, output: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, output)?;
    Ok(())
}

/// Read task output from disk.
pub fn read_task_output(path: &Path) -> anyhow::Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

/// Append to task output (streaming output).
pub fn append_task_output(path: &Path, chunk: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(chunk.as_bytes())?;
    Ok(())
}

/// Check if task output exists.
pub fn task_output_exists(path: &Path) -> bool {
    path.exists()
}

/// Delete task output.
pub fn delete_task_output(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "output.test.rs"]
mod tests;
