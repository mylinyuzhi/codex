use super::*;

#[test]
fn test_find_coco_home_default() {
    // When COCODE_HOME is not set, should return ~/.coco
    let home = find_coco_home();
    assert!(home.ends_with(DEFAULT_DIR));
}

#[test]
fn test_coco_home_env_constant() {
    assert_eq!(COCODE_HOME_ENV, "COCODE_HOME");
}
