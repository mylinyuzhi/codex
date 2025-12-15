//! Attachment generators for system reminders.
//!
//! Each generator produces a specific type of system reminder.

mod background_task;
mod changed_files;
mod critical_instruction;
mod plan_mode;
mod todo_reminder;

pub use background_task::BackgroundTaskGenerator;
pub use changed_files::ChangedFilesGenerator;
pub use critical_instruction::CriticalInstructionGenerator;
pub use plan_mode::PlanModeGenerator;
pub use todo_reminder::TodoReminderGenerator;
