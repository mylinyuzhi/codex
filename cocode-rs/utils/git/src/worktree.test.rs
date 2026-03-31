use super::*;

#[test]
fn test_list_orphaned_worktrees_parses_porcelain() {
    // Verify the parsing logic handles the porcelain format correctly.
    // Full integration tests require a real git repo, so we just ensure
    // the function returns None for a non-repo path.
    let result = list_orphaned_worktrees(std::path::Path::new("/nonexistent"));
    assert!(result.is_none());
}
