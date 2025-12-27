//! Attachment generators for system reminders.
//!
//! Each generator produces a specific type of system reminder.

mod agent_task;
mod changed_files;
mod critical_instruction;
mod lsp_diagnostics;
mod plan_mode;
mod plan_tool_reminder;
mod shell_task;

pub use agent_task::AgentTaskGenerator;
pub use changed_files::ChangedFilesGenerator;
pub use critical_instruction::CriticalInstructionGenerator;
pub use lsp_diagnostics::LspDiagnosticsGenerator;
pub use plan_mode::PlanModeGenerator;
pub use plan_tool_reminder::PlanToolReminderGenerator;
pub use shell_task::ShellTaskGenerator;
