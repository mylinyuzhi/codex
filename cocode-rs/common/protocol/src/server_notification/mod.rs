//! Server notification protocol types.
//!
//! These types define the universal client-facing event protocol shared by
//! SDK, TUI, and IDE consumers. They were moved here (from `app-server-protocol`)
//! so that `CoreEvent` in `cocode-protocol` can wrap `ServerNotification`
//! without introducing upward dependencies.
//!
//! The `schemars::JsonSchema` derive is gated behind the `schema` cargo feature
//! so the 30+ crates that depend on `cocode-protocol` don't pull in `schemars`.

pub mod item;
pub mod notification;
pub mod usage;

pub use item::*;
pub use notification::*;
pub use usage::*;
