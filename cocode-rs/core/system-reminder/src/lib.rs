//! cocode-system-reminder - Dynamic context injection for agent conversations.
//!
//! This crate provides the system reminder infrastructure for injecting dynamic
//! contextual metadata into agent conversations. It mirrors Claude Code's
//! attachment system with XML-tagged `<system-reminder>` messages.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                    cocode-system-reminder                           │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  Orchestrator          │  Generators           │  Types            │
//! │  - parallel execution  │  - ChangedFiles       │  - AttachmentType │
//! │  - timeout protection  │  - PlanMode*          │  - ReminderTier   │
//! │  - tier filtering      │  - TodoReminders      │  - XmlTag         │
//! │  - throttle management │  - LspDiagnostics     │  - SystemReminder │
//! │                        │  - NestedMemory       │                   │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # System Prompt vs System Reminder
//!
//! | System | Type | When | What | Where |
//! |--------|------|------|------|-------|
//! | core/prompt | Static | Build time | System prompt template | Main system message |
//! | system-reminder | Dynamic | Per-turn | Contextual metadata | Conversation history |
//!
//! They are complementary:
//! - `core/prompt` builds the **static base prompt** (identity, tool policy, etc.)
//! - `system-reminder` injects **dynamic context** (file changes, plan mode, diagnostics)
//!
//! # Quick Start
//!
//! ```ignore
//! use cocode_system_reminder::{
//!     SystemReminderOrchestrator, SystemReminderConfig, GeneratorContext,
//! };
//!
//! // Create orchestrator with default config
//! let config = SystemReminderConfig::default();
//! let orchestrator = SystemReminderOrchestrator::new(config);
//!
//! // Build context for this turn
//! let ctx = GeneratorContext::builder()
//!     .turn_number(5)
//!     .is_main_agent(true)
//!     .has_user_input(true)
//!     .build();
//!
//! // Generate all applicable reminders
//! let reminders = orchestrator.generate_all(&ctx).await;
//!
//! // Inject into message history
//! inject_reminders(reminders, &mut messages, turn_id);
//! ```

pub mod config;
pub mod error;
pub mod file_context_resolver;
pub mod file_read_tracking_policy;
pub mod generator;
pub mod generators;
pub mod history_file_read_state;
pub mod inject;
pub mod orchestrator;
pub mod parsing;
pub mod throttle;
pub mod types;
pub mod xml;

// Re-export main types at crate root
pub use config::SystemReminderConfig;
pub use error::Result;
pub use error::SystemReminderError;
// Re-export file context resolver types
pub use file_context_resolver::FileReadConfig;
pub use file_context_resolver::MentionResolution;
pub use file_context_resolver::ReadFileResult;
pub use file_context_resolver::ResolvedFile;
pub use file_context_resolver::is_cacheable_file;
pub use file_context_resolver::read_file_with_limits;
pub use file_context_resolver::resolve_mentions;
// Re-export file read tracking policy
pub use file_read_tracking_policy::MentionReadDecision;
pub use file_read_tracking_policy::categorize_read_kind;
pub use file_read_tracking_policy::is_cacheable_read;
pub use file_read_tracking_policy::is_full_content_read_tool;
pub use file_read_tracking_policy::is_read_state_source_tool;
pub use file_read_tracking_policy::is_stronger_kind;
pub use file_read_tracking_policy::resolve_mention_read_decision;
pub use file_read_tracking_policy::should_skip_tracked_file;
// Re-export history file read state
pub use history_file_read_state::BUILD_STATE_DEFAULT_MAX_ENTRIES;
pub use history_file_read_state::FileReadStateEntry;
pub use history_file_read_state::build_file_read_state_from_modifiers;
pub use history_file_read_state::build_file_read_state_from_turns;
pub use history_file_read_state::build_read_state_from_modifier;
pub use history_file_read_state::file_read_infos_to_states;
pub use history_file_read_state::merge_file_read_state;
// Type aliases for API compatibility with reference branch
pub use history_file_read_state::ReadFileState;
pub use history_file_read_state::ReadStateKind;
// Re-export FileTracker from cocode-tools (unified file tracking)
pub use cocode_tools::FileReadState;
pub use cocode_tools::FileTracker;
pub use generator::ApprovedPlanInfo;
pub use generator::AsyncHookResponseInfo;
pub use generator::AttachmentGenerator;
pub use generator::BackgroundTaskInfo;
pub use generator::BackgroundTaskStatus;
pub use generator::BackgroundTaskType;
pub use generator::CompactedLargeFile;
pub use generator::CronJobInfo;
pub use generator::GeneratorContext;
pub use generator::GeneratorContextBuilder;
pub use generator::HookBlockingInfo;
pub use generator::HookContextInfo;
pub use generator::HookState;
pub use generator::InvokedSkillInfo;
pub use generator::MentionReadRecord;
pub use generator::QueuedCommandInfo;
pub use generator::RestoredPlanInfo;
pub use generator::RewindContextInfo;
pub use generator::SkillInfo;
pub use generator::StructuredTaskInfo;
pub use inject::InjectedBlock;
pub use inject::InjectedMessage;
pub use inject::NormalizedMessages;
pub use inject::combine_reminders;
pub use inject::create_injected_messages;
pub use inject::inject_reminders;
pub use inject::normalize_injected_messages;
pub use orchestrator::SystemReminderOrchestrator;
pub use throttle::ThrottleConfig;
pub use throttle::ThrottleManager;
pub use types::AttachmentType;
pub use types::ContentBlock;
pub use types::FileReadInfo;
pub use types::FileReadKind;
pub use types::MessageRole;
pub use types::ReminderMessage;
pub use types::ReminderOutput;
pub use types::ReminderTier;
pub use types::SystemReminder;
pub use types::XmlTag;
pub use xml::extract_system_reminder;
pub use xml::wrap_system_reminder;
pub use xml::wrap_with_tag;

// Parsing utilities
pub use parsing::AgentMention;
pub use parsing::FileMention;
pub use parsing::ParsedMentions;
pub use parsing::parse_agent_mentions;
pub use parsing::parse_file_mentions;
pub use parsing::parse_mentions;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::config::SystemReminderConfig;
    pub use crate::generator::AttachmentGenerator;
    pub use crate::generator::GeneratorContext;
    pub use crate::inject::InjectedBlock;
    pub use crate::inject::InjectedMessage;
    pub use crate::inject::create_injected_messages;
    pub use crate::inject::inject_reminders;
    pub use crate::orchestrator::SystemReminderOrchestrator;
    pub use crate::types::AttachmentType;
    pub use crate::types::ContentBlock;
    pub use crate::types::MessageRole;
    pub use crate::types::ReminderMessage;
    pub use crate::types::ReminderOutput;
    pub use crate::types::ReminderTier;
    pub use crate::types::SystemReminder;
    pub use crate::types::XmlTag;
    pub use crate::xml::wrap_system_reminder;
    // Re-export FileTracker from cocode-tools
    pub use cocode_tools::FileTracker;
}
