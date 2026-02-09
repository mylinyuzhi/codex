use super::tool_error::*;
use super::*;

#[test]
fn test_error_constructors() {
    let err: ToolError = NotFoundSnafu { name: "test_tool" }.build();
    assert!(err.to_string().contains("test_tool"));

    let err: ToolError = InvalidInputSnafu {
        message: "bad json",
    }
    .build();
    assert!(err.to_string().contains("bad json"));

    let err: ToolError = TimeoutSnafu {
        timeout_secs: 30i64,
    }
    .build();
    assert!(err.to_string().contains("30"));
}

#[test]
fn test_is_retriable() {
    assert!(
        TimeoutSnafu {
            timeout_secs: 30i64
        }
        .build()
        .is_retriable()
    );
    assert!(
        IoSnafu {
            message: "network error"
        }
        .build()
        .is_retriable()
    );
    assert!(!NotFoundSnafu { name: "test" }.build().is_retriable());
    assert!(
        !PermissionDeniedSnafu { message: "denied" }
            .build()
            .is_retriable()
    );
}

#[test]
fn test_status_codes() {
    assert_eq!(
        NotFoundSnafu { name: "test" }.build().status_code(),
        StatusCode::InvalidArguments
    );
    assert_eq!(
        PermissionDeniedSnafu { message: "test" }
            .build()
            .status_code(),
        StatusCode::PermissionDenied
    );
    assert_eq!(
        TimeoutSnafu {
            timeout_secs: 30i64
        }
        .build()
        .status_code(),
        StatusCode::Timeout
    );
}

#[test]
fn test_from_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let tool_err: ToolError = io_err.into();
    assert!(matches!(tool_err, ToolError::Io { .. }));
}
