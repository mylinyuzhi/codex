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
