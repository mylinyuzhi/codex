use super::*;

#[test]
fn test_find_cocode_home_default() {
    // When COCODE_HOME is not set, should return ~/.cocode
    let home = find_cocode_home();
    assert!(home.ends_with(DEFAULT_DIR));
}

#[test]
fn test_cocode_home_env_constant() {
    assert_eq!(COCODE_HOME_ENV, "COCODE_HOME");
}
