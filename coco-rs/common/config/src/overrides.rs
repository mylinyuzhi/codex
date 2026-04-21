use coco_types::PermissionMode;

/// Mutable state that changes during a session.
/// NOT persisted to config files. Lost when session ends.
///
/// Thinking-level and fast-mode are **not** here because they have
/// their own state holders — `ThinkingLevel` lives per-query on
/// `QueryParams`, and fast-mode runs off the process-wide state in
/// `coco_config::fast_mode`. This struct is only for layered config
/// resolution (the kind that flows into `RuntimeConfig` at build time).
#[derive(Debug, Clone, Default)]
pub struct RuntimeOverrides {
    /// /model command override — slash-qualified `provider/model_id`
    /// routed through `RuntimeConfigBuilder::with_overrides`.
    pub model_override: Option<String>,
    /// --permission-mode CLI override; wins over `settings.permissions.default_mode`.
    pub permission_mode_override: Option<PermissionMode>,
}
