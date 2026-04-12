//! IDE bridge (VS Code, JetBrains) and REPL bridge for SDK/daemon callers.
//!
//! Provides message types and a server skeleton for communication
//! between IDE extensions and the coco agent, plus a REPL bridge
//! for headless/non-TUI communication with SDK consumers.

pub mod protocol;
pub mod repl;
pub mod server;

pub use protocol::BridgeInMessage;
pub use protocol::BridgeOutMessage;
pub use protocol::BridgeTransport;
pub use repl::BridgeState;
pub use repl::ReplBridge;
pub use repl::ReplInMessage;
pub use repl::ReplOutMessage;
pub use server::BridgeServer;
