use super::*;
use pretty_assertions::assert_eq;
use std::path::PathBuf;

#[test]
fn test_list_orphaned_worktrees_parses_porcelain() {
    // Verify the parsing logic handles the porcelain format correctly.
    // Full integration tests require a real git repo, so we just ensure
    // the function returns None for a non-repo path.
    let result = list_orphaned_worktrees(std::path::Path::new("/nonexistent"));
    assert!(result.is_none());
}

#[test]
fn parse_porcelain_main_worktree_only() {
    let raw = "worktree /home/u/repo\nHEAD 0123\nbranch refs/heads/main\n";
    assert_eq!(
        parse_worktree_output(raw),
        vec![PathBuf::from("/home/u/repo")]
    );
}

#[test]
fn parse_porcelain_multiple_worktrees() {
    let raw = "\
worktree /home/u/repo
HEAD 0123
branch refs/heads/main

worktree /home/u/repo-feat
HEAD 4567
branch refs/heads/feat
";
    assert_eq!(
        parse_worktree_output(raw),
        vec![
            PathBuf::from("/home/u/repo"),
            PathBuf::from("/home/u/repo-feat"),
        ],
    );
}

#[test]
fn parse_porcelain_empty_input() {
    assert_eq!(parse_worktree_output(""), Vec::<PathBuf>::new());
}

#[test]
fn parse_porcelain_ignores_non_worktree_lines() {
    let raw = "worktree /repo\nHEAD abc\ndetached\n";
    assert_eq!(parse_worktree_output(raw), vec![PathBuf::from("/repo")]);
}

#[test]
fn parse_porcelain_normalizes_nfc() {
    // Decomposed (a + combining acute U+0301) → precomposed á (U+00E1).
    let raw = "worktree /repo/cafe\u{0301}\n";
    assert_eq!(
        parse_worktree_output(raw),
        vec![PathBuf::from("/repo/caf\u{00e9}")],
    );
}
