use super::*;

#[tokio::test]
async fn memory_dialog_lists_entries_in_ts_order() {
    let h = MemoryDialogHandler::new(
        PathBuf::from("/proj"),
        PathBuf::from("/home/u"),
        Some(PathBuf::from("/managed")),
    );
    let entries = h.entries();
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].scope, MemoryScope::Managed);
    assert_eq!(entries[1].scope, MemoryScope::User);
    assert_eq!(entries[2].scope, MemoryScope::Project);
    assert_eq!(entries[3].scope, MemoryScope::ProjectLocal);
    assert_eq!(entries[2].path, PathBuf::from("/proj/CLAUDE.md"));
    assert_eq!(entries[3].path, PathBuf::from("/proj/CLAUDE.local.md"));
}

#[tokio::test]
async fn execute_emits_open_dialog() {
    let h = MemoryDialogHandler::new(PathBuf::from("/p"), PathBuf::from("/h"), None);
    match h.execute_command("").await.unwrap() {
        CommandResult::OpenDialog(DialogSpec::MemoryFileSelector { entries }) => {
            assert!(!entries.is_empty());
        }
        other => panic!("unexpected: {other:?}"),
    }
}
