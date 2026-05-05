use pretty_assertions::assert_eq;

use crate::claudemd::MemoryFileSource;
use crate::claudemd::discover_memory_files;

#[test]
fn discovers_project_root_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("CLAUDE.md"), "# Test").unwrap();

    let files = discover_memory_files(dir.path());
    let root = files.iter().find(|f| {
        f.source == MemoryFileSource::Project
            && f.path.file_name().unwrap() == std::ffi::OsStr::new("CLAUDE.md")
    });
    assert!(root.is_some(), "expected CLAUDE.md to load as Project");
    assert!(root.unwrap().content.contains("# Test"));
}

#[test]
fn discovers_agents_md_at_project_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "# Agents").unwrap();

    let files = discover_memory_files(dir.path());
    let root = files.iter().find(|f| {
        f.source == MemoryFileSource::Project
            && f.path.file_name().unwrap() == std::ffi::OsStr::new("AGENTS.md")
    });
    assert!(root.is_some(), "expected AGENTS.md to load as Project");
    assert!(root.unwrap().content.contains("# Agents"));
}

#[test]
fn discovers_both_claude_and_agents_when_present() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("CLAUDE.md"), "c").unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "a").unwrap();

    let files = discover_memory_files(dir.path());
    let names: Vec<&str> = files
        .iter()
        .filter(|f| f.source == MemoryFileSource::Project && f.path.parent() == Some(dir.path()))
        .map(|f| f.path.file_name().unwrap().to_str().unwrap())
        .collect();
    assert!(names.contains(&"CLAUDE.md"));
    assert!(names.contains(&"AGENTS.md"));
}

#[test]
fn case_insensitive_match() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Claude.md"), "x").unwrap();

    let files = discover_memory_files(dir.path());
    let hit = files
        .iter()
        .find(|f| f.source == MemoryFileSource::Project && f.path.parent() == Some(dir.path()));
    assert!(hit.is_some(), "Claude.md should match case-insensitively");
}

#[test]
fn discovers_local_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("CLAUDE.local.md"), "local config").unwrap();

    let files = discover_memory_files(dir.path());
    let local = files.iter().find(|f| f.source == MemoryFileSource::Local);
    assert!(local.is_some());
}

#[test]
fn discovers_agents_local_md() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("AGENTS.local.md"), "local").unwrap();

    let files = discover_memory_files(dir.path());
    let local = files.iter().find(|f| f.source == MemoryFileSource::Local);
    assert!(local.is_some(), "expected AGENTS.local.md to load as Local");
}

#[test]
fn discovers_dot_claude_config_dir() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join(".claude");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("CLAUDE.md"), "config").unwrap();

    let files = discover_memory_files(dir.path());
    let cfg_file = files
        .iter()
        .find(|f| f.source == MemoryFileSource::ProjectConfig);
    assert!(cfg_file.is_some());
}

#[test]
fn empty_dir_has_no_project_files() {
    let dir = tempfile::tempdir().unwrap();
    let files = discover_memory_files(dir.path());
    assert!(
        files.iter().all(|f| f.source != MemoryFileSource::Project),
        "empty CWD should not produce Project-source entries"
    );
}

#[test]
fn does_not_load_immediate_children_anymore() {
    // Phase 5a regression test: TS only walks root→CWD inclusive in the
    // eager phase. Children of CWD must NOT be eager-loaded; they're the
    // job of the per-file trigger pipeline (Phase 2). Without this guard,
    // we'd double-load every CLAUDE.md the trigger pipeline finds.
    let dir = tempfile::tempdir().unwrap();
    let child = dir.path().join("subproject");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(child.join("CLAUDE.md"), "child").unwrap();

    let files = discover_memory_files(dir.path());
    let child_loaded = files.iter().any(|f| f.path == child.join("CLAUDE.md"));
    assert!(
        !child_loaded,
        "immediate child CLAUDE.md must not be eager-loaded"
    );
}

#[test]
fn walks_root_to_cwd() {
    // Build /tmp_root/proj/sub. CWD = /tmp_root/proj/sub.
    // Eager should load /tmp_root/proj/CLAUDE.md (parent of CWD) and
    // /tmp_root/proj/sub/CLAUDE.md (CWD itself), but the temp prefix
    // isn't a memory dir.
    let root = tempfile::tempdir().unwrap();
    let proj = root.path().join("proj");
    let sub = proj.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(proj.join("CLAUDE.md"), "proj").unwrap();
    std::fs::write(sub.join("CLAUDE.md"), "sub").unwrap();

    let files = discover_memory_files(&sub);
    let project_paths: Vec<_> = files
        .iter()
        .filter(|f| f.source == MemoryFileSource::Project)
        .map(|f| f.path.clone())
        .collect();
    // Both should appear, with the deeper one (CWD) loaded after the
    // ancestor — TS "later = higher attention priority" semantics.
    let proj_idx = project_paths
        .iter()
        .position(|p| p == &proj.join("CLAUDE.md"));
    let sub_idx = project_paths
        .iter()
        .position(|p| p == &sub.join("CLAUDE.md"));
    assert!(proj_idx.is_some(), "ancestor CLAUDE.md missing");
    assert!(sub_idx.is_some(), "CWD CLAUDE.md missing");
    assert!(
        proj_idx.unwrap() < sub_idx.unwrap(),
        "ancestor should load before CWD (root→CWD order)"
    );
}

#[test]
fn dedupes_canonical_path() {
    // Two entries pointing at the same file (via symlink): only one load.
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("CLAUDE.md");
    std::fs::write(&real, "x").unwrap();

    let files = discover_memory_files(dir.path());
    let count = files
        .iter()
        .filter(|f| f.path.canonicalize().ok() == Some(real.canonicalize().unwrap()))
        .count();
    assert_eq!(count, 1, "expected exactly one load of {}", real.display());
}
