use super::*;
use pretty_assertions::assert_eq;
use std::path::Path;
use std::path::PathBuf;

#[test]
fn override_path_wins() {
    let dir = MemoryDir::resolve(
        Path::new("/home/u/.coco"),
        Path::new("/work/repo"),
        Some(Path::new("/custom/memory")),
    );
    assert_eq!(dir.personal, PathBuf::from("/custom/memory"));
    assert_eq!(dir.team, PathBuf::from("/custom/memory/team"));
}

#[test]
fn default_layout_outside_git_uses_project_path() {
    // tmp dir won't be a git repo so canonicalization falls through.
    let temp = tempfile::tempdir().unwrap();
    let dir = MemoryDir::resolve(Path::new("/home/u/.coco"), temp.path(), None);
    let sanitized = sanitize_project_path(temp.path());
    assert_eq!(
        dir.personal,
        PathBuf::from("/home/u/.coco")
            .join("projects")
            .join(sanitized)
            .join("memory")
    );
}

#[test]
fn sanitize_replaces_separators() {
    assert_eq!(sanitize_project_path(Path::new("/a/b/c")), "a-b-c");
    assert_eq!(sanitize_project_path(Path::new("a/b")), "a-b");
}

#[test]
fn personal_and_team_index_paths() {
    let dir = MemoryDir {
        personal: PathBuf::from("/m"),
        team: PathBuf::from("/m/team"),
    };
    assert_eq!(dir.personal_index(), PathBuf::from("/m/MEMORY.md"));
    assert_eq!(dir.team_index(), PathBuf::from("/m/team/MEMORY.md"));
}
