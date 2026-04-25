//! Full `Attachment` taxonomy — every TS `Attachment.type` in
//! `src/utils/attachments.ts:440-731` gets a variant here.
//!
//! This enum is the **single compile-time source of truth** for how many
//! distinct TS attachment discriminators exist and how each one is
//! handled on the Rust side. The paired [`Coverage`] classifier forces
//! every variant to be explicitly categorized — a new TS attachment type
//! breaks the [`coverage_of`] match until someone decides where it lives.
//!
//! **Scope.** This crate (`coco-types`) owns the discriminator. It does
//! **not** own per-variant payload structures — those belong to the
//! crate that produces the data (e.g. file attachments live on
//! `core/context`, hook events live on `hooks`, reminder payloads live
//! on `core/system-reminder`). `coco-types` has zero internal
//! dependencies; adding payload types here would force every downstream
//! crate to depend on the union of all consumer crates' data shapes.
//!
//! See `core/system-reminder/README.md` "Full TS Attachment coverage
//! index" for the per-variant rationale behind each [`Coverage`]
//! assignment.

use serde::Deserialize;
use serde::Serialize;

/// Every TS `Attachment.type` discriminator. 60 variants.
///
/// Wire format is snake_case via `#[serde(rename_all = "snake_case")]`
/// to match TS `Attachment.type` exactly, so transcripts round-trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    // ── Reminder (in-crate `core/system-reminder` generators) ──
    PlanMode,
    PlanModeReentry,
    PlanModeExit,
    AutoMode,
    AutoModeExit,
    TodoReminder,
    TaskReminder,
    /// TS: user-supplied per-turn critical instruction
    /// (`toolUseContext.criticalSystemReminder_EXPERIMENTAL`).
    ///
    /// **coco-rs dual role**: also serves as the generic carrier kind for
    /// `coco_messages::create_meta_message` / `create_system_reminder_message`
    /// (post-Phase-2 drop-in for the old `User{is_meta:true}` shape). The
    /// reminder-generator path reads `ctx.config.critical_instruction`
    /// for the TS-aligned case; engine-internal meta injection reuses the
    /// same kind because API-visible + UI-hidden wrapped text is exactly
    /// its visibility profile. Both paths land in `Message::Attachment`
    /// with `AttachmentBody::Api(LlmMessage)`.
    CriticalSystemReminder,
    CompactionReminder,
    DateChange,
    VerifyPlanReminder,
    UltrathinkEffort,
    TokenUsage,
    BudgetUsd,
    OutputTokenUsage,
    CompanionIntro,
    DeferredToolsDelta,
    AgentListingDelta,
    McpInstructionsDelta,
    HookSuccess,
    HookBlockingError,
    HookAdditionalContext,
    HookStoppedContinuation,
    AsyncHookResponse,
    Diagnostics,
    OutputStyle,
    QueuedCommand,
    TaskStatus,
    SkillListing,
    InvokedSkills,
    TeammateMailbox,
    TeamContext,
    McpResource,
    AgentMention,
    SelectedLinesInIde,
    OpenedFileInIde,
    NestedMemory,
    RelevantMemories,

    // ── Silent reminder (in-crate, zero API tokens, metadata for UI) ──
    AlreadyReadFile,
    EditedImageFile,

    // ── Outside reminder (different crate owns the model-visible path) ──
    File,
    Directory,
    PdfReference,
    CompactFileReference,
    PlanFileReference,
    EditedTextFile,

    // ── Silent event (UI / telemetry / permission surface, not a reminder) ──
    CommandPermissions,
    HookCancelled,
    HookErrorDuringExecution,
    HookNonBlockingError,
    HookPermissionDecision,
    HookSystemMessage,
    StructuredOutput,
    DynamicSkill,

    // ── Feature-gated (TS feature flag; runtime not ported) ──
    ContextEfficiency,
    SkillDiscovery,

    // ── Runtime bookkeeping (no model-visible API text in TS either) ──
    MaxTurnsReached,
    CurrentSessionMemory,
    TeammateShutdownBatch,
    BagelConsole,
}

impl AttachmentKind {
    /// Stable string identifier matching the snake_case wire form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlanMode => "plan_mode",
            Self::PlanModeReentry => "plan_mode_reentry",
            Self::PlanModeExit => "plan_mode_exit",
            Self::AutoMode => "auto_mode",
            Self::AutoModeExit => "auto_mode_exit",
            Self::TodoReminder => "todo_reminder",
            Self::TaskReminder => "task_reminder",
            Self::CriticalSystemReminder => "critical_system_reminder",
            Self::CompactionReminder => "compaction_reminder",
            Self::DateChange => "date_change",
            Self::VerifyPlanReminder => "verify_plan_reminder",
            Self::UltrathinkEffort => "ultrathink_effort",
            Self::TokenUsage => "token_usage",
            Self::BudgetUsd => "budget_usd",
            Self::OutputTokenUsage => "output_token_usage",
            Self::CompanionIntro => "companion_intro",
            Self::DeferredToolsDelta => "deferred_tools_delta",
            Self::AgentListingDelta => "agent_listing_delta",
            Self::McpInstructionsDelta => "mcp_instructions_delta",
            Self::HookSuccess => "hook_success",
            Self::HookBlockingError => "hook_blocking_error",
            Self::HookAdditionalContext => "hook_additional_context",
            Self::HookStoppedContinuation => "hook_stopped_continuation",
            Self::AsyncHookResponse => "async_hook_response",
            Self::Diagnostics => "diagnostics",
            Self::OutputStyle => "output_style",
            Self::QueuedCommand => "queued_command",
            Self::TaskStatus => "task_status",
            Self::SkillListing => "skill_listing",
            Self::InvokedSkills => "invoked_skills",
            Self::TeammateMailbox => "teammate_mailbox",
            Self::TeamContext => "team_context",
            Self::McpResource => "mcp_resource",
            Self::AgentMention => "agent_mention",
            Self::SelectedLinesInIde => "selected_lines_in_ide",
            Self::OpenedFileInIde => "opened_file_in_ide",
            Self::NestedMemory => "nested_memory",
            Self::RelevantMemories => "relevant_memories",
            Self::AlreadyReadFile => "already_read_file",
            Self::EditedImageFile => "edited_image_file",
            Self::File => "file",
            Self::Directory => "directory",
            Self::PdfReference => "pdf_reference",
            Self::CompactFileReference => "compact_file_reference",
            Self::PlanFileReference => "plan_file_reference",
            Self::EditedTextFile => "edited_text_file",
            Self::CommandPermissions => "command_permissions",
            Self::HookCancelled => "hook_cancelled",
            Self::HookErrorDuringExecution => "hook_error_during_execution",
            Self::HookNonBlockingError => "hook_non_blocking_error",
            Self::HookPermissionDecision => "hook_permission_decision",
            Self::HookSystemMessage => "hook_system_message",
            Self::StructuredOutput => "structured_output",
            Self::DynamicSkill => "dynamic_skill",
            Self::ContextEfficiency => "context_efficiency",
            Self::SkillDiscovery => "skill_discovery",
            Self::MaxTurnsReached => "max_turns_reached",
            Self::CurrentSessionMemory => "current_session_memory",
            Self::TeammateShutdownBatch => "teammate_shutdown_batch",
            Self::BagelConsole => "bagel_console",
        }
    }

    /// How this attachment type is handled in coco-rs. See [`Coverage`].
    pub const fn coverage(self) -> Coverage {
        coverage_of(self)
    }

    /// Does this attachment's body reach the LLM API?
    ///
    /// TS parity: kind NOT in `normalizeAttachmentForAPI`-returns-`[]` list
    /// (`utils/messages.ts:4252-4261`). Hand-maintained match — ORTHOGONAL
    /// to `renders_in_transcript`. The four quadrants (API × UI) all exist.
    pub const fn is_api_visible(self) -> bool {
        use AttachmentKind::*;
        match self {
            // All reminders + outside-reminder file content go to API.
            PlanMode
            | PlanModeReentry
            | PlanModeExit
            | AutoMode
            | AutoModeExit
            | TodoReminder
            | TaskReminder
            | CriticalSystemReminder
            | CompactionReminder
            | DateChange
            | VerifyPlanReminder
            | UltrathinkEffort
            | TokenUsage
            | BudgetUsd
            | OutputTokenUsage
            | CompanionIntro
            | DeferredToolsDelta
            | AgentListingDelta
            | McpInstructionsDelta
            | HookSuccess
            | HookBlockingError
            | HookAdditionalContext
            | HookStoppedContinuation
            | AsyncHookResponse
            | Diagnostics
            | OutputStyle
            | QueuedCommand
            | TaskStatus
            | SkillListing
            | InvokedSkills
            | TeammateMailbox
            | TeamContext
            | McpResource
            | AgentMention
            | SelectedLinesInIde
            | OpenedFileInIde
            | NestedMemory
            | RelevantMemories
            | File
            | Directory
            | PdfReference
            | CompactFileReference
            | PlanFileReference
            | EditedTextFile => true,
            // TS `normalizeAttachmentForAPI` returns `[]` for these.
            AlreadyReadFile
            | EditedImageFile
            | CommandPermissions
            | HookCancelled
            | HookErrorDuringExecution
            | HookNonBlockingError
            | HookPermissionDecision
            | HookSystemMessage
            | StructuredOutput
            | DynamicSkill
            | ContextEfficiency
            | SkillDiscovery
            | MaxTurnsReached
            | CurrentSessionMemory
            | TeammateShutdownBatch
            | BagelConsole => false,
        }
    }

    /// Does this attachment render in the UI transcript?
    ///
    /// TS parity: kind NOT in `NULL_RENDERING_ATTACHMENT_TYPES`
    /// (`components/messages/nullRenderingAttachments.ts:14-49`).
    /// Hand-maintained — ORTHOGONAL to `is_api_visible`.
    ///
    /// The `false` branch is the TS `NULL_RENDERING_TYPES` list verbatim;
    /// the `true` branch covers everything else that actually renders
    /// (file attachments, tool results, diagnostics, hook errors, etc.).
    pub const fn renders_in_transcript(self) -> bool {
        use AttachmentKind::*;
        match self {
            // TS `NULL_RENDERING_TYPES` (`nullRenderingAttachments.ts:14-49`).
            HookSuccess
            | HookAdditionalContext
            | HookCancelled
            | CommandPermissions
            | AgentMention
            | BudgetUsd
            | CriticalSystemReminder
            | EditedImageFile
            | EditedTextFile
            | OpenedFileInIde
            | OutputStyle
            | PlanMode
            | PlanModeExit
            | PlanModeReentry
            | StructuredOutput
            | TeamContext
            | TodoReminder
            | TaskReminder
            | ContextEfficiency
            | DeferredToolsDelta
            | McpInstructionsDelta
            | CompanionIntro
            | TokenUsage
            | UltrathinkEffort
            | MaxTurnsReached
            | AutoMode
            | AutoModeExit
            | OutputTokenUsage
            | VerifyPlanReminder
            | CurrentSessionMemory
            | CompactionReminder
            | DateChange => false,
            // Also treat silent-dedup / runtime-bookkeeping kinds as
            // non-rendering (not in TS NULL_RENDERING because TS doesn't
            // enumerate them there, but coco-rs intentionally hides them).
            AlreadyReadFile | SkillDiscovery | TeammateShutdownBatch | BagelConsole => false,
            // Everything else renders.
            AgentListingDelta
            | AsyncHookResponse
            | HookBlockingError
            | HookStoppedContinuation
            | Diagnostics
            | QueuedCommand
            | TaskStatus
            | SkillListing
            | InvokedSkills
            | TeammateMailbox
            | McpResource
            | SelectedLinesInIde
            | NestedMemory
            | RelevantMemories
            | File
            | Directory
            | PdfReference
            | CompactFileReference
            | PlanFileReference
            | HookErrorDuringExecution
            | HookNonBlockingError
            | HookSystemMessage
            | HookPermissionDecision
            | DynamicSkill => true,
        }
    }

    /// Should this attachment survive compaction? Returns `true` for:
    /// - audit-trail kinds (permission decisions, command permissions)
    /// - any API-hidden + UI-visible kind (preserves user's view of hook events)
    ///
    /// Everything else (pure reminders that regenerate per-turn, pure silent
    /// dedup markers) is stripped by the compactor.
    pub const fn survives_compaction(self) -> bool {
        use AttachmentKind::*;
        match self {
            // Audit / compliance trail.
            HookPermissionDecision | CommandPermissions => true,
            // API-hidden UI-visible — keep so user's transcript stays coherent
            // after compaction.
            HookErrorDuringExecution | HookNonBlockingError | HookSystemMessage | DynamicSkill => {
                true
            }
            // Outside-crate file references survive (content is already summary-
            // friendly and `services/compact` handles them specially).
            CompactFileReference | PlanFileReference => true,
            // Everything else: reminders regenerate per-turn, silent dedup
            // markers are ephemeral, file content gets re-injected via compact.
            _ => false,
        }
    }

    /// Every variant in declaration order. Length must equal 60 (TS
    /// `Attachment` union size at time of writing) — enforced by the
    /// parity test.
    pub const fn all() -> &'static [AttachmentKind] {
        &[
            Self::PlanMode,
            Self::PlanModeReentry,
            Self::PlanModeExit,
            Self::AutoMode,
            Self::AutoModeExit,
            Self::TodoReminder,
            Self::TaskReminder,
            Self::CriticalSystemReminder,
            Self::CompactionReminder,
            Self::DateChange,
            Self::VerifyPlanReminder,
            Self::UltrathinkEffort,
            Self::TokenUsage,
            Self::BudgetUsd,
            Self::OutputTokenUsage,
            Self::CompanionIntro,
            Self::DeferredToolsDelta,
            Self::AgentListingDelta,
            Self::McpInstructionsDelta,
            Self::HookSuccess,
            Self::HookBlockingError,
            Self::HookAdditionalContext,
            Self::HookStoppedContinuation,
            Self::AsyncHookResponse,
            Self::Diagnostics,
            Self::OutputStyle,
            Self::QueuedCommand,
            Self::TaskStatus,
            Self::SkillListing,
            Self::InvokedSkills,
            Self::TeammateMailbox,
            Self::TeamContext,
            Self::McpResource,
            Self::AgentMention,
            Self::SelectedLinesInIde,
            Self::OpenedFileInIde,
            Self::NestedMemory,
            Self::RelevantMemories,
            Self::AlreadyReadFile,
            Self::EditedImageFile,
            Self::File,
            Self::Directory,
            Self::PdfReference,
            Self::CompactFileReference,
            Self::PlanFileReference,
            Self::EditedTextFile,
            Self::CommandPermissions,
            Self::HookCancelled,
            Self::HookErrorDuringExecution,
            Self::HookNonBlockingError,
            Self::HookPermissionDecision,
            Self::HookSystemMessage,
            Self::StructuredOutput,
            Self::DynamicSkill,
            Self::ContextEfficiency,
            Self::SkillDiscovery,
            Self::MaxTurnsReached,
            Self::CurrentSessionMemory,
            Self::TeammateShutdownBatch,
            Self::BagelConsole,
        ]
    }
}

impl std::fmt::Display for AttachmentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How an [`AttachmentKind`] is handled on the Rust side.
///
/// Every kind maps to exactly one variant via [`AttachmentKind::coverage`]
/// — a `match` in [`coverage_of`] that must stay exhaustive. Adding a new
/// [`AttachmentKind`] variant without assigning coverage fails to compile.
///
/// Strings are `&'static str` so callers can route / log / telemetry
/// without allocating. When a coverage entry points at a generator or
/// crate, keep the string in sync with the actual name — no runtime
/// validation catches drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// In-crate reminder, model-visible. `generator` names the
    /// [`AttachmentGenerator`] impl (e.g. `"PlanModeEnterGenerator"`).
    ///
    /// [`AttachmentGenerator`]: https://docs.rs/coco-system-reminder
    Reminder { generator: &'static str },
    /// In-crate reminder that injects zero API tokens but carries
    /// UI-visible metadata.
    SilentReminder { generator: &'static str },
    /// TS wraps this in `<system-reminder>` but coco-rs routes it through
    /// a different crate (typically `core/context` for file attachments,
    /// `services/compact` for post-compaction references).
    OutsideReminder {
        owner_crate: &'static str,
        note: &'static str,
    },
    /// TS `normalizeAttachmentForAPI` returns `[]` and coco-rs doesn't
    /// own the variant as a reminder. Event / bookkeeping data belongs
    /// to `owner_crate` (hooks, permissions, tools, …).
    SilentEvent {
        owner_crate: &'static str,
        note: &'static str,
    },
    /// Not ported because a TS feature gate keeps the runtime behind a
    /// flag coco-rs has no equivalent for yet.
    FeatureGated { feature: &'static str },
    /// TS runtime / UI bookkeeping only; never becomes API text. Kept
    /// in the union so `--resume`-loaded transcripts deserialize cleanly.
    RuntimeBookkeeping { note: &'static str },
}

/// Exhaustive mapping from [`AttachmentKind`] to [`Coverage`].
///
/// Every variant **must** appear in this match. Adding an
/// [`AttachmentKind`] variant without also adding an arm here is a
/// compile error, which is the point — it forces the author to decide
/// where the new variant lives before the repo builds.
///
/// The return values are themselves checked by the
/// `coverage_strings_do_not_drift` test: whenever a `generator` field
/// names a generator that doesn't exist in `core/system-reminder`, that
/// test should catch the drift during CI.
pub const fn coverage_of(kind: AttachmentKind) -> Coverage {
    use AttachmentKind::*;
    match kind {
        // ── In-crate reminders (model-visible) ──
        PlanMode => Coverage::Reminder {
            generator: "PlanModeEnterGenerator",
        },
        PlanModeReentry => Coverage::Reminder {
            generator: "PlanModeReentryGenerator",
        },
        PlanModeExit => Coverage::Reminder {
            generator: "PlanModeExitGenerator",
        },
        AutoMode => Coverage::Reminder {
            generator: "AutoModeEnterGenerator",
        },
        AutoModeExit => Coverage::Reminder {
            generator: "AutoModeExitGenerator",
        },
        TodoReminder => Coverage::Reminder {
            generator: "TodoRemindersGenerator",
        },
        TaskReminder => Coverage::Reminder {
            generator: "TaskRemindersGenerator",
        },
        CriticalSystemReminder => Coverage::Reminder {
            generator: "CriticalSystemReminderGenerator",
        },
        CompactionReminder => Coverage::Reminder {
            generator: "CompactionReminderGenerator",
        },
        DateChange => Coverage::Reminder {
            generator: "DateChangeGenerator",
        },
        VerifyPlanReminder => Coverage::Reminder {
            generator: "VerifyPlanReminderGenerator",
        },
        UltrathinkEffort => Coverage::Reminder {
            generator: "UltrathinkEffortGenerator",
        },
        TokenUsage => Coverage::Reminder {
            generator: "TokenUsageGenerator",
        },
        BudgetUsd => Coverage::Reminder {
            generator: "BudgetUsdGenerator",
        },
        OutputTokenUsage => Coverage::Reminder {
            generator: "OutputTokenUsageGenerator",
        },
        CompanionIntro => Coverage::Reminder {
            generator: "CompanionIntroGenerator",
        },
        DeferredToolsDelta => Coverage::Reminder {
            generator: "DeferredToolsDeltaGenerator",
        },
        AgentListingDelta => Coverage::Reminder {
            generator: "AgentListingDeltaGenerator",
        },
        McpInstructionsDelta => Coverage::Reminder {
            generator: "McpInstructionsDeltaGenerator",
        },
        HookSuccess => Coverage::Reminder {
            generator: "HookSuccessGenerator",
        },
        HookBlockingError => Coverage::Reminder {
            generator: "HookBlockingErrorGenerator",
        },
        HookAdditionalContext => Coverage::Reminder {
            generator: "HookAdditionalContextGenerator",
        },
        HookStoppedContinuation => Coverage::Reminder {
            generator: "HookStoppedContinuationGenerator",
        },
        AsyncHookResponse => Coverage::Reminder {
            generator: "AsyncHookResponseGenerator",
        },
        Diagnostics => Coverage::Reminder {
            generator: "DiagnosticsGenerator",
        },
        OutputStyle => Coverage::Reminder {
            generator: "OutputStyleGenerator",
        },
        QueuedCommand => Coverage::Reminder {
            generator: "QueuedCommandGenerator",
        },
        TaskStatus => Coverage::Reminder {
            generator: "TaskStatusGenerator",
        },
        SkillListing => Coverage::Reminder {
            generator: "SkillListingGenerator",
        },
        InvokedSkills => Coverage::Reminder {
            generator: "InvokedSkillsGenerator",
        },
        TeammateMailbox => Coverage::Reminder {
            generator: "TeammateMailboxGenerator",
        },
        TeamContext => Coverage::Reminder {
            generator: "TeamContextGenerator",
        },
        McpResource => Coverage::Reminder {
            generator: "McpResourcesGenerator",
        },
        AgentMention => Coverage::Reminder {
            generator: "AgentMentionsGenerator",
        },
        SelectedLinesInIde => Coverage::Reminder {
            generator: "IdeSelectionGenerator",
        },
        OpenedFileInIde => Coverage::Reminder {
            generator: "IdeOpenedFileGenerator",
        },
        NestedMemory => Coverage::Reminder {
            generator: "NestedMemoryGenerator",
        },
        RelevantMemories => Coverage::Reminder {
            generator: "RelevantMemoriesGenerator",
        },

        // ── In-crate silent reminders (metadata only) ──
        AlreadyReadFile => Coverage::SilentReminder {
            generator: "AlreadyReadFileGenerator",
        },
        EditedImageFile => Coverage::SilentReminder {
            generator: "EditedImageFileGenerator",
        },

        // ── Outside this crate (file attachments + post-compact references) ──
        File => Coverage::OutsideReminder {
            owner_crate: "core/context",
            note: "user @-mentioned file content is loaded via Attachment::File",
        },
        Directory => Coverage::OutsideReminder {
            owner_crate: "core/context",
            note: "directory listing emitted alongside @-mention resolution",
        },
        PdfReference => Coverage::OutsideReminder {
            owner_crate: "core/context",
            note: "large-PDF reference attached via @-mention pipeline",
        },
        CompactFileReference => Coverage::OutsideReminder {
            owner_crate: "services/compact",
            note: "post-compact file reference to preserve read paths",
        },
        PlanFileReference => Coverage::OutsideReminder {
            owner_crate: "services/compact",
            note: "post-compact plan file re-injection",
        },
        EditedTextFile => Coverage::OutsideReminder {
            owner_crate: "core/context",
            note: "changed-files tracker emits diff-bearing reminders",
        },

        // ── Silent events (UI / telemetry, owned outside reminder crate) ──
        CommandPermissions => Coverage::SilentEvent {
            owner_crate: "commands / permissions",
            note: "slash-command permission UI payload",
        },
        HookCancelled => Coverage::SilentEvent {
            owner_crate: "hooks",
            note: "hook cancellation event for UI / telemetry only",
        },
        HookErrorDuringExecution => Coverage::SilentEvent {
            owner_crate: "hooks",
            note: "hook runtime error event (non-blocking)",
        },
        HookNonBlockingError => Coverage::SilentEvent {
            owner_crate: "hooks",
            note: "non-blocking hook error surfaced to UI / logs",
        },
        HookPermissionDecision => Coverage::SilentEvent {
            owner_crate: "core/permissions",
            note: "permission decision feed; not model-visible",
        },
        HookSystemMessage => Coverage::SilentEvent {
            owner_crate: "hooks",
            note: "hook-originated system message for UI only",
        },
        StructuredOutput => Coverage::SilentEvent {
            owner_crate: "core/tool-runtime",
            note: "structured tool-output payload; consumed via ToolResult",
        },
        DynamicSkill => Coverage::SilentEvent {
            owner_crate: "skills",
            note: "dynamic skill-load marker; skill content flows via Skill tool",
        },

        // ── Feature-gated (awaiting runtime port) ──
        ContextEfficiency => Coverage::FeatureGated {
            feature: "HISTORY_SNIP (services/compact snip runtime not ported)",
        },
        SkillDiscovery => Coverage::FeatureGated {
            feature: "EXPERIMENTAL_SKILL_SEARCH (skill-search not ported)",
        },

        // ── Runtime bookkeeping (no model text in TS either) ──
        MaxTurnsReached => Coverage::RuntimeBookkeeping {
            note: "query-loop budget marker; produced in app/query, not a reminder",
        },
        CurrentSessionMemory => Coverage::RuntimeBookkeeping {
            note: "session-memory snapshot placeholder; no emitter in TS snapshot",
        },
        TeammateShutdownBatch => Coverage::RuntimeBookkeeping {
            note: "swarm transcript-collapse marker; UI-only",
        },
        BagelConsole => Coverage::RuntimeBookkeeping {
            note: "internal dev console placeholder; no API text",
        },
    }
}

/// Cross-crate carrier for `Attachment`-tagged events — the shape that
/// owning crates produce so the rest of coco-rs (UI / transcript /
/// telemetry) can route them uniformly.
///
/// Designed for the **silent / outside-reminder** half of the
/// [`AttachmentKind`] taxonomy. The in-crate reminder half
/// (`Coverage::Reminder` / `Coverage::SilentReminder`) goes through
/// `coco-system-reminder`'s own `SystemReminder` type instead — that
/// crate owns the model-visible rendering path.
///
/// # Who produces what
///
/// Use [`Coverage`] to determine if a kind should be produced here.
/// Owning crates per [`coverage_of`]:
///
/// - `hooks`: `HookCancelled`, `HookErrorDuringExecution`,
///   `HookNonBlockingError`, `HookSystemMessage`
/// - `core/permissions`: `HookPermissionDecision`
/// - `commands / permissions`: `CommandPermissions`
/// - `core/tool-runtime`: `StructuredOutput`
/// - `skills`: `DynamicSkill`
/// - `core/context` / `services/compact`: `File`, `Directory`,
///   `PdfReference`, `CompactFileReference`, `PlanFileReference`,
///   `EditedTextFile` (note: these are model-visible attachments
///   owned outside `system-reminder`, not silent events)
///
/// # Wire format
///
/// `kind` round-trips as the TS `Attachment.type` snake_case string;
/// `payload` is an opaque JSON blob whose shape is per-variant and
/// validated by the owning crate. `is_meta` mirrors TS `isMeta` on
/// `UserMessage` — `true` for silent / UI-only events, `false` if the
/// event should also surface in the model-visible transcript.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachmentEvent {
    pub kind: AttachmentKind,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub payload: serde_json::Value,
    #[serde(default = "default_is_meta")]
    pub is_meta: bool,
}

fn default_is_meta() -> bool {
    true
}

impl AttachmentEvent {
    /// Build a silent event with a structured payload (the common case
    /// for `Coverage::SilentEvent` kinds).
    pub fn silent(kind: AttachmentKind, payload: serde_json::Value) -> Self {
        Self {
            kind,
            payload,
            is_meta: true,
        }
    }

    /// Build a silent event with no payload — just a marker / tombstone.
    pub fn silent_marker(kind: AttachmentKind) -> Self {
        Self {
            kind,
            payload: serde_json::Value::Null,
            is_meta: true,
        }
    }

    /// Build an event that should appear in the model-visible transcript
    /// (not typical for silent events; reserved for owner crates that
    /// produce `Coverage::OutsideReminder` kinds like `edited_text_file`).
    pub fn visible(kind: AttachmentKind, payload: serde_json::Value) -> Self {
        Self {
            kind,
            payload,
            is_meta: false,
        }
    }
}

#[cfg(test)]
#[path = "attachment_kind.test.rs"]
mod tests;
