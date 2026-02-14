use super::*;
use std::path::PathBuf;

fn test_config() -> SystemReminderConfig {
    SystemReminderConfig::default()
}

#[tokio::test]
async fn test_no_diagnostics() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .build();

    let generator = LspDiagnosticsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_with_diagnostics() {
    let config = test_config();
    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .diagnostics(vec![
            DiagnosticInfo {
                file_path: PathBuf::from("/src/main.rs"),
                line: 10,
                column: 5,
                severity: "error".to_string(),
                message: "cannot find value `foo`".to_string(),
                code: Some("E0425".to_string()),
            },
            DiagnosticInfo {
                file_path: PathBuf::from("/src/main.rs"),
                line: 15,
                column: 1,
                severity: "warning".to_string(),
                message: "unused variable".to_string(),
                code: None,
            },
        ])
        .build();

    let generator = LspDiagnosticsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_some());

    let reminder = result.expect("reminder");
    assert!(reminder.content().unwrap().contains("main.rs"));
    assert!(reminder.content().unwrap().contains("cannot find value"));
    assert!(reminder.content().unwrap().contains("[E0425]"));
    assert!(reminder.content().unwrap().contains("unused variable"));
}

#[tokio::test]
async fn test_severity_filtering() {
    let mut config = test_config();
    config.attachments.lsp_diagnostics_min_severity = DiagnosticSeverity::Error;

    let ctx = GeneratorContext::builder()
        .config(&config)
        .turn_number(1)
        .is_main_agent(true)
        .cwd(PathBuf::from("/tmp"))
        .diagnostics(vec![DiagnosticInfo {
            file_path: PathBuf::from("/src/main.rs"),
            line: 15,
            column: 1,
            severity: "warning".to_string(), // Only warning, but filter is Error
            message: "unused variable".to_string(),
            code: None,
        }])
        .build();

    let generator = LspDiagnosticsGenerator;
    let result = generator.generate(&ctx).await.expect("generate");
    assert!(result.is_none()); // Filtered out
}

#[test]
fn test_severity_filter_logic() {
    // Error passes Error filter
    assert!(severity_passes_filter("error", DiagnosticSeverity::Error));

    // Warning doesn't pass Error filter
    assert!(!severity_passes_filter(
        "warning",
        DiagnosticSeverity::Error
    ));

    // Warning passes Warning filter
    assert!(severity_passes_filter(
        "warning",
        DiagnosticSeverity::Warning
    ));

    // Error passes Warning filter
    assert!(severity_passes_filter("error", DiagnosticSeverity::Warning));

    // Hint passes Hint filter
    assert!(severity_passes_filter("hint", DiagnosticSeverity::Hint));
}

#[test]
fn test_generator_properties() {
    let generator = LspDiagnosticsGenerator;
    assert_eq!(generator.name(), "LspDiagnosticsGenerator");
    assert_eq!(generator.tier(), ReminderTier::MainAgentOnly);
    assert_eq!(generator.attachment_type(), AttachmentType::LspDiagnostics);
}
