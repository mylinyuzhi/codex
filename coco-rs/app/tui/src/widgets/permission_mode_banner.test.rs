use coco_types::PermissionMode;

use super::PermissionModeBanner;

#[test]
fn should_display_only_for_non_default_modes() {
    assert!(!PermissionModeBanner::should_display(
        PermissionMode::Default
    ));
    assert!(PermissionModeBanner::should_display(
        PermissionMode::AcceptEdits
    ));
    assert!(PermissionModeBanner::should_display(PermissionMode::Plan));
    assert!(PermissionModeBanner::should_display(
        PermissionMode::BypassPermissions
    ));
}
