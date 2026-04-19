use super::HookStatusPanel;
use crate::state::session::HookEntry;
use crate::state::session::HookEntryStatus;

#[test]
fn should_display_gates_on_non_empty() {
    let empty: [HookEntry; 0] = [];
    assert!(!HookStatusPanel::should_display(&empty));

    let filled = [HookEntry {
        hook_id: "h1".into(),
        hook_name: "pre-commit".into(),
        status: HookEntryStatus::Running,
        output: None,
    }];
    assert!(HookStatusPanel::should_display(&filled));
}
