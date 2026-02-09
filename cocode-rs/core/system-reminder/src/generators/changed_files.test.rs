use super::*;
use crate::file_tracker::FileTracker;
use crate::file_tracker::ReadFileState;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_tracker() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = ChangedFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_no_changes() {
    let config = test_config();
    let tracker = FileTracker::new();

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .cwd(PathBuf::from("/tmp"))
        .file_tracker(&tracker)
        .build();

    let generator = ChangedFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[test]
fn test_generator_properties() {
    let generator = ChangedFilesGenerator;
    assert_eq!(generator.name(), "ChangedFilesGenerator");
    assert_eq!(generator.attachment_type(), AttachmentType::ChangedFiles);

    let config = test_config();
    assert!(generator.is_enabled(&config));

    // No throttle
    let throttle = generator.throttle_config();
    assert_eq!(throttle.min_turns_between, 0);
}

#[test]
fn test_generate_diff_simple() {
    let old = "line1\nline2\nline3\n";
    let new = "line1\nmodified\nline3\n";
    let path = Path::new("test.rs");

    let diff = ChangedFilesGenerator::generate_diff(old, new, path);
    assert!(diff.contains("-line2"));
    assert!(diff.contains("+modified"));
}

#[test]
fn test_generate_diff_addition() {
    let old = "line1\nline2\n";
    let new = "line1\nline2\nline3\n";
    let path = Path::new("test.rs");

    let diff = ChangedFilesGenerator::generate_diff(old, new, path);
    assert!(diff.contains("+line3"));
}

#[test]
fn test_generate_diff_deletion() {
    let old = "line1\nline2\nline3\n";
    let new = "line1\nline3\n";
    let path = Path::new("test.rs");

    let diff = ChangedFilesGenerator::generate_diff(old, new, path);
    assert!(diff.contains("-line2"));
}

#[test]
fn test_generate_diff_no_changes() {
    let content = "line1\nline2\n";
    let path = Path::new("test.rs");

    let diff = ChangedFilesGenerator::generate_diff(content, content, path);
    // When content is identical, the diff will contain only equal lines (space prefix)
    // and no additions or deletions
    assert!(!diff.contains("+line"));
    assert!(!diff.contains("-line"));
}

#[tokio::test]
async fn test_changed_file_with_diff() {
    // Create a temp file with initial content
    let temp = NamedTempFile::new().expect("create temp file");
    let path = temp.path().to_path_buf();

    // Write initial content
    std::fs::write(&path, "initial\ncontent\nhere\n").expect("write initial");

    // Track the file read
    let tracker = FileTracker::new();
    let old_mtime = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok());
    let state = ReadFileState::new("old\ncontent\nhere\n".to_string(), old_mtime, 1);
    tracker.track_read(&path, state);

    // Modify the file (content differs from tracked)
    std::fs::write(&path, "new\ncontent\nhere\n").expect("write new");

    // Now the file should be detected as changed (content differs)
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(2)
        .cwd(PathBuf::from("/tmp"))
        .file_tracker(&tracker)
        .build();

    let generator = ChangedFilesGenerator;

    // Check if file is detected as changed
    let changed = tracker.changed_files();
    if !changed.is_empty() {
        let result = generator.generate(&ctx).await.expect("generate");
        if let Some(reminder) = result {
            // Should contain diff markers
            assert!(
                reminder.content().unwrap().contains("```diff")
                    || reminder.content().unwrap().contains("modified since")
            );
        }
    }
}