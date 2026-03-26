//! Re-exports `EventMapper` from the shared `cocode-app-server` crate.
//!
//! The canonical implementation lives in `cocode_app_server::event_mapper`.
//! This module re-exports it so existing imports in `sdk/mod.rs` continue
//! to work without code changes.

pub use cocode_app_server::event_mapper::EventMapper;
