use crate::claudemd::ClaudeMdSource;
use crate::claudemd::discover_claude_md_files;

#[test]
fn test_discover_project_root_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("CLAUDE.md"), "# Test").unwrap();

    let files = discover_claude_md_files(dir.path());
    assert!(!files.is_empty());
    let root = files
        .iter()
        .find(|f| f.source == ClaudeMdSource::ProjectRoot);
    assert!(root.is_some());
    assert!(root.unwrap().content.contains("# Test"));
}

#[test]
fn test_discover_local_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    let claude_dir = dir.path().join(".claude");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(claude_dir.join("CLAUDE.local.md"), "local config").unwrap();

    let files = discover_claude_md_files(dir.path());
    let local = files.iter().find(|f| f.source == ClaudeMdSource::Local);
    assert!(local.is_some());
}

#[test]
fn test_discover_empty_dir() {
    let dir = tempfile::tempdir().unwrap();
    let files = discover_claude_md_files(dir.path());
    // May include user-global, but no project files
    assert!(
        files
            .iter()
            .all(|f| f.source != ClaudeMdSource::ProjectRoot)
    );
}

#[test]
fn test_discover_child_claude_md() {
    let dir = tempfile::tempdir().unwrap();
    let child = dir.path().join("subproject");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(child.join("CLAUDE.md"), "child").unwrap();

    let files = discover_claude_md_files(dir.path());
    let child_file = files.iter().find(|f| f.source == ClaudeMdSource::Child);
    assert!(child_file.is_some());
}
