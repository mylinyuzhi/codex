use super::system_reminder_error::*;
use super::*;

#[test]
fn test_error_constructors() {
    let err: SystemReminderError = GeneratorFailedSnafu {
        name: "ChangedFiles",
        message: "failed to read file",
    }
    .build();
    assert!(err.to_string().contains("ChangedFiles"));
    assert!(err.to_string().contains("failed to read file"));

    let err: SystemReminderError = GeneratorTimeoutSnafu {
        name: "LspDiagnostics",
        timeout_ms: 1000_i64,
    }
    .build();
    assert!(err.to_string().contains("LspDiagnostics"));
    assert!(err.to_string().contains("1000ms"));
}

#[test]
fn test_status_codes() {
    assert_eq!(
        GeneratorFailedSnafu {
            name: "test",
            message: "test"
        }
        .build()
        .status_code(),
        StatusCode::Internal
    );

    assert_eq!(
        GeneratorTimeoutSnafu {
            name: "test",
            timeout_ms: 1000_i64
        }
        .build()
        .status_code(),
        StatusCode::Timeout
    );

    assert_eq!(
        InvalidConfigSnafu { message: "test" }.build().status_code(),
        StatusCode::InvalidConfig
    );
}