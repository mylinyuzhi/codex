use std::io::Write;
use std::path::PathBuf;

use tempfile::TempDir;

use super::*;
use crate::config::ResolvedAutoMemoryConfig;

fn test_config(dir: PathBuf) -> ResolvedAutoMemoryConfig {
    ResolvedAutoMemoryConfig {
        enabled: true,
        directory: dir,
        max_lines: 200,
        max_relevant_files: 5,
        max_lines_per_file: 200,
        relevant_search_timeout_ms: 5000,
        relevant_memories_enabled: false,
        memory_extraction_enabled: false,
        max_frontmatter_lines: 20,
        staleness_warning_days: 1,
        relevant_memories_throttle_turns: 3,
        max_files_to_scan: 200,
        min_keyword_length: 3,
        disable_reason: None,
    }
}

#[tokio::test]
async fn test_refresh_loads_index() {
    let tmp = TempDir::new().unwrap();
    let memory_dir = tmp.path().join("memory");
    std::fs::create_dir_all(&memory_dir).unwrap();
    let mut f = std::fs::File::create(memory_dir.join("MEMORY.md")).unwrap();
    write!(f, "# Index\n- [a](a.md)").unwrap();

    let state = AutoMemoryState::new(test_config(memory_dir));
    assert!(state.index().await.is_none());

    state.refresh().await;
    let index = state.index().await.unwrap();
    assert!(index.raw_content.contains("Index"));
}

#[tokio::test]
async fn test_refresh_no_memory_file() {
    let tmp = TempDir::new().unwrap();
    let memory_dir = tmp.path().join("memory");
    std::fs::create_dir_all(&memory_dir).unwrap();

    let state = AutoMemoryState::new(test_config(memory_dir));
    state.refresh().await;
    assert!(state.index().await.is_none());
}

#[tokio::test]
async fn test_disabled_state_skips_refresh() {
    let tmp = TempDir::new().unwrap();
    let mut config = test_config(tmp.path().to_path_buf());
    config.enabled = false;

    let state = AutoMemoryState::new(config);
    state.refresh().await;
    assert!(state.index().await.is_none());
}

#[tokio::test]
async fn test_refresh_creates_directory() {
    let tmp = TempDir::new().unwrap();
    let memory_dir = tmp.path().join("nonexistent_sub").join("memory");
    assert!(!memory_dir.exists());

    let state = AutoMemoryState::new(test_config(memory_dir.clone()));
    state.refresh().await;
    // Directory should have been created
    assert!(memory_dir.exists());
    // No MEMORY.md file yet, so index is None
    assert!(state.index().await.is_none());
}
