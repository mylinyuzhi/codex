//! Audit-add reminder generators (May 2026).
//!
//! Only `skill_discovery` is a model-visible reminder produced here.
//! API-hidden TS attachments (`command_permissions`, `dynamic_skill`,
//! `structured_output`, `max_turns_reached`) are emitted as typed
//! silent `AttachmentMessage`s by their owner crates via
//! `coco_messages::AttachmentEmitter`, drained by the engine inbox at
//! turn start. `teammate_shutdown_batch` is `RuntimeBookkeeping` until
//! the TUI collapse path lands.
//!
//! ## TS-divergence: `skill_discovery` uses keyword-match, not Haiku
//!
//! TS `services/skillSearch/prefetch.ts` runs a Haiku-class LLM call
//! against the user prompt + active skill catalog to suggest skills.
//! coco-rs ships a **local substring + word-prefix heuristic** in
//! `coco_skills::SkillManager::skill_discovery` because the TS-style
//! AKI service isn't ported yet. The payload shape matches TS exactly
//! (`SkillDiscoveryPayload { skills, signal, source }`) but:
//!
//! - `source = Native` (TS uses `Native | Aki | Both` — coco-rs has no
//!   AKI counterpart)
//! - `signal = "local_keyword_match"` (TS uses `DiscoverySignal` enum:
//!   `user_message`, `assistant_turn`, `write_pivot`, …)
//!
//! When/if the LLM-backed path ports over, swap the producer and update
//! the `signal` value to the TS enum. Until then, downstream consumers
//! that key on `signal` should treat the coco-rs value as opaque.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

// ── skill_discovery ────────────────────────────────────────────────────

/// TS `skill_discovery` (`attachments.ts:538-542`). UserPrompt-tier
/// heuristic skill suggestion. The source pre-renders the exact TS
/// prompt from `messages.ts`; empty candidate lists thread `None`.
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
        let Some(payload) = ctx.skill_discovery.clone() else {
            return Ok(None);
        };
        if payload.skills.is_empty() {
            return Ok(None);
        }
        Ok(Some(SystemReminder::skill_discovery(payload)))
    }
}
