//! Test-only helpers exposed across the integration-test seam.
//!
//! Integration tests in `tests/` only see the crate's public API, so
//! crate-internal pipeline pieces (the queue → history drain, the
//! per-item attachment conversion) need a thin `pub` re-export to be
//! reachable for end-to-end coverage. Production code never goes
//! through this module — direct callers route through `engine` /
//! `engine_finalize_turn` which still use the `pub(crate)` paths.
//!
//! Keep this module minimal: it's an integration-test seam, not a
//! second public API.

pub use crate::helpers::drain_command_queue_into_history as drain_into_history;
pub use crate::helpers::queued_command_to_attachment;
