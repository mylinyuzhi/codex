//! System reminder generators.
//!
//! This module contains all the individual generator implementations
//! for different types of system reminders.

pub mod changed_files;
pub mod lsp_diagnostics;
pub mod nested_memory;
pub mod plan_mode;
pub mod todo_reminders;

// Re-export generators
pub use changed_files::ChangedFilesGenerator;
pub use lsp_diagnostics::LspDiagnosticsGenerator;
pub use nested_memory::NestedMemoryGenerator;
pub use plan_mode::{PlanModeApprovedGenerator, PlanModeEnterGenerator, PlanToolReminderGenerator};
pub use todo_reminders::TodoRemindersGenerator;
