use super::*;

// ── validate_slug ──

#[test]
fn test_validate_slug_valid() {
    assert!(validate_slug("feature-foo").is_ok());
    assert!(validate_slug("asm").is_ok());
    assert!(validate_slug("user_name.branch-1").is_ok());
    assert!(validate_slug("user/feature-foo").is_ok());
}

#[test]
fn test_validate_slug_empty() {
    assert!(validate_slug("").is_err());
}

#[test]
fn test_validate_slug_too_long() {
    let long = "a".repeat(65);
    let err = validate_slug(&long).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("64 characters"), "got: {msg}");
}

#[test]
fn test_validate_slug_path_traversal() {
    assert!(validate_slug("../escape").is_err());
    assert!(validate_slug("foo/../bar").is_err());
    assert!(validate_slug("..").is_err());
    assert!(validate_slug(".").is_err());
}

#[test]
fn test_validate_slug_invalid_chars() {
    assert!(validate_slug("feat ure").is_err()); // space
    assert!(validate_slug("feat@ure").is_err()); // @
    assert!(validate_slug("feat!ure").is_err()); // !
}

#[test]
fn test_validate_slug_leading_trailing_slash() {
    assert!(validate_slug("/foo").is_err());
    assert!(validate_slug("foo/").is_err());
}

// ── flatten_slug / worktree_branch_name ──

#[test]
fn test_flatten_slug() {
    assert_eq!(flatten_slug("user/feature"), "user+feature");
    assert_eq!(flatten_slug("simple"), "simple");
    assert_eq!(flatten_slug("a/b/c"), "a+b+c");
}

#[test]
fn test_worktree_branch_name() {
    assert_eq!(worktree_branch_name("feature"), "worktree-feature");
    assert_eq!(
        worktree_branch_name("user/feature"),
        "worktree-user+feature"
    );
}

// ── Integration tests (require git) ──

/// Create a temporary git repo and test worktree create/list/remove.
#[test]
fn test_worktree_lifecycle() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let repo = tmp.path();

    // Initialize a git repository with an initial commit
    run_git(repo, &["init"]).expect("git init");
    run_git(repo, &["config", "user.email", "test@test.com"]).expect("git config email");
    run_git(repo, &["config", "user.name", "Test"]).expect("git config name");
    std::fs::write(repo.join("README.md"), "# test").expect("write file");
    run_git(repo, &["add", "."]).expect("git add");
    run_git(repo, &["commit", "-m", "initial"]).expect("git commit");

    // Create a worktree
    let result = create_worktree(repo, "test-wt", None).expect("create worktree");
    assert!(!result.info.existed);
    assert_eq!(result.info.branch, "worktree-test-wt");
    assert!(result.info.path.exists());
    assert!(!result.info.head_commit.is_empty());

    // Creating the same worktree should resume (existed=true)
    let resume = create_worktree(repo, "test-wt", None).expect("resume worktree");
    assert!(resume.info.existed);

    // List worktrees — should include main + our worktree
    let list = list_worktrees(repo).expect("list worktrees");
    assert!(list.len() >= 2, "expected at least 2 worktrees: {list:?}");
    assert!(
        list.iter().any(|w| w.branch == "worktree-test-wt"),
        "our worktree should be in list: {list:?}"
    );

    // Check no uncommitted changes
    let changes = has_changes(&result.info.path).expect("has_changes");
    assert!(!changes);

    // Make a change and verify
    std::fs::write(result.info.path.join("new-file.txt"), "data").expect("write");
    let changes = has_changes(&result.info.path).expect("has_changes after write");
    assert!(changes);

    // Remove the worktree
    remove_worktree(repo, "test-wt").expect("remove worktree");
    assert!(!result.info.path.exists());
}

#[test]
fn test_create_worktree_invalid_slug() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let result = create_worktree(tmp.path(), "../escape", None);
    assert!(result.is_err());
}
