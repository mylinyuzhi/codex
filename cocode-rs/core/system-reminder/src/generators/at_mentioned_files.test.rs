use super::*;
use std::io::Write;
use tempfile::TempDir;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_mentions() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .has_user_input(true)
        .user_prompt("Hello, how are you?")
        .cwd(std::path::PathBuf::from("/tmp"))
        .build();

    let generator = AtMentionedFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_file_mention() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let file_path = temp_dir.path().join("test.txt");
    {
        let mut file = fs::File::create(&file_path).expect("create file");
        writeln!(file, "line 1").expect("write");
        writeln!(file, "line 2").expect("write");
        writeln!(file, "line 3").expect("write");
    }

    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .has_user_input(true)
        .user_prompt("Check @test.txt please")
        .cwd(temp_dir.path().to_path_buf())
        .build();

    let generator = AtMentionedFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("Read tool"));
    assert!(reminder.content().unwrap().contains("line 1"));
}

#[tokio::test]
async fn test_file_with_line_range() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let file_path = temp_dir.path().join("test.txt");
    {
        let mut file = fs::File::create(&file_path).expect("create file");
        for i in 1..=10 {
            writeln!(file, "line {i}").expect("write");
        }
    }

    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .has_user_input(true)
        .user_prompt("Check @test.txt:3-5 please")
        .cwd(temp_dir.path().to_path_buf())
        .build();

    let generator = AtMentionedFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("line 3"));
    assert!(reminder.content().unwrap().contains("line 4"));
    assert!(reminder.content().unwrap().contains("line 5"));
    assert!(!reminder.content().unwrap().contains("line 6"));
}

#[tokio::test]
async fn test_file_with_line_start_to_eof() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let file_path = temp_dir.path().join("test.txt");
    {
        let mut file = fs::File::create(&file_path).expect("create file");
        for i in 1..=10 {
            writeln!(file, "line {i}").expect("write");
        }
    }

    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .has_user_input(true)
        .user_prompt("Check @test.txt:8 please")
        .cwd(temp_dir.path().to_path_buf())
        .build();

    let generator = AtMentionedFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    let content = reminder.content().unwrap();
    // Should include lines 8-10 (from line 8 to EOF)
    assert!(content.contains("line 8"));
    assert!(content.contains("line 9"));
    assert!(content.contains("line 10"));
    // Should NOT include lines before 8
    assert!(!content.contains("line 7"));
}

#[test]
fn test_escape_json_string() {
    assert_eq!(escape_json_string("hello"), "hello");
    assert_eq!(escape_json_string("hello\nworld"), "hello\\nworld");
    assert_eq!(escape_json_string("say \"hi\""), "say \\\"hi\\\"");
}

#[test]
fn test_generator_properties() {
    let generator = AtMentionedFilesGenerator;
    assert_eq!(generator.name(), "AtMentionedFilesGenerator");
    assert_eq!(generator.tier(), ReminderTier::UserPrompt);
    assert_eq!(
        generator.attachment_type(),
        AttachmentType::AtMentionedFiles
    );
}

#[tokio::test]
async fn test_file_with_line_range_still_reads_when_tracked_unchanged() {
    // When a file has a line range specified, it should be re-read even if
    // the full file was previously tracked as unchanged. This is because
    // line ranges represent different "views" of the file.
    let temp_dir = TempDir::new().expect("create temp dir");
    let file_path = temp_dir.path().join("test.txt");
    {
        let mut file = fs::File::create(&file_path).expect("create file");
        for i in 1..=6 {
            writeln!(file, "line {i}").expect("write");
        }
    }

    // Track the file as already read (full content)
    let mtime = fs::metadata(&file_path)
        .ok()
        .and_then(|m| m.modified().ok());
    let tracker = crate::FileTracker::new();
    tracker.track_read(
        &file_path,
        crate::FileReadState::complete_with_turn(
            fs::read_to_string(&file_path).expect("read file"),
            mtime,
            1,
        ),
    );

    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(2)
        .is_main_agent(true)
        .has_user_input(true)
        .user_prompt("Check @test.txt:3-4 please")
        .cwd(temp_dir.path().to_path_buf())
        .file_tracker(&tracker)
        .build();

    let generator = AtMentionedFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");

    // Should return content, not already-read
    assert!(result.is_some());
    let reminder = result.expect("reminder");
    let content = reminder.content().expect("content");
    assert!(content.contains("line 3"));
    assert!(content.contains("line 4"));
    assert!(!content.contains("line 1"));
}

#[tokio::test]
async fn test_duplicate_mentions_same_normalized_path_are_deduped() {
    // When the same file is mentioned multiple times via different paths
    // (e.g., @dup.txt and @./dup.txt), it should only be processed once.
    let temp_dir = TempDir::new().expect("create temp dir");
    let file_path = temp_dir.path().join("dup.txt");
    fs::write(&file_path, "line 1\n").expect("write");

    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .has_user_input(true)
        .user_prompt("Check @dup.txt and @./dup.txt")
        .cwd(temp_dir.path().to_path_buf())
        .build();

    let generator = AtMentionedFilesGenerator;
    let reminder = generator
        .generate(&ctx)
        .await
        .expect("generate")
        .expect("reminder");

    // Should only appear once in the content (deduplicated)
    let content = reminder.content().expect("content");
    // The file path appears once in the input, once in the result
    // Count the number of "dup.txt" occurrences in "file_path" sections
    let file_path_occurrences = content.matches("\"file_path\"").count();
    assert_eq!(file_path_occurrences, 1, "Should only read the file once");
    // Content should only appear once
    assert_eq!(
        content.matches("line 1").count(),
        1,
        "Content should appear once"
    );
}

#[tokio::test]
async fn test_already_read_file_returns_silent_reminder() {
    // When a file was already read and is unchanged, it should return
    // a silent AlreadyReadFile reminder (zero tokens).
    let temp_dir = TempDir::new().expect("create temp dir");
    let file_path = temp_dir.path().join("cached.txt");
    fs::write(&file_path, "cached content\n").expect("write");

    // Track the file as already read
    let mtime = fs::metadata(&file_path)
        .ok()
        .and_then(|m| m.modified().ok());
    let tracker = crate::FileTracker::new();
    tracker.track_read(
        &file_path,
        crate::FileReadState::complete_with_turn(
            fs::read_to_string(&file_path).expect("read file"),
            mtime,
            1,
        ),
    );

    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(2)
        .is_main_agent(true)
        .has_user_input(true)
        .user_prompt("Check @cached.txt")
        .cwd(temp_dir.path().to_path_buf())
        .file_tracker(&tracker)
        .build();

    let generator = AtMentionedFilesGenerator;
    let result = generator.generate(&ctx).await.expect("generate");

    // Should return a silent already-read reminder
    assert!(result.is_some());
    let reminder = result.expect("reminder");
    // AlreadyReadFile type should be silent (no content)
    assert_eq!(reminder.attachment_type, AttachmentType::AlreadyReadFile);
}
