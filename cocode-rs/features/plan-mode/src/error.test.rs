use super::*;

#[test]
fn test_error_display() {
    let err = plan_mode_error::SlugCollisionSnafu { max_retries: 5 }.build();
    assert!(err.to_string().contains("slug"));
}
