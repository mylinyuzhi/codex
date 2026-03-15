use super::*;
use std::process::Command as StdCommand;
use tempfile::TempDir;

#[tokio::test]
async fn test_get_uncommitted_changes_empty() {
    let temp = TempDir::new().unwrap();
    StdCommand::new("git")
        .args(["init"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    let changes = get_uncommitted_changes(temp.path()).await.unwrap();
    assert!(changes.is_empty());
}

#[tokio::test]
async fn test_get_uncommitted_changes_with_file() {
    let temp = TempDir::new().unwrap();
    StdCommand::new("git")
        .args(["init"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    // Create a file
    std::fs::write(temp.path().join("test.txt"), "content").unwrap();

    let changes = get_uncommitted_changes(temp.path()).await.unwrap();
    assert_eq!(changes.len(), 1);
    assert!(changes[0].contains("test.txt"));
}

#[tokio::test]
async fn test_get_head_commit() {
    let temp = TempDir::new().unwrap();
    StdCommand::new("git")
        .args(["init"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    // Configure git user for commit
    StdCommand::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    // Create initial commit
    std::fs::write(temp.path().join("test.txt"), "content").unwrap();
    StdCommand::new("git")
        .args(["add", "-A"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    let commit = get_head_commit(temp.path()).await.unwrap();
    assert!(!commit.is_empty());
    assert!(commit.len() >= 7); // Git SHA is at least 7 chars
}

#[test]
fn test_read_plan_file_no_dir() {
    let temp = TempDir::new().unwrap();
    let result = read_plan_file_if_exists(temp.path());
    assert!(result.is_none());
}

#[test]
fn test_read_plan_file_with_plan() {
    let temp = TempDir::new().unwrap();
    let plans_dir = temp.path().join(".cocode").join("plans");
    std::fs::create_dir_all(&plans_dir).unwrap();
    std::fs::write(plans_dir.join("test-plan.md"), "# Plan\n1. Do X").unwrap();

    let result = read_plan_file_if_exists(temp.path());
    assert!(result.is_some());
    assert!(result.unwrap().contains("# Plan"));
}

#[tokio::test]
async fn test_commit_if_needed_no_changes() {
    let temp = TempDir::new().unwrap();
    StdCommand::new("git")
        .args(["init"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    let result = commit_if_needed(temp.path(), "test commit").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_commit_if_needed_with_changes() {
    let temp = TempDir::new().unwrap();
    StdCommand::new("git")
        .args(["init"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    // Configure git user for commit
    StdCommand::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    StdCommand::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(temp.path())
        .output()
        .unwrap();

    // Create a file
    std::fs::write(temp.path().join("test.txt"), "content").unwrap();

    let result = commit_if_needed(temp.path(), "[iter-0] Test commit")
        .await
        .unwrap();
    assert!(result.is_some());
    let commit_id = result.unwrap();
    assert!(!commit_id.is_empty());
}
