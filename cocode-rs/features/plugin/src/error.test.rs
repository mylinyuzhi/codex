use super::*;

#[test]
fn test_error_display() {
    let err = plugin_error::ManifestNotFoundSnafu {
        path: PathBuf::from("/path/to/plugin"),
    }
    .build();
    assert!(err.to_string().contains("/path/to/plugin"));

    let err = plugin_error::InvalidManifestSnafu {
        path: PathBuf::from("/plugin"),
        message: "missing name".to_string(),
    }
    .build();
    assert!(err.to_string().contains("missing name"));
}

#[test]
fn test_error_status_codes() {
    let err = plugin_error::ManifestNotFoundSnafu {
        path: PathBuf::from("/test"),
    }
    .build();
    assert_eq!(err.status_code(), StatusCode::FileNotFound);

    let err = plugin_error::PathTraversalSnafu {
        path: PathBuf::from("../../../etc/passwd"),
    }
    .build();
    assert_eq!(err.status_code(), StatusCode::PermissionDenied);
}
