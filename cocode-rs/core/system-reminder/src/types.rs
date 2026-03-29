//! Core types for system reminders.
//!
//! This module defines the fundamental types used throughout the system reminder
//! infrastructure, including attachment types, reminder tiers, and XML tags.

use serde::Deserialize;
use serde::Serialize;

/// Reminder tier determines when generators run.
///
/// Tiers allow filtering generators based on the agent context:
/// - `Core`: Always runs, for all agents including sub-agents
/// - `MainAgentOnly`: Only runs for the main agent, not sub-agents
/// - `UserPrompt`: Only runs when user input is present
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReminderTier {
    /// Always checked, available to all agents including sub-agents.
    Core,
    /// Only for main agent, not sub-agents.
    MainAgentOnly,
    /// Only when user input exists in this turn.
    UserPrompt,
}

/// XML tag types for wrapping reminder content.
///
/// Different tags serve different purposes and may be handled differently
/// by the model or UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XmlTag {
    /// Primary system reminder tag: `<system-reminder>`.
    SystemReminder,
    /// Async status notifications: `<system-notification>`.
    SystemNotification,
    /// LSP diagnostic issues: `<new-diagnostics>`.
    NewDiagnostics,
    /// Past session data: `<session-memory>`.
    SessionMemory,
    /// No XML wrapping (content is already wrapped or should be raw).
    None,
}

impl XmlTag {
    /// Get the XML tag name string.
    pub fn tag_name(&self) -> Option<&'static str> {
        match self {
            XmlTag::SystemReminder => Some("system-reminder"),
            XmlTag::SystemNotification => Some("system-notification"),
            XmlTag::NewDiagnostics => Some("new-diagnostics"),
            XmlTag::SessionMemory => Some("session-memory"),
            XmlTag::None => None,
        }
    }
}

/// Types of attachments that can be generated.
///
/// Each attachment type has an associated tier and XML tag. The generator
/// for each type produces content specific to that attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentType {
    // === Core tier (always run) ===
    /// Security guidelines (dual-placed for compaction survival).
    SecurityGuidelines,
    /// Detects files that changed since last read.
    ChangedFiles,
    /// Plan mode entry instructions (5-phase workflow).
    PlanModeEnter,
    /// Reference to plan file after compaction.
    PlanModeFileReference,
    /// Periodic reminder to use update_plan tool.
    PlanToolReminder,
    /// Plan mode exit instructions (one-time after approval).
    PlanModeExit,
    /// User-defined critical instructions.
    CriticalInstruction,
    /// Auto-discovered CLAUDE.md and rules files.
    NestedMemory,

    // === MainAgentOnly tier ===
    /// Available skills for the Skill tool.
    AvailableSkills,
    /// Background shell task status.
    BackgroundTask,
    /// LSP diagnostic injection.
    LspDiagnostics,
    /// Output style instructions.
    OutputStyle,
    /// Task/todo list context.
    TodoReminders,
    /// Delegate mode instructions.
    DelegateMode,
    /// Auto mode instructions (autonomous execution).
    AutoMode,
    /// Auto mode exit notification.
    AutoModeExit,
    /// Collaboration notifications from other agents.
    CollabNotifications,
    /// Plan verification reminder during implementation.
    PlanVerification,

    // === UserPrompt tier ===
    /// Files mentioned via @file syntax.
    AtMentionedFiles,
    /// Agent invocations via @agent-type syntax.
    AgentMentions,
    /// Skill invoked by user (skill prompt content injection).
    InvokedSkills,

    // === Hook-related (MainAgentOnly tier) ===
    /// Background hook completed and returned additional context.
    AsyncHookResponse,
    /// Hook blocked execution and returned context.
    HookBlockingError,
    /// Hook succeeded and added context for the model.
    HookAdditionalContext,

    // === Real-time steering ===
    /// Queued commands from user (Enter during streaming).
    /// Consumed once and injected as steering to address each message.
    QueuedCommands,

    // === Cron state ===
    /// Cron job state reminders (survives compaction).
    CronReminders,

    // === Team context (Core tier) ===
    /// Team identity and member list for teammates.
    TeamContext,
    /// Unread mailbox messages for teammates.
    TeamMailbox,

    // === Phase 2 (future) ===
    /// Tool result injection.
    ToolResult,
    /// Async agent task status.
    AsyncAgentStatus,
    /// Session memory from past sessions.
    SessionMemoryContent,
    /// Token usage stats.
    TokenUsage,
    /// Budget in USD.
    BudgetUsd,

    // === Already read files ===
    /// Already read file summaries (generates tool_use/tool_result pairs).
    AlreadyReadFile,

    // === Compact file reference ===
    /// References to large files that were compacted.
    CompactFileReference,

    // === Rewind ===
    /// Notification that a rewind occurred (one-time, consumed after generation).
    Rewind,

    // === Compaction reminder ===
    /// Reminder that auto-compact is enabled (prevents "context anxiety").
    CompactionReminder,

    // === Auto memory ===
    /// Auto memory prompt (MEMORY.md instructions + content).
    AutoMemoryPrompt,
    /// Relevant memory files from semantic search.
    RelevantMemories,

    // === Worktree ===
    /// Worktree creation/removal notifications.
    WorktreeState,

    // === Delta tracking ===
    /// Deferred tools set has changed (new/removed tools since last turn).
    DeferredToolsDelta,
    /// MCP server instructions have changed.
    McpInstructionsDelta,

    // === Sandbox ===
    /// Recent sandbox violations (operation denied by sandbox policy).
    SandboxViolations,

    // === Session info ===
    /// Current session name (displayed to model for context).
    SessionName,
    /// Output token usage for current turn.
    OutputTokenUsage,
    /// Configuration change notification.
    ConfigChange,

    // === Effort / date / IDE ===
    EffortLevel,
    DateChange,
    SelectedLinesInIde,
    OpenedFileInIde,
}

impl AttachmentType {
    /// Get the XML tag for this attachment type.
    pub fn xml_tag(&self) -> XmlTag {
        match self {
            // Most attachments use the standard system-reminder tag
            AttachmentType::SecurityGuidelines
            | AttachmentType::ChangedFiles
            | AttachmentType::PlanModeEnter
            | AttachmentType::PlanModeFileReference
            | AttachmentType::PlanToolReminder
            | AttachmentType::PlanModeExit
            | AttachmentType::CriticalInstruction
            | AttachmentType::NestedMemory
            | AttachmentType::AvailableSkills
            | AttachmentType::BackgroundTask
            | AttachmentType::OutputStyle
            | AttachmentType::TodoReminders
            | AttachmentType::DelegateMode
            | AttachmentType::AutoMode
            | AttachmentType::AutoModeExit
            | AttachmentType::CollabNotifications
            | AttachmentType::PlanVerification
            | AttachmentType::AtMentionedFiles
            | AttachmentType::AgentMentions
            | AttachmentType::InvokedSkills
            | AttachmentType::AsyncHookResponse
            | AttachmentType::HookBlockingError
            | AttachmentType::HookAdditionalContext
            | AttachmentType::QueuedCommands
            | AttachmentType::CronReminders
            | AttachmentType::ToolResult
            | AttachmentType::AsyncAgentStatus
            | AttachmentType::TokenUsage
            | AttachmentType::BudgetUsd
            | AttachmentType::CompactFileReference
            | AttachmentType::Rewind
            | AttachmentType::TeamContext
            | AttachmentType::TeamMailbox
            | AttachmentType::CompactionReminder
            | AttachmentType::AutoMemoryPrompt
            | AttachmentType::RelevantMemories
            | AttachmentType::WorktreeState
            | AttachmentType::DeferredToolsDelta
            | AttachmentType::McpInstructionsDelta
            | AttachmentType::SandboxViolations
            | AttachmentType::SessionName
            | AttachmentType::OutputTokenUsage
            | AttachmentType::ConfigChange
            | AttachmentType::EffortLevel
            | AttachmentType::DateChange
            | AttachmentType::SelectedLinesInIde
            | AttachmentType::OpenedFileInIde => XmlTag::SystemReminder,

            // Already read files don't use XML tags (uses tool_use/tool_result)
            AttachmentType::AlreadyReadFile => XmlTag::None,

            // LSP diagnostics have their own tag
            AttachmentType::LspDiagnostics => XmlTag::NewDiagnostics,

            // Session memory has its own tag
            AttachmentType::SessionMemoryContent => XmlTag::SessionMemory,
        }
    }

    /// Get the reminder tier for this attachment type.
    pub fn tier(&self) -> ReminderTier {
        match self {
            // Core tier - always run
            AttachmentType::SecurityGuidelines
            | AttachmentType::ChangedFiles
            | AttachmentType::PlanModeEnter
            | AttachmentType::PlanModeFileReference
            | AttachmentType::PlanToolReminder
            | AttachmentType::PlanModeExit
            | AttachmentType::CriticalInstruction
            | AttachmentType::NestedMemory
            | AttachmentType::TeamContext
            | AttachmentType::TeamMailbox
            | AttachmentType::AutoMemoryPrompt
            | AttachmentType::SandboxViolations
            | AttachmentType::DateChange => ReminderTier::Core,

            // MainAgentOnly tier
            AttachmentType::AvailableSkills
            | AttachmentType::BackgroundTask
            | AttachmentType::LspDiagnostics
            | AttachmentType::OutputStyle
            | AttachmentType::TodoReminders
            | AttachmentType::DelegateMode
            | AttachmentType::AutoMode
            | AttachmentType::AutoModeExit
            | AttachmentType::CollabNotifications
            | AttachmentType::PlanVerification
            | AttachmentType::AsyncHookResponse
            | AttachmentType::HookBlockingError
            | AttachmentType::HookAdditionalContext
            | AttachmentType::QueuedCommands
            | AttachmentType::CronReminders
            | AttachmentType::ToolResult
            | AttachmentType::AsyncAgentStatus
            | AttachmentType::SessionMemoryContent
            | AttachmentType::TokenUsage
            | AttachmentType::BudgetUsd
            | AttachmentType::AlreadyReadFile
            | AttachmentType::CompactFileReference
            | AttachmentType::Rewind
            | AttachmentType::CompactionReminder
            | AttachmentType::RelevantMemories
            | AttachmentType::WorktreeState
            | AttachmentType::DeferredToolsDelta
            | AttachmentType::McpInstructionsDelta
            | AttachmentType::SessionName
            | AttachmentType::OutputTokenUsage
            | AttachmentType::ConfigChange
            | AttachmentType::EffortLevel => ReminderTier::MainAgentOnly,

            // UserPrompt tier
            AttachmentType::AtMentionedFiles
            | AttachmentType::AgentMentions
            | AttachmentType::InvokedSkills
            | AttachmentType::SelectedLinesInIde
            | AttachmentType::OpenedFileInIde => ReminderTier::UserPrompt,
        }
    }

    /// Get the display name for this attachment type.
    pub fn name(&self) -> &'static str {
        match self {
            AttachmentType::SecurityGuidelines => "security_guidelines",
            AttachmentType::ChangedFiles => "changed_files",
            AttachmentType::PlanModeEnter => "plan_mode_enter",
            AttachmentType::PlanModeFileReference => "plan_mode_file_reference",
            AttachmentType::PlanToolReminder => "plan_tool_reminder",
            AttachmentType::PlanModeExit => "plan_mode_exit",
            AttachmentType::CriticalInstruction => "critical_instruction",
            AttachmentType::NestedMemory => "nested_memory",
            AttachmentType::AvailableSkills => "available_skills",
            AttachmentType::BackgroundTask => "background_task",
            AttachmentType::LspDiagnostics => "lsp_diagnostics",
            AttachmentType::OutputStyle => "output_style",
            AttachmentType::TodoReminders => "todo_reminders",
            AttachmentType::DelegateMode => "delegate_mode",
            AttachmentType::AutoMode => "auto_mode",
            AttachmentType::AutoModeExit => "auto_mode_exit",
            AttachmentType::CollabNotifications => "collab_notifications",
            AttachmentType::PlanVerification => "plan_verification",
            AttachmentType::AtMentionedFiles => "at_mentioned_files",
            AttachmentType::AgentMentions => "agent_mentions",
            AttachmentType::InvokedSkills => "invoked_skills",
            AttachmentType::AsyncHookResponse => "async_hook_response",
            AttachmentType::HookBlockingError => "hook_blocking_error",
            AttachmentType::HookAdditionalContext => "hook_additional_context",
            AttachmentType::QueuedCommands => "queued_commands",
            AttachmentType::CronReminders => "cron_reminders",
            AttachmentType::ToolResult => "tool_result",
            AttachmentType::AsyncAgentStatus => "async_agent_status",
            AttachmentType::SessionMemoryContent => "session_memory",
            AttachmentType::TokenUsage => "token_usage",
            AttachmentType::BudgetUsd => "budget_usd",
            AttachmentType::AlreadyReadFile => "already_read_file",
            AttachmentType::CompactFileReference => "compact_file_reference",
            AttachmentType::TeamContext => "team_context",
            AttachmentType::TeamMailbox => "team_mailbox",
            AttachmentType::Rewind => "rewind",
            AttachmentType::CompactionReminder => "compaction_reminder",
            AttachmentType::AutoMemoryPrompt => "auto_memory_prompt",
            AttachmentType::RelevantMemories => "relevant_memories",
            AttachmentType::WorktreeState => "worktree_state",
            AttachmentType::DeferredToolsDelta => "deferred_tools_delta",
            AttachmentType::McpInstructionsDelta => "mcp_instructions_delta",
            AttachmentType::SandboxViolations => "sandbox_violations",
            AttachmentType::SessionName => "session_name",
            AttachmentType::OutputTokenUsage => "output_token_usage",
            AttachmentType::ConfigChange => "config_change",
            AttachmentType::EffortLevel => "effort_level",
            AttachmentType::DateChange => "date_change",
            AttachmentType::SelectedLinesInIde => "selected_lines_in_ide",
            AttachmentType::OpenedFileInIde => "opened_file_in_ide",
        }
    }
}

impl std::fmt::Display for AttachmentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// ReminderOutput and related types
// ============================================================================

/// Generator output - supports multiple message types.
///
/// This enum allows generators to produce either simple text content
/// or multiple messages (used for tool_use/tool_result pairs).
///
/// # Silent vs Model-Visible Outputs
///
/// Outputs are categorized into two types:
///
/// **Model-visible** (sent to API):
/// - `Text` - Simple text content wrapped in XML tags
/// - `Messages` - Multi-message content (tool_use/tool_result pairs)
/// - `ModelAttachment` - Structured payload visible to model
///
/// **Silent** (zero tokens to API, UI-only):
/// - `Silent` - No content, just state tracking
/// - `SilentText` - Display-only text
/// - `SilentMessages` - Display-only messages
/// - `SilentAttachment` - Structured silent payload
///
/// Silent outputs are filtered out during message injection, reducing token
/// usage while still being visible in UI logs. Used for already-read files
/// to inform the model without consuming context window space.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReminderOutput {
    // === Model-visible outputs ===
    /// Single text content (most common case).
    Text(String),
    /// Multiple messages (used for tool_use/tool_result pairs).
    Messages(Vec<ReminderMessage>),
    /// Structured payload visible to model.
    ModelAttachment {
        /// The payload data.
        payload: serde_json::Value,
    },

    // === Silent outputs (zero tokens to API) ===
    /// No content, just state tracking.
    Silent,
    /// Display-only text (not sent to model).
    SilentText {
        /// The text content for UI display only.
        content: String,
    },
    /// Display-only messages (not sent to model).
    SilentMessages {
        /// Messages for UI display only.
        messages: Vec<ReminderMessage>,
    },
    /// Structured silent payload (not sent to model).
    SilentAttachment {
        /// The payload data for UI display only.
        payload: serde_json::Value,
    },
}

impl ReminderOutput {
    /// Get the text content if this is a Text variant.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ReminderOutput::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Get the messages if this is a Messages variant.
    pub fn as_messages(&self) -> Option<&[ReminderMessage]> {
        match self {
            ReminderOutput::Messages(msgs) => Some(msgs),
            _ => None,
        }
    }

    /// Get the payload if this is a ModelAttachment or SilentAttachment variant.
    pub fn as_attachment(&self) -> Option<&serde_json::Value> {
        match self {
            ReminderOutput::ModelAttachment { payload }
            | ReminderOutput::SilentAttachment { payload } => Some(payload),
            _ => None,
        }
    }

    /// Check if this is a text output.
    pub fn is_text(&self) -> bool {
        matches!(self, ReminderOutput::Text(_))
    }

    /// Check if this is a messages output.
    pub fn is_messages(&self) -> bool {
        matches!(self, ReminderOutput::Messages(_))
    }

    /// Check if this is a model attachment output.
    pub fn is_model_attachment(&self) -> bool {
        matches!(self, ReminderOutput::ModelAttachment { .. })
    }

    /// Check if this output is silent (zero tokens to API).
    ///
    /// Silent outputs are filtered out during message injection,
    /// reducing token usage for already-read files.
    pub fn is_silent(&self) -> bool {
        matches!(
            self,
            ReminderOutput::Silent
                | ReminderOutput::SilentText { .. }
                | ReminderOutput::SilentMessages { .. }
                | ReminderOutput::SilentAttachment { .. }
        )
    }

    /// Check if this output should be sent to the model.
    ///
    /// This is the inverse of `is_silent()`.
    pub fn is_model_visible(&self) -> bool {
        !self.is_silent()
    }

    /// Create a silent text output for UI display only.
    pub fn silent_text(content: impl Into<String>) -> Self {
        ReminderOutput::SilentText {
            content: content.into(),
        }
    }

    /// Create a silent attachment output for UI display only.
    pub fn silent_attachment(payload: serde_json::Value) -> Self {
        ReminderOutput::SilentAttachment { payload }
    }
}

/// A message within a reminder output.
///
/// Used when generating tool_use/tool_result pairs or other multi-message content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderMessage {
    /// The role of this message (user or assistant).
    pub role: MessageRole,
    /// Content blocks within this message.
    pub blocks: Vec<ContentBlock>,
    /// Whether this is metadata (hidden from user, visible to model).
    pub is_meta: bool,
}

impl ReminderMessage {
    /// Create a new user message.
    pub fn user(blocks: Vec<ContentBlock>) -> Self {
        Self {
            role: MessageRole::User,
            blocks,
            is_meta: true,
        }
    }

    /// Create a new assistant message.
    pub fn assistant(blocks: Vec<ContentBlock>) -> Self {
        Self {
            role: MessageRole::Assistant,
            blocks,
            is_meta: true,
        }
    }
}

/// Role of a message within a reminder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// User message (typically contains tool_result).
    User,
    /// Assistant message (typically contains tool_use).
    Assistant,
}

/// Content block within a reminder message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text content.
    Text { text: String },
    /// Tool use block (synthetic tool call).
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    /// Tool result block.
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

impl ContentBlock {
    /// Create a text content block.
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text { text: text.into() }
    }

    /// Create a tool use content block.
    pub fn tool_use(
        id: impl Into<String>,
        name: impl Into<String>,
        input: serde_json::Value,
    ) -> Self {
        ContentBlock::ToolUse {
            id: id.into(),
            name: name.into(),
            input,
        }
    }

    /// Create a tool result content block.
    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<String>) -> Self {
        ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
        }
    }
}

// ============================================================================
// Metadata Types for Specific Attachments
// ============================================================================

/// Metadata for AlreadyReadFile attachments.
///
/// Contains information about files that were already read and unchanged.
/// This metadata allows the UI to display "Read <filename>" notifications
/// while the API receives zero tokens (silent reminder).
///
/// Claude Code v2.1.38 alignment: already_read_file type is SILENT.
/// The normalizer returns [] for this type, meaning zero tokens to API.
/// The UI uses this metadata to show the notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlreadyReadFileMeta {
    /// Paths of files that were already read and unchanged.
    pub paths: Vec<std::path::PathBuf>,
}

// ============================================================================
// SystemReminder
// ============================================================================

/// Information about a file read during reminder generation.
///
/// Used by the @mention generator to track files that were read
/// so the driver can update FileTracker after generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReadInfo {
    /// Path to the file that was read.
    pub path: std::path::PathBuf,
    /// Content of the file (for FileTracker state).
    pub content: String,
    /// File modification time (for change detection).
    pub mtime: Option<std::time::SystemTime>,
    /// Turn number when the file was read.
    pub turn_number: i32,
    /// Kind of read operation.
    #[serde(default)]
    pub read_kind: FileReadKind,
    /// Line offset of the read (1-based, None if from start).
    /// Uses i64 for large file support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    /// Line limit of the read.
    /// Uses i64 for large file support.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
}

/// Kind of file read operation.
///
/// Re-exported from protocol for use in system reminders.
pub use cocode_protocol::FileReadKind;

/// A generated system reminder ready for injection.
///
/// This represents the output of a generator after processing.
/// Supports both simple text content and multi-message outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemReminder {
    /// The type of attachment this reminder represents.
    pub attachment_type: AttachmentType,
    /// The output content (text or messages).
    pub output: ReminderOutput,
    /// The tier this reminder belongs to (derived from attachment_type).
    pub tier: ReminderTier,
    /// Whether this is metadata (hidden from user, visible to model).
    pub is_meta: bool,
    /// Whether this reminder is silent (zero tokens in API, UI-only).
    ///
    /// Silent reminders are filtered out during message injection,
    /// reducing token usage while still being visible in UI logs.
    /// Used for already-read files to inform the model without
    /// consuming context window space.
    #[serde(default)]
    pub is_silent: bool,
    /// Optional metadata for specific attachment types.
    ///
    /// Used by silent reminders to provide UI-visible information
    /// without sending tokens to the API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ReminderMetadata>,
    /// Files that were read during generation (for FileTracker updates).
    ///
    /// Used by the @mention generator to track files that need to be
    /// recorded in FileTracker. The driver processes this list after
    /// generate_all() to update the shared FileTracker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_reads: Vec<FileReadInfo>,
    /// Whether this reminder should bypass the throttle.
    ///
    /// Set by generators when the content is urgent (e.g., completion
    /// notifications for background agents that should not be delayed).
    #[serde(default)]
    pub bypass_throttle: bool,
}

/// Type-specific metadata for reminders.
///
/// Allows silent reminders to carry information for UI display
/// without sending content to the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReminderMetadata {
    /// Metadata for already-read file reminders.
    AlreadyReadFile(AlreadyReadFileMeta),
}

impl SystemReminder {
    /// Create a new text-based system reminder.
    ///
    /// This is the most common case for simple text reminders.
    pub fn text(attachment_type: AttachmentType, content: impl Into<String>) -> Self {
        Self {
            tier: attachment_type.tier(),
            attachment_type,
            output: ReminderOutput::Text(content.into()),
            is_meta: true,
            is_silent: false,
            metadata: None,
            file_reads: Vec::new(),
            bypass_throttle: false,
        }
    }

    /// Create a new multi-message system reminder.
    ///
    /// Used for generating tool_use/tool_result pairs and other
    /// multi-message content.
    pub fn messages(attachment_type: AttachmentType, messages: Vec<ReminderMessage>) -> Self {
        Self {
            tier: attachment_type.tier(),
            attachment_type,
            output: ReminderOutput::Messages(messages),
            is_meta: true,
            is_silent: false,
            metadata: None,
            file_reads: Vec::new(),
            bypass_throttle: false,
        }
    }

    /// Create a new model attachment reminder.
    ///
    /// Used for structured payloads that should be visible to the model.
    pub fn model_attachment(attachment_type: AttachmentType, payload: serde_json::Value) -> Self {
        Self {
            tier: attachment_type.tier(),
            attachment_type,
            output: ReminderOutput::ModelAttachment { payload },
            is_meta: true,
            is_silent: false,
            metadata: None,
            file_reads: Vec::new(),
            bypass_throttle: false,
        }
    }

    /// Create a silent reminder with no content.
    ///
    /// Used for state tracking only, no tokens sent to API.
    pub fn silent(attachment_type: AttachmentType) -> Self {
        Self {
            tier: attachment_type.tier(),
            attachment_type,
            output: ReminderOutput::Silent,
            is_meta: true,
            is_silent: true,
            metadata: None,
            file_reads: Vec::new(),
            bypass_throttle: false,
        }
    }

    /// Create a silent text reminder (UI display only).
    ///
    /// The text is visible in UI logs but not sent to the model.
    pub fn silent_text(attachment_type: AttachmentType, content: impl Into<String>) -> Self {
        Self {
            tier: attachment_type.tier(),
            attachment_type,
            output: ReminderOutput::SilentText {
                content: content.into(),
            },
            is_meta: true,
            is_silent: true,
            metadata: None,
            file_reads: Vec::new(),
            bypass_throttle: false,
        }
    }

    /// Create a silent attachment reminder (UI display only).
    ///
    /// The payload is visible in UI logs but not sent to the model.
    pub fn silent_attachment(attachment_type: AttachmentType, payload: serde_json::Value) -> Self {
        Self {
            tier: attachment_type.tier(),
            attachment_type,
            output: ReminderOutput::SilentAttachment { payload },
            is_meta: true,
            is_silent: true,
            metadata: None,
            file_reads: Vec::new(),
            bypass_throttle: false,
        }
    }

    /// Create a new system reminder (legacy API, creates text output).
    ///
    /// For backwards compatibility. Prefer `SystemReminder::text()` for new code.
    pub fn new(attachment_type: AttachmentType, content: impl Into<String>) -> Self {
        Self::text(attachment_type, content)
    }

    /// Set whether this reminder is silent (zero tokens in API).
    ///
    /// Silent reminders are filtered out during message injection,
    /// reducing token usage for already-read files.
    pub fn with_silent(mut self, is_silent: bool) -> Self {
        self.is_silent = is_silent;
        self
    }

    /// Set whether this reminder should bypass throttle checks.
    ///
    /// Used for urgent content like completion notifications that
    /// should not be delayed by the per-generator turn throttle.
    pub fn set_bypass_throttle(&mut self, bypass: bool) {
        self.bypass_throttle = bypass;
    }

    /// Set the metadata for this reminder.
    ///
    /// Used by silent reminders to provide UI-visible information.
    pub fn with_metadata(mut self, metadata: ReminderMetadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Set the file reads for this reminder.
    ///
    /// Used by the @mention generator to track files that need to be
    /// recorded in FileTracker.
    pub fn with_file_reads(mut self, file_reads: Vec<FileReadInfo>) -> Self {
        self.file_reads = file_reads;
        self
    }

    /// Create an AlreadyReadFile silent reminder with metadata.
    ///
    /// This is a convenience constructor for the common case of creating
    /// an already-read-file reminder that is silent (zero tokens) but
    /// carries path information for UI display.
    ///
    /// Uses `SilentAttachment` variant for structured payload that is
    /// visible in UI logs but not sent to the API.
    pub fn already_read_files(paths: Vec<std::path::PathBuf>) -> Self {
        let payload = serde_json::to_value(AlreadyReadFileMeta {
            paths: paths.clone(),
        })
        .unwrap_or(serde_json::Value::Null);

        Self {
            tier: AttachmentType::AlreadyReadFile.tier(),
            attachment_type: AttachmentType::AlreadyReadFile,
            output: ReminderOutput::SilentAttachment { payload },
            is_meta: true,
            is_silent: true,
            metadata: Some(ReminderMetadata::AlreadyReadFile(AlreadyReadFileMeta {
                paths,
            })),
            file_reads: Vec::new(),
            bypass_throttle: false,
        }
    }

    /// Get the XML tag for this reminder.
    pub fn xml_tag(&self) -> XmlTag {
        self.attachment_type.xml_tag()
    }

    /// Get the text content if this is a text reminder.
    pub fn content(&self) -> Option<&str> {
        self.output.as_text()
    }

    /// Get the wrapped content with XML tags.
    ///
    /// Returns `None` for multi-message reminders or silent reminders
    /// (they don't use XML wrapping).
    pub fn wrapped_content(&self) -> Option<String> {
        match &self.output {
            ReminderOutput::Text(content) => {
                Some(crate::xml::wrap_with_tag(content, self.xml_tag()))
            }
            ReminderOutput::Messages(_)
            | ReminderOutput::ModelAttachment { .. }
            | ReminderOutput::Silent
            | ReminderOutput::SilentText { .. }
            | ReminderOutput::SilentMessages { .. }
            | ReminderOutput::SilentAttachment { .. } => None,
        }
    }

    /// Check if this is a text reminder.
    pub fn is_text(&self) -> bool {
        self.output.is_text()
    }

    /// Check if this is a multi-message reminder.
    pub fn is_messages(&self) -> bool {
        self.output.is_messages()
    }

    /// Check if this reminder is silent (zero tokens to API).
    pub fn is_silent_output(&self) -> bool {
        self.output.is_silent() || self.is_silent
    }
}

#[cfg(test)]
#[path = "types.test.rs"]
mod tests;
