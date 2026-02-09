use super::*;

#[test]
fn test_error_display() {
    let err = plan_mode_error::NoHomeDirSnafu.build();
    assert!(err.to_string().contains("home directory"));
}
