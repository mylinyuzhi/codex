use std::fs;
use std::os::unix::fs::symlink;

use pretty_assertions::assert_eq;
use tempfile::tempdir;

use super::MEMORY_FILE_CANDIDATES;
use super::MEMORY_LOCAL_FILE_CANDIDATES;
use super::find_memory_files;

#[test]
fn finds_claude_md_exact_case() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("CLAUDE.md"), "x").unwrap();

    let hits = find_memory_files(dir.path(), MEMORY_FILE_CANDIDATES);
    assert_eq!(
        hits.iter()
            .map(|p| p.file_name().unwrap())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsStr::new("CLAUDE.md")]
    );
}

#[test]
fn finds_agents_md_exact_case() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("AGENTS.md"), "x").unwrap();

    let hits = find_memory_files(dir.path(), MEMORY_FILE_CANDIDATES);
    assert_eq!(
        hits.iter()
            .map(|p| p.file_name().unwrap())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsStr::new("AGENTS.md")]
    );
}

#[test]
fn finds_mixed_case_claude_md() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("Claude.md"), "x").unwrap();

    let hits = find_memory_files(dir.path(), MEMORY_FILE_CANDIDATES);
    assert_eq!(
        hits.iter()
            .map(|p| p.file_name().unwrap())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsStr::new("Claude.md")]
    );
}

#[test]
fn finds_lowercase_agents_md() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("agents.md"), "x").unwrap();

    let hits = find_memory_files(dir.path(), MEMORY_FILE_CANDIDATES);
    assert_eq!(
        hits.iter()
            .map(|p| p.file_name().unwrap())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsStr::new("agents.md")]
    );
}

#[test]
fn finds_uppercase_extension() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("CLAUDE.MD"), "x").unwrap();

    let hits = find_memory_files(dir.path(), MEMORY_FILE_CANDIDATES);
    assert_eq!(
        hits.iter()
            .map(|p| p.file_name().unwrap())
            .collect::<Vec<_>>(),
        vec![std::ffi::OsStr::new("CLAUDE.MD")]
    );
}

#[test]
fn loads_both_claude_and_agents_when_present() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("CLAUDE.md"), "c").unwrap();
    fs::write(dir.path().join("AGENTS.md"), "a").unwrap();

    let hits = find_memory_files(dir.path(), MEMORY_FILE_CANDIDATES);
    let names: Vec<&str> = hits
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap())
        .collect();
    // Stable alphabetical order by lowercased basename.
    assert_eq!(names, vec!["AGENTS.md", "CLAUDE.md"]);
}

#[test]
fn skips_directories_with_matching_name() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("CLAUDE.md")).unwrap();

    let hits = find_memory_files(dir.path(), MEMORY_FILE_CANDIDATES);
    assert!(hits.is_empty(), "directory entry should not match");
}

#[test]
fn finds_local_variants() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("CLAUDE.local.md"), "c").unwrap();
    fs::write(dir.path().join("agents.local.md"), "a").unwrap();

    let hits = find_memory_files(dir.path(), MEMORY_LOCAL_FILE_CANDIDATES);
    let names: Vec<&str> = hits
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(names, vec!["agents.local.md", "CLAUDE.local.md"]);
}

#[test]
fn missing_dir_returns_empty() {
    let dir = tempdir().unwrap();
    let nonexistent = dir.path().join("does_not_exist");

    let hits = find_memory_files(&nonexistent, MEMORY_FILE_CANDIDATES);
    assert!(hits.is_empty());
}

#[test]
fn ignores_unrelated_files() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "x").unwrap();
    fs::write(dir.path().join("notes.txt"), "x").unwrap();
    fs::write(dir.path().join("CLAUDE.md"), "x").unwrap();

    let hits = find_memory_files(dir.path(), MEMORY_FILE_CANDIDATES);
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].file_name().unwrap(),
        std::ffi::OsStr::new("CLAUDE.md")
    );
}

#[test]
fn deterministic_order_across_calls() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("AGENTS.md"), "a").unwrap();
    fs::write(dir.path().join("CLAUDE.md"), "c").unwrap();

    let first = find_memory_files(dir.path(), MEMORY_FILE_CANDIDATES);
    let second = find_memory_files(dir.path(), MEMORY_FILE_CANDIDATES);
    assert_eq!(first, second);
}

#[test]
fn follows_symlink_to_file() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("real_claude.md");
    fs::write(&target, "x").unwrap();
    symlink(&target, dir.path().join("CLAUDE.md")).unwrap();

    let hits = find_memory_files(dir.path(), MEMORY_FILE_CANDIDATES);
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].file_name().unwrap(),
        std::ffi::OsStr::new("CLAUDE.md")
    );
}
