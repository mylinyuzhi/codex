use super::*;

#[test]
fn test_build_restore_options_with_file_changes() {
    let opts = build_restore_options(
        /*file_history_enabled*/ true, /*has_file_changes*/ true,
    );
    assert_eq!(opts.len(), 3);
    assert_eq!(opts[0].restore_type, RestoreType::Both);
    assert_eq!(opts[1].restore_type, RestoreType::ConversationOnly);
    assert_eq!(opts[2].restore_type, RestoreType::CodeOnly);
}

#[test]
fn test_build_restore_options_no_file_changes() {
    let opts = build_restore_options(
        /*file_history_enabled*/ true, /*has_file_changes*/ false,
    );
    assert_eq!(opts.len(), 1);
    assert_eq!(opts[0].restore_type, RestoreType::ConversationOnly);
}

#[test]
fn test_build_restore_options_file_history_disabled() {
    let opts = build_restore_options(
        /*file_history_enabled*/ false, /*has_file_changes*/ false,
    );
    assert_eq!(opts.len(), 1);
    assert_eq!(opts[0].restore_type, RestoreType::ConversationOnly);
}
