//! Common error utilities for cocode-rs.

pub mod ext;
pub mod status_code;

// Re-export snafu and snafu-virtstack for convenience
pub use snafu;
pub use snafu::Location;
pub use snafu_virtstack::VirtualStackTrace;
pub use snafu_virtstack::stack_trace_debug;

pub use ext::BoxedError;
pub use ext::BoxedErrorSource;
pub use ext::ErrorExt;
pub use ext::PlainError;
pub use ext::boxed;
pub use ext::boxed_err;
pub use status_code::StatusCode;
