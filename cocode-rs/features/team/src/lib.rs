//! Agent team system for cocode-rs.
//!
//! This crate provides the core team orchestration layer for multi-agent
//! collaboration, aligned with Claude Code's agent team system. It includes:
//!
//! - **Types**: Team, TeamMember, AgentMessage, MessageType, MemberStatus
//! - **Store**: Dual-layer persistence (in-memory cache + filesystem)
//! - **Mailbox**: JSONL-based inter-agent messaging with atomic writes
//! - **FastPath**: In-process channel-based message delivery (<1ms)
//! - **TaskLedger**: Shared task list with atomic claiming and dependencies
//! - **Polling**: Priority-based polling loop for teammate agents
//! - **Delegate**: Coordination-only mode for team leads
//! - **Shutdown**: Graceful shutdown protocol tracking
//! - **Config**: Configurable team settings (not hardcoded)
//!
//! # Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │                          cocode-team                                 │
//! ├──────────────────────────────────────────────────────────────────────┤
//! │  types      │  store     │  mailbox   │  fast_path  │  task_ledger  │
//! │  Team       │  TeamStore │  Mailbox   │  FastPath   │  TaskLedger   │
//! │  Member     │  (memory + │  (JSONL +  │  (mpsc      │  (claiming +  │
//! │  Message    │   fs)      │   atomic)  │   channels) │   deps)       │
//! ├─────────────┼────────────┼────────────┼─────────────┼───────────────┤
//! │  polling    │  delegate  │  shutdown  │  config     │  error        │
//! │  TeamPoller │  Delegate  │  Shutdown  │  TeamConfig │  TeamError    │
//! │  (priority  │  Mode      │  Tracker   │             │               │
//! │   loop)     │  (coord)   │            │             │               │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```

pub mod config;
pub mod delegate;
pub mod error;
pub mod fast_path;
pub mod mailbox;
pub mod polling;
pub mod shutdown;
pub mod store;
pub mod task_ledger;
pub mod types;

// Re-export primary types for convenience.
pub use config::TeamConfig;
pub use delegate::DELEGATE_MODE_TOOLS;
pub use delegate::DelegateModeState;
pub use delegate::filter_for_delegate_mode;
pub use delegate::is_delegate_tool;
pub use error::Result;
pub use error::TeamError;
pub use fast_path::FastPath;
pub use mailbox::Mailbox;
pub use polling::PollConfig;
pub use polling::PollResult;
pub use polling::TeamPoller;
pub use shutdown::ShutdownTracker;
pub use store::TeamStore;
pub use task_ledger::ClaimResult;
pub use task_ledger::TaskLedger;
pub use task_ledger::TeamTask;
pub use task_ledger::TeamTaskStatus;
pub use types::AgentMessage;
pub use types::MemberStatus;
pub use types::MessageType;
pub use types::SandboxPermissionRequest;
pub use types::SandboxPermissionResponse;
pub use types::SandboxRestrictionKind;
pub use types::Team;
pub use types::TeamMember;
pub use types::format_team_summary;
