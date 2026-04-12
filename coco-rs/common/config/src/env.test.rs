use super::*;

#[test]
fn test_is_env_truthy_values() {
    for (val, expected) in [
        ("1", true),
        ("true", true),
        ("TRUE", true),
        ("yes", true),
        ("on", true),
        ("0", false),
        ("false", false),
        ("", false),
        ("anything", false),
    ] {
        // SAFETY: test-only, single-threaded context
        unsafe { std::env::set_var("_COCO_TEST_TRUTHY", val) };
        assert_eq!(
            is_env_truthy("_COCO_TEST_TRUTHY"),
            expected,
            "is_env_truthy({val:?})"
        );
    }
    unsafe { std::env::remove_var("_COCO_TEST_TRUTHY") };
}

#[test]
fn test_is_env_truthy_unset() {
    unsafe { std::env::remove_var("_COCO_TEST_UNSET") };
    assert!(!is_env_truthy("_COCO_TEST_UNSET"));
}

#[test]
fn test_is_env_falsy_values() {
    for (val, expected) in [
        ("0", true),
        ("false", true),
        ("FALSE", true),
        ("no", true),
        ("off", true),
        ("1", false),
        ("true", false),
    ] {
        unsafe { std::env::set_var("_COCO_TEST_FALSY", val) };
        assert_eq!(
            is_env_falsy("_COCO_TEST_FALSY"),
            expected,
            "is_env_falsy({val:?})"
        );
    }
    unsafe { std::env::remove_var("_COCO_TEST_FALSY") };
}
