//! Re-exports from `cocode-team` for backward compatibility.
//!
//! The canonical types live in `cocode_team::types`. This module provides
//! re-exports and legacy compatibility helpers used by the team tools.

pub use cocode_team::AgentMessage;
pub use cocode_team::Mailbox;
pub use cocode_team::MemberStatus;
pub use cocode_team::MessageType;
pub use cocode_team::Team;
pub use cocode_team::TeamMember;
pub use cocode_team::TeamStore;
pub use cocode_team::format_team_summary;

use std::sync::Arc;

/// Shared reference to a [`TeamStore`].
pub type TeamStoreRef = Arc<TeamStore>;

/// Shared reference to a [`Mailbox`].
pub type MailboxRef = Arc<Mailbox>;
