//! Logger module for warning logging.
//!
//! This module provides warning logging functionality matching the TypeScript
//! `@ai-sdk/ai` logger pattern.

mod log_warnings;

pub use log_warnings::FIRST_WARNING_INFO_MESSAGE;
pub use log_warnings::LogWarningsFunction;
pub use log_warnings::LogWarningsOptions;
pub use log_warnings::log_warnings;
pub use log_warnings::reset_log_warnings_state;
pub use log_warnings::set_log_warnings;
