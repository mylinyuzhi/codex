use coco_types::PermissionMode;
use coco_types::ThinkingLevel;

/// Mutable state that changes during a session.
/// NOT persisted to config files. Lost when session ends.
#[derive(Debug, Clone, Default)]
pub struct RuntimeOverrides {
    /// /model command override.
    pub model_override: Option<String>,
    /// /effort or /think command override.
    pub thinking_level_override: Option<ThinkingLevel>,
    /// /fast command override.
    pub fast_mode_override: Option<bool>,
    /// Plan mode toggle.
    pub permission_mode_override: Option<PermissionMode>,
}
