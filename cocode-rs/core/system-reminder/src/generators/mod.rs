//! System reminder generators.
//!
//! This module contains all the individual generator implementations
//! for different types of system reminders.

pub mod available_skills;
pub mod changed_files;
pub mod collab_notifications;
pub mod delegate_mode;
pub mod hook_response;
pub mod lsp_diagnostics;
pub mod nested_memory;
pub mod plan_mode;
pub mod plan_mode_exit;
pub mod plan_verification;
pub mod todo_reminders;
pub mod token_usage;
pub mod unified_tasks;

// Re-export generators
pub use available_skills::{AVAILABLE_SKILLS_KEY, AvailableSkillsGenerator, SkillInfo};
pub use changed_files::ChangedFilesGenerator;
pub use collab_notifications::CollabNotificationsGenerator;
pub use delegate_mode::DelegateModeGenerator;
pub use hook_response::{
    ASYNC_HOOK_RESPONSES_KEY, AsyncHookResponseGenerator, AsyncHookResponseInfo, HOOK_BLOCKING_KEY,
    HOOK_CONTEXT_KEY, HookAdditionalContextGenerator, HookBlockingErrorGenerator, HookBlockingInfo,
    HookContextInfo,
};
pub use lsp_diagnostics::LspDiagnosticsGenerator;
pub use nested_memory::NestedMemoryGenerator;
pub use plan_mode::{PlanModeApprovedGenerator, PlanModeEnterGenerator, PlanToolReminderGenerator};
pub use plan_mode_exit::PlanModeExitGenerator;
pub use plan_verification::PlanVerificationGenerator;
pub use todo_reminders::TodoRemindersGenerator;
pub use token_usage::TokenUsageGenerator;
pub use unified_tasks::{UNIFIED_TASKS_KEY, UnifiedTasksGenerator};
