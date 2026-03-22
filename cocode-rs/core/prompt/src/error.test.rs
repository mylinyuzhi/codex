use super::prompt_error::*;
use super::*;

#[test]
fn test_error_constructors() {
    let err: PromptError = TemplateSnafu {
        message: "invalid placeholder",
    }
    .build();
    assert!(err.to_string().contains("invalid placeholder"));

    let err: PromptError = MissingContextSnafu { field: "platform" }.build();
    assert!(err.to_string().contains("platform"));
}

#[test]
fn test_status_codes() {
    assert_eq!(
        TemplateSnafu { message: "test" }.build().status_code(),
        StatusCode::Internal
    );
    assert_eq!(
        MissingContextSnafu { field: "test" }.build().status_code(),
        StatusCode::InvalidArguments
    );
}
