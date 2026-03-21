use super::context_error::*;
use super::*;

#[test]
fn test_error_constructors() {
    let err: ContextError = BudgetExceededSnafu {
        message: "system prompt too large",
    }
    .build();
    assert!(err.to_string().contains("system prompt too large"));

    let err: ContextError = InvalidConfigSnafu {
        message: "negative token count",
    }
    .build();
    assert!(err.to_string().contains("negative token count"));

    let err: ContextError = BuildSnafu {
        message: "missing environment",
    }
    .build();
    assert!(err.to_string().contains("missing environment"));
}

#[test]
fn test_status_codes() {
    assert_eq!(
        BudgetExceededSnafu { message: "test" }
            .build()
            .status_code(),
        StatusCode::InvalidArguments
    );
    assert_eq!(
        InvalidConfigSnafu { message: "test" }.build().status_code(),
        StatusCode::InvalidConfig
    );
    assert_eq!(
        BuildSnafu { message: "test" }.build().status_code(),
        StatusCode::Internal
    );
}
