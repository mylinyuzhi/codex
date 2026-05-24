//! Server notifications — re-exported from `cocode-protocol`.
//!
//! The canonical definitions live in `cocode_protocol::server_notification`.
//! This module re-exports them so existing `use cocode_app_server_protocol::*`
//! import paths continue to work without changes.

pub use cocode_protocol::server_notification::notification::*;
