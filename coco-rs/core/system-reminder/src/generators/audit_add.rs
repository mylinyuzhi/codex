//! Audit-add reminder generators (May 2026).
//!
//! Eight TS-parity reminders that were missing from the originally-ported
//! catalog. Each is `Coverage::SilentReminder` with
//! `AttachmentKind::is_api_visible() == false` (TS
//! `normalizeAttachmentForAPI` returns `[]` for these kinds), so all
//! generators emit through [`SystemReminder::silent_text`] — the body
//! reaches the UI / transcript via
//! [`NormalizedMessages::display_only`](crate::inject::NormalizedMessages)
//! but never goes to the API. This avoids tripping the
//! `AttachmentMessage::api()` debug_assert that requires
//! `kind.is_api_visible()` to be true.
//!
//! Each generator carries the originating TS line reference in its doc
//! comment. Bodies are taken verbatim from `messages.ts` where the TS
//! template is short and stable; for the longer dynamic templates
//! (current_session_memory, command_permissions) the engine pre-formats
//! the content and we emit it verbatim, mirroring the
//! `TeammateMailboxGenerator`/`SkillListingGenerator` pre-formatted-body
//! pattern.
//!
//! ## Wiring status (R1 triage)
//!
//! | Generator                        | Status                       | Upstream owner / blocker                                    |
//! |----------------------------------|------------------------------|-------------------------------------------------------------|
//! | `MaxTurnsReachedGenerator`       | ✅ wired                     | `app/query/src/engine_turn_reminders.rs:593-594`            |
//! | `CommandPermissionsGenerator`    | ✅ wired (mailbox)           | `coco_query::ReminderMailbox.command_permissions`           |
//! | `DynamicSkillGenerator`          | ✅ wired (mailbox)           | `coco_query::ReminderMailbox.dynamic_skill`                 |
//! | `StructuredOutputGenerator`      | ✅ wired (mailbox)           | `coco_query::ReminderMailbox.structured_output`             |
//! | `TeammateShutdownBatchGenerator` | ✅ wired (mailbox)           | `coco_query::ReminderMailbox.teammate_shutdown_batch`       |
//! | `CurrentSessionMemoryGenerator`  | ⏳ pending — `None` upstream | `coco-memory` (SessionMemory). TS: `services/SessionMemory/sessionMemoryCheck.ts`. **Retained because TS has this attachment** (`attachments.ts:662-666`). |
//! | `SkillDiscoveryGenerator`        | ⏳ pending — `None` upstream | `coco-skills` heuristic suggester. TS: `services/skillSearch/prefetch.ts`. **Retained because TS has this attachment** (`attachments.ts:538-542`). |
//! | `ContextEfficiencyGenerator`     | ⛔ never fires in coco-rs    | Gated behind TS `feature('HISTORY_SNIP')` which coco-rs intentionally does not port (see root CLAUDE.md "Compaction — three generic strategies only"). Kept as a `None`-return so the parity-test invariant `all_attachment_type_variants_have_default_generator` enforces full coverage. |
//!
//! Pending generators (⏳) sit in the catalog because TS has the
//! matching attachment variant — removing them would create a TS-
//! parity regression. They emit when the upstream producer crate
//! (`coco-memory` / `coco-skills`) populates the snapshot via
//! `TurnReminderInput`.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

// ── max_turns_reached ──────────────────────────────────────────────────

/// TS `max_turns_reached` (`attachments.ts:657-660`,
/// `messages.ts:4259+`). Surfaces the turn-budget exhaustion condition
/// to the model so it can wrap up gracefully.
///
/// Gate: `ctx.max_turns_reached_signal == true` (engine pre-computes by
/// comparing `turn_number` to its configured cap).
#[derive(Debug, Default)]
pub struct MaxTurnsReachedGenerator;

#[async_trait]
impl AttachmentGenerator for MaxTurnsReachedGenerator {
    fn name(&self) -> &str {
        "MaxTurnsReachedGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::MaxTurnsReached
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.max_turns_reached
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.max_turns_reached_signal {
            return Ok(None);
        }
        Ok(Some(SystemReminder::silent_text(
            AttachmentType::MaxTurnsReached,
            "The configured maximum number of turns has been reached. Wrap up the current task and produce a final response — additional tool calls will be rejected by the engine.".to_string(),
        )))
    }
}

// ── current_session_memory ─────────────────────────────────────────────

/// TS `current_session_memory` (`attachments.ts:662-666`). Body is
/// pre-formatted by `coco-memory` and threaded through
/// [`GeneratorContext::current_session_memory`]; emit verbatim.
///
/// **Status (pending upstream):** `coco-memory` does not yet populate
/// this slot. TS source is `services/SessionMemory/sessionMemoryCheck.ts`
/// which runs the "memorable moment" classifier and emits the formatted
/// body when it fires. Until `coco-memory` ports that classifier the
/// generator returns `None` — TS-parity for the SessionMemory-disabled
/// case. The variant is retained because **TS has this attachment** and
/// removing it would be a TS-parity regression.
#[derive(Debug, Default)]
pub struct CurrentSessionMemoryGenerator;

#[async_trait]
impl AttachmentGenerator for CurrentSessionMemoryGenerator {
    fn name(&self) -> &str {
        "CurrentSessionMemoryGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::CurrentSessionMemory
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.current_session_memory
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(body) = ctx.current_session_memory.as_deref() else {
            return Ok(None);
        };
        if body.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::silent_text(
            AttachmentType::CurrentSessionMemory,
            body.to_string(),
        )))
    }
}

// ── command_permissions ────────────────────────────────────────────────

/// TS `command_permissions` (`attachments.ts:605-608`). Permissions
/// snapshot pre-formatted by `coco-permissions`.
#[derive(Debug, Default)]
pub struct CommandPermissionsGenerator;

#[async_trait]
impl AttachmentGenerator for CommandPermissionsGenerator {
    fn name(&self) -> &str {
        "CommandPermissionsGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::CommandPermissions
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.command_permissions
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(body) = ctx.command_permissions.as_deref() else {
            return Ok(None);
        };
        if body.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::silent_text(
            AttachmentType::CommandPermissions,
            body.to_string(),
        )))
    }
}

// ── dynamic_skill ──────────────────────────────────────────────────────

/// TS `dynamic_skill` (`attachments.ts:525-530`). Pre-formatted
/// directory listing of dynamically loaded skills (or `None` until
/// `coco-skills` wires the snapshot).
#[derive(Debug, Default)]
pub struct DynamicSkillGenerator;

#[async_trait]
impl AttachmentGenerator for DynamicSkillGenerator {
    fn name(&self) -> &str {
        "DynamicSkillGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::DynamicSkill
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.dynamic_skill
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(body) = ctx.dynamic_skill.as_deref() else {
            return Ok(None);
        };
        if body.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::silent_text(
            AttachmentType::DynamicSkill,
            body.to_string(),
        )))
    }
}

// ── skill_discovery ────────────────────────────────────────────────────

/// TS `skill_discovery` (`attachments.ts:538-542`). UserPrompt-tier
/// heuristic skill suggestion. Body pre-formatted by `coco-skills`.
///
/// **Status (pending upstream):** `coco-skills` does not yet emit a
/// pre-formatted body for the discovery hint. TS source is
/// `services/skillSearch/prefetch.ts::getTurnZeroSkillDiscovery`
/// (turn-0 user-input pass) + the inter-turn prefetch path. Until
/// `coco-skills` adopts that prefetcher and threads the result into
/// `ReminderMailbox`, the generator returns `None` — TS-parity for
/// the no-discovery-hit case. The variant is retained because **TS
/// has this attachment**.
#[derive(Debug, Default)]
pub struct SkillDiscoveryGenerator;

#[async_trait]
impl AttachmentGenerator for SkillDiscoveryGenerator {
    fn name(&self) -> &str {
        "SkillDiscoveryGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::SkillDiscovery
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.skill_discovery
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(body) = ctx.skill_discovery.as_deref() else {
            return Ok(None);
        };
        if body.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::silent_text(
            AttachmentType::SkillDiscovery,
            body.to_string(),
        )))
    }
}

// ── structured_output ──────────────────────────────────────────────────

/// TS `structured_output` (`attachments.ts:639-641`). A tool-emitted
/// structured-output blob the engine wants to surface back to the model
/// next turn.
#[derive(Debug, Default)]
pub struct StructuredOutputGenerator;

#[async_trait]
impl AttachmentGenerator for StructuredOutputGenerator {
    fn name(&self) -> &str {
        "StructuredOutputGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::StructuredOutput
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.structured_output
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(body) = ctx.structured_output.as_deref() else {
            return Ok(None);
        };
        if body.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::silent_text(
            AttachmentType::StructuredOutput,
            body.to_string(),
        )))
    }
}

// ── teammate_shutdown_batch ────────────────────────────────────────────

/// TS `teammate_shutdown_batch` (`attachments.ts:668-670`). Swarm-only
/// shutdown signal pre-formatted by the swarm coordinator.
#[derive(Debug, Default)]
pub struct TeammateShutdownBatchGenerator;

#[async_trait]
impl AttachmentGenerator for TeammateShutdownBatchGenerator {
    fn name(&self) -> &str {
        "TeammateShutdownBatchGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::TeammateShutdownBatch
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.teammate_shutdown_batch
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        let Some(body) = ctx.teammate_shutdown_batch.as_deref() else {
            return Ok(None);
        };
        if body.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::silent_text(
            AttachmentType::TeammateShutdownBatch,
            body.to_string(),
        )))
    }
}

// ── context_efficiency ─────────────────────────────────────────────────

/// TS `context_efficiency` (`attachments.ts:675-676`,
/// `messages.ts:4150+`). A nudge to compact / snip when context is
/// approaching the limit but auto-compact isn't available.
///
/// **R1 status (intentionally dormant):** the TS counterpart is gated
/// behind `feature('HISTORY_SNIP')`. coco-rs does not port that
/// feature (see root CLAUDE.md "Compaction — three generic strategies
/// only"). `ctx.context_efficiency_signal` therefore stays `false`
/// forever and the generator never fires. Retained in the catalog so
/// the parity-test invariant
/// `all_attachment_type_variants_have_default_generator` continues
/// to hold without an enum migration; will become live the day a
/// HISTORY_SNIP-equivalent lands in coco-rs.
#[derive(Debug, Default)]
pub struct ContextEfficiencyGenerator;

#[async_trait]
impl AttachmentGenerator for ContextEfficiencyGenerator {
    fn name(&self) -> &str {
        "ContextEfficiencyGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::ContextEfficiency
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.context_efficiency
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.context_efficiency_signal {
            return Ok(None);
        }
        Ok(Some(SystemReminder::silent_text(
            AttachmentType::ContextEfficiency,
            "Context is approaching the model's effective window. Consider summarising or compacting older tool output before continuing — long-running tool calls may otherwise be truncated.".to_string(),
        )))
    }
}
