use super::executor_error::*;
use super::*;

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
    let err: ExecutorError = ExecutionSnafu {
        message: "iteration failed",
    }
    .build();
    assert_eq!(err.status_code(), StatusCode::Internal);
    assert!(err.to_string().contains("Iteration execution failed"));
}

#[test]
fn test_context_error() {
    let err: ExecutorError = ContextSnafu {
        message: "invalid config",
    }
    .build();
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
    let exec_err: ExecutorError = ExecutionSnafu { message: "test" }.build();
    assert!(exec_err.status_code().is_retryable());

    // IO errors are not retryable by default
    let git_err: ExecutorError = GitSnafu { message: "test" }.build();
    assert!(!git_err.status_code().is_retryable());
}
