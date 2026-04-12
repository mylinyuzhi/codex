//! CLAUDE.md / MEMORY.md directory management.
//!
//! TS: memdir/ (1.7K LOC) — manages the .claude/memory/ directory structure.

use std::path::Path;
use std::path::PathBuf;

/// Memory directory layout.
pub struct MemoryDir {
    pub root: PathBuf,
    pub index_path: PathBuf,
}

impl MemoryDir {
    /// Create a new memory directory manager.
    pub fn new(project_dir: &Path) -> Self {
        let root = project_dir.join(".claude").join("memory");
        let index_path = root.join("MEMORY.md");
        Self { root, index_path }
    }

    /// Ensure the memory directory exists.
    pub fn ensure_exists(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        Ok(())
    }

    /// Get path for a new memory file.
    pub fn file_path(&self, filename: &str) -> PathBuf {
        self.root.join(filename)
    }

    /// List all memory files (excluding MEMORY.md).
    pub fn list_files(&self) -> anyhow::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        if !self.root.exists() {
            return Ok(files);
        }
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md")
                && path.file_name().is_some_and(|n| n != "MEMORY.md")
            {
                files.push(path);
            }
        }
        files.sort();
        Ok(files)
    }

    /// Generate a unique filename for a memory entry.
    pub fn generate_filename(&self, name: &str) -> String {
        let sanitized: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{sanitized}.md")
    }

    /// Check if MEMORY.md index exists.
    pub fn has_index(&self) -> bool {
        self.index_path.exists()
    }

    /// Read the MEMORY.md index content.
    pub fn read_index(&self) -> anyhow::Result<String> {
        Ok(std::fs::read_to_string(&self.index_path)?)
    }

    /// Count memory files.
    pub fn file_count(&self) -> anyhow::Result<usize> {
        Ok(self.list_files()?.len())
    }
}

#[cfg(test)]
#[path = "memdir.test.rs"]
mod tests;
