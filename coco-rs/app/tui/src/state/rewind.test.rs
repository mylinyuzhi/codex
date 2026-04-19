use super::*;

#[test]
fn test_build_restore_options_with_file_changes() {
    let opts = build_restore_options(
        /*file_history_enabled*/ true, /*has_file_changes*/ true,
    );
    assert_eq!(
        opts,
        vec![
            RestoreType::Both,
            RestoreType::ConversationOnly,
            RestoreType::CodeOnly,
        ]
    );
}

#[test]
fn test_build_restore_options_no_file_changes() {
    let opts = build_restore_options(
        /*file_history_enabled*/ true, /*has_file_changes*/ false,
    );
    assert_eq!(opts, vec![RestoreType::ConversationOnly]);
}

#[test]
fn test_build_restore_options_file_history_disabled() {
    let opts = build_restore_options(
        /*file_history_enabled*/ false, /*has_file_changes*/ false,
    );
    assert_eq!(opts, vec![RestoreType::ConversationOnly]);
}
