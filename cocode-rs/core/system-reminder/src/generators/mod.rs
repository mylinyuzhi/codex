//! System reminder generators.
//!
//! This module contains all the individual generator implementations
//! for different types of system reminders.

pub mod agent_mentions;
pub mod at_mentioned_files;
pub mod auto_memory_prompt;

pub mod available_skills;
pub mod budget_usd;
pub mod changed_files;
pub mod collab_notifications;
pub mod compact_file_reference;
pub mod compaction_reminder;
pub mod cron_reminders;
pub mod delegate_mode;
pub mod hook_response;
pub mod invoked_skills;
pub mod lsp_diagnostics;
pub mod nested_memory;
pub mod output_style;
pub mod plan_mode;
pub mod queued_commands;
pub mod relevant_memories;
pub mod rewind_reminder;
pub mod security_guidelines;
pub mod team_context;
pub mod team_mailbox;
pub mod todo_reminders;
pub mod token_usage;
pub mod unified_tasks;

// Re-export generators
pub use agent_mentions::AgentMentionsGenerator;
pub use at_mentioned_files::AtMentionedFilesGenerator;
pub use auto_memory_prompt::AutoMemoryPromptGenerator;
pub use available_skills::AvailableSkillsGenerator;
pub use budget_usd::BudgetUsdGenerator;
pub use changed_files::ChangedFilesGenerator;
pub use collab_notifications::CollabNotificationsGenerator;
pub use compact_file_reference::CompactFileReferenceGenerator;
pub use compaction_reminder::CompactionReminderGenerator;
pub use cron_reminders::CronRemindersGenerator;
pub use delegate_mode::DelegateModeGenerator;
pub use hook_response::AsyncHookResponseGenerator;
pub use hook_response::AsyncHookResponseInfo;
pub use hook_response::HookAdditionalContextGenerator;
pub use hook_response::HookBlockingErrorGenerator;
pub use hook_response::HookBlockingInfo;
pub use hook_response::HookContextInfo;
pub use invoked_skills::InvokedSkillsGenerator;
pub use lsp_diagnostics::LspDiagnosticsGenerator;
pub use nested_memory::NestedMemoryGenerator;
pub use output_style::OutputStyleGenerator;
pub use plan_mode::PlanFileReferenceGenerator;
pub use plan_mode::PlanModeEnterGenerator;
pub use plan_mode::PlanModeExitGenerator;
pub use plan_mode::PlanToolReminderGenerator;
pub use plan_mode::PlanVerificationGenerator;
pub use plan_mode::SubagentPlanReminderGenerator;
pub use queued_commands::QueuedCommandsGenerator;
pub use relevant_memories::RelevantMemoriesGenerator;
pub use rewind_reminder::RewindReminderGenerator;
pub use security_guidelines::SecurityGuidelinesGenerator;
pub use team_context::TeamContextGenerator;
pub use team_mailbox::TeamMailboxGenerator;
pub use todo_reminders::TodoRemindersGenerator;
pub use token_usage::TokenUsageGenerator;
pub use unified_tasks::UnifiedTasksGenerator;
