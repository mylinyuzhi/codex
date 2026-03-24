//! Universal client-facing protocol for cocode.
//!
//! This crate defines the wire types shared by **all** cocode frontends:
//! SDK CLI mode (NDJSON over stdio), TUI (in-process channels), and future
//! IDE integration (WebSocket).
//!
//! Design principles:
//! - **Transport-agnostic**: usable over channels, stdio NDJSON, or WebSocket.
//! - **Self-contained**: no dependency on internal `cocode-protocol` crate so
//!   the public API stays stable even as internals evolve.
//! - **Schema-derivable**: all types derive `schemars::JsonSchema` to enable
//!   multi-language codegen (Python via `datamodel-code-generator`, TypeScript
//!   via `json-schema-to-typescript`).

mod config;
mod item;
mod notification;
mod request;
mod usage;

pub use config::*;
pub use item::*;
pub use notification::*;
pub use request::*;
pub use usage::*;

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
