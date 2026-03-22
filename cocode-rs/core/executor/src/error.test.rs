use super::executor_error::*;
use super::*;
use snafu::IntoError;

#[derive(Debug)]
struct DummyError {
    code: StatusCode,
}

impl std::fmt::Display for DummyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "dummy")
    }
}

impl std::error::Error for DummyError {}

impl cocode_error::ext::StackError for DummyError {
    fn debug_fmt(&self, layer: usize, buf: &mut Vec<String>) {
        buf.push(format!("{layer}: dummy"));
    }

    fn next(&self) -> Option<&dyn cocode_error::ext::StackError> {
        None
    }
}

impl ErrorExt for DummyError {
    fn status_code(&self) -> StatusCode {
        self.code
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn test_git_error() {
    let err: ExecutorError = GitSnafu {
        message: "failed to get HEAD",
    }
    .build();
    assert_eq!(err.status_code(), StatusCode::IoError);
    assert!(err.to_string().contains("Git operation failed"));
}

#[test]
fn test_execution_error() {
    let source = cocode_error::boxed_err(DummyError {
        code: StatusCode::Internal,
    });
    let err: ExecutorError = ExecutionSnafu.into_error(source);
    assert_eq!(err.status_code(), StatusCode::Internal);
    assert!(err.to_string().contains("Iteration execution failed"));
}

#[test]
fn test_context_error() {
    let source = cocode_error::boxed_err(DummyError {
        code: StatusCode::InvalidArguments,
    });
    let err: ExecutorError = ContextSnafu {
        message: "invalid config",
    }
    .into_error(source);
    assert_eq!(err.status_code(), StatusCode::InvalidArguments);
}

#[test]
fn test_summarization_error() {
    let err: ExecutorError = SummarizationSnafu {
        message: "LLM call failed",
    }
    .build();
    assert_eq!(err.status_code(), StatusCode::Internal);
}

#[test]
fn test_task_spawn_error() {
    let err: ExecutorError = TaskSpawnSnafu {
        message: "spawn_blocking failed",
    }
    .build();
    assert_eq!(err.status_code(), StatusCode::Internal);
}

#[test]
fn test_error_retryable() {
    // Internal errors are retryable
    let source = cocode_error::boxed_err(DummyError {
        code: StatusCode::Internal,
    });
    let exec_err: ExecutorError = ExecutionSnafu.into_error(source);
    assert!(exec_err.status_code().is_retryable());

    // IO errors are not retryable by default
    let git_err: ExecutorError = GitSnafu { message: "test" }.build();
    assert!(!git_err.status_code().is_retryable());
}
