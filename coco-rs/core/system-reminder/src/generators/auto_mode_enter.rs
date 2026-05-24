//! TS `auto_mode` steady-state generator.
//!
//! Full/Sparse cadence symmetric to [`PlanModeEnterGenerator`] but gated on
//! `ctx.is_auto_mode` (engine flag covering both `mode == 'auto'` and
//! `mode == 'plan'` with the auto-mode classifier active — TS
//! `attachments.ts:1341-1344`).
//!
//! Text verbatim from TS:
//! - Full:   `messages.ts:3428-3438` (the 6-point "Auto Mode Active" block)
//! - Sparse: `messages.ts:3446` (the one-line "still active" reminder)
//!
//! Auto-mode exit is a separate one-shot generator
//! ([`super::AutoModeExitGenerator`]); this file handles entry + steady-state
//! only.

use async_trait::async_trait;

use crate::error::Result;
use crate::generator::AttachmentGenerator;
use crate::generator::GeneratorContext;
use crate::throttle::ThrottleConfig;
use crate::types::AttachmentType;
use crate::types::SystemReminder;
use coco_config::SystemReminderConfig;

/// TS `messages.ts:3428-3438` — full auto-mode instructions.
const AUTO_MODE_FULL: &str = "## Auto Mode Active

Auto mode is active. The user chose continuous, autonomous execution. You should:

1. **Execute immediately** — Start implementing right away. Make reasonable assumptions and proceed on low-risk work.
2. **Minimize interruptions** — Prefer making reasonable assumptions over asking questions for routine decisions.
3. **Prefer action over planning** — Do not enter plan mode unless the user explicitly asks. When in doubt, start coding.
4. **Expect course corrections** — The user may provide suggestions or course corrections at any point; treat those as normal input.
5. **Do not take overly destructive actions** — Auto mode is not a license to destroy. Anything that deletes data or modifies shared or production systems still needs explicit user confirmation. If you reach such a decision point, ask and wait, or course correct to a safer method instead.
6. **Avoid data exfiltration** — Post even routine messages to chat platforms or work tickets only if the user has directed you to. You must not share secrets (e.g. credentials, internal documentation) unless the user has explicitly authorized both that specific secret and its destination.";

/// TS `messages.ts:3446` — one-line sparse reminder.
const AUTO_MODE_SPARSE: &str = "Auto mode still active (see full instructions earlier in conversation). Execute autonomously, minimize interruptions, prefer action over planning.";

/// Steady-state "auto mode still active" reminder.
///
/// Cadence: [`ThrottleConfig::auto_mode`] → 5 turns between emissions,
/// Full on every 5th emission (first is Full).
#[derive(Debug, Default)]
pub struct AutoModeEnterGenerator;

#[async_trait]
impl AttachmentGenerator for AutoModeEnterGenerator {
    fn name(&self) -> &str {
        "AutoModeEnterGenerator"
    }

    fn attachment_type(&self) -> AttachmentType {
        AttachmentType::AutoMode
    }

    fn is_enabled(&self, config: &SystemReminderConfig) -> bool {
        config.attachments.auto_mode
    }

    fn throttle_config(&self) -> ThrottleConfig {
        ThrottleConfig::auto_mode()
    }

    async fn generate(&self, ctx: &GeneratorContext<'_>) -> Result<Option<SystemReminder>> {
        if !ctx.is_auto_mode {
            return Ok(None);
        }
        let content = if ctx.should_use_full_content(AttachmentType::AutoMode) {
            AUTO_MODE_FULL
        } else {
            AUTO_MODE_SPARSE
        };
        Ok(Some(SystemReminder::new(
            AttachmentType::AutoMode,
            content.to_string(),
        )))
    }
}

#[cfg(test)]
#[path = "auto_mode_enter.test.rs"]
mod tests;
