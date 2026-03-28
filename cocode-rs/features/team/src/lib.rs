//! Agent team system for cocode-rs.
//!
//! This crate provides the core team orchestration layer for multi-agent
//! collaboration, aligned with Claude Code's agent team system. It includes:
//!
//! - **Types**: Team, TeamMember, AgentMessage, MessageType, MemberStatus
//! - **Store**: Dual-layer persistence (in-memory cache + filesystem)
//! - **Mailbox**: JSONL-based inter-agent messaging with atomic writes
//! - **Shutdown**: Graceful shutdown protocol tracking
//! - **Config**: Configurable team settings (not hardcoded)
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────┐
//! │                       cocode-team                         │
//! ├──────────────────────────────────────────────────────────┤
//! │  types     │  store     │  mailbox   │  shutdown │ config│
//! │  Team      │  TeamStore │  Mailbox   │  Shutdown │ Team  │
//! │  Member    │  (memory + │  (JSONL +  │  Tracker  │ Config│
//! │  Message   │   fs)      │   atomic)  │           │       │
//! └──────────────────────────────────────────────────────────┘
//! ```

pub mod config;
pub mod error;
pub mod mailbox;
pub mod shutdown;
pub mod store;
pub mod types;

// Re-export primary types for convenience.
pub use config::TeamConfig;
pub use error::Result;
pub use error::TeamError;
pub use mailbox::Mailbox;
pub use shutdown::ShutdownTracker;
pub use store::TeamStore;
pub use types::AgentMessage;
pub use types::MemberStatus;
pub use types::MessageType;
pub use types::SandboxPermissionRequest;
pub use types::SandboxPermissionResponse;
pub use types::SandboxRestrictionKind;
pub use types::Team;
pub use types::TeamMember;
pub use types::format_team_summary;
