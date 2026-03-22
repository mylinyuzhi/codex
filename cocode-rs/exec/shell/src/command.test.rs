use super::*;

#[test]
fn test_command_result_success() {
    let result = CommandResult {
        exit_code: 0,
        stdout: "hello".to_string(),
        stderr: String::new(),
        duration_ms: 100,
        truncated: false,
        new_cwd: None,
        extracted_paths: None,
    };
    assert!(result.success());
}

#[test]
fn test_command_result_failure() {
    let result = CommandResult {
        exit_code: 1,
        stdout: String::new(),
        stderr: "error".to_string(),
        duration_ms: 50,
        truncated: false,
        new_cwd: None,
        extracted_paths: None,
    };
    assert!(!result.success());
}

#[test]
fn test_command_result_truncated() {
    let result = CommandResult {
        exit_code: 0,
        stdout: "partial...".to_string(),
        stderr: String::new(),
        duration_ms: 200,
        truncated: true,
        new_cwd: None,
        extracted_paths: None,
    };
    assert!(result.truncated);
    assert!(result.success());
}

#[test]
fn test_command_input_defaults() {
    let input: CommandInput = serde_json::from_str(r#"{"command":"ls"}"#).expect("parse");
    assert_eq!(input.command, "ls");
    assert!(input.timeout_ms.is_none());
    assert!(input.working_dir.is_none());
    assert!(input.description.is_none());
    assert!(!input.run_in_background);
}

#[test]
fn test_command_input_full() {
    let input = CommandInput {
        command: "cargo build".to_string(),
        timeout_ms: Some(30000),
        working_dir: Some(PathBuf::from("/tmp")),
        description: Some("Build the project".to_string()),
        run_in_background: true,
    };

    let json = serde_json::to_string(&input).expect("serialize");
    let parsed: CommandInput = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.command, "cargo build");
    assert_eq!(parsed.timeout_ms, Some(30000));
    assert_eq!(parsed.working_dir, Some(PathBuf::from("/tmp")));
    assert_eq!(parsed.description.as_deref(), Some("Build the project"));
    assert!(parsed.run_in_background);
}

#[test]
fn test_command_result_serde_roundtrip() {
    let result = CommandResult {
        exit_code: 0,
        stdout: "output".to_string(),
        stderr: "warn".to_string(),
        duration_ms: 1234,
        truncated: false,
        new_cwd: Some(PathBuf::from("/home/user")),
        extracted_paths: None,
    };

    let json = serde_json::to_string(&result).expect("serialize");
    let parsed: CommandResult = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.exit_code, result.exit_code);
    assert_eq!(parsed.stdout, result.stdout);
    assert_eq!(parsed.stderr, result.stderr);
    assert_eq!(parsed.duration_ms, result.duration_ms);
    assert_eq!(parsed.truncated, result.truncated);
    assert_eq!(parsed.new_cwd, result.new_cwd);
    assert_eq!(
        parsed.extracted_paths.is_none(),
        result.extracted_paths.is_none()
    );
}

#[test]
fn test_command_result_new_cwd_skipped_when_none() {
    let result = CommandResult {
        exit_code: 0,
        stdout: "ok".to_string(),
        stderr: String::new(),
        duration_ms: 10,
        truncated: false,
        new_cwd: None,
        extracted_paths: None,
    };

    let json = serde_json::to_string(&result).expect("serialize");
    // new_cwd should not appear in JSON when None
    assert!(!json.contains("new_cwd"));
    // extracted_paths should not appear in JSON when None
    assert!(!json.contains("extracted_paths"));
}

#[test]
fn test_extracted_paths_new() {
    let paths = vec![PathBuf::from("/file1.txt"), PathBuf::from("/file2.txt")];
    let extracted = ExtractedPaths::new(paths.clone(), 50);

    assert_eq!(extracted.paths, paths);
    assert!(extracted.extraction_attempted);
    assert_eq!(extracted.extraction_ms, 50);
    assert!(extracted.has_paths());
}

#[test]
fn test_extracted_paths_not_attempted() {
    let extracted = ExtractedPaths::not_attempted();

    assert!(extracted.paths.is_empty());
    assert!(!extracted.extraction_attempted);
    assert_eq!(extracted.extraction_ms, 0);
    assert!(!extracted.has_paths());
}

#[test]
fn test_extracted_paths_default() {
    let extracted = ExtractedPaths::default();

    assert!(extracted.paths.is_empty());
    assert!(!extracted.extraction_attempted);
    assert_eq!(extracted.extraction_ms, 0);
}

#[test]
fn test_command_result_with_extracted_paths() {
    let extracted = ExtractedPaths::new(vec![PathBuf::from("/test.rs")], 25);
    let result = CommandResult {
        exit_code: 0,
        stdout: "output".to_string(),
        stderr: String::new(),
        duration_ms: 100,
        truncated: false,
        new_cwd: None,
        extracted_paths: Some(extracted),
    };

    let json = serde_json::to_string(&result).expect("serialize");
    assert!(json.contains("extracted_paths"));
    assert!(json.contains("/test.rs"));

    let parsed: CommandResult = serde_json::from_str(&json).expect("deserialize");
    assert!(parsed.extracted_paths.is_some());
    let paths = parsed.extracted_paths.expect("extracted_paths");
    assert_eq!(paths.paths.len(), 1);
    assert!(paths.extraction_attempted);
}
