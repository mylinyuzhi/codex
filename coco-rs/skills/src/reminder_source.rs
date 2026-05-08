//! `SkillsSource` impl on [`crate::SkillManager`].
//!
//! `listing()` renders the full skill catalog as a bullet list of
//! `- name: description` entries — matches the TS `skill_listing`
//! attachment shape well enough for the model to parse; exact-verbatim
//! format of TS `generateSkillToolPrompt()` (1% context budget, 250-char
//! description cap, bundled never truncated) is out of scope for the
//! per-turn reminder path and stays in `skills::generate_skill_tool_prompt`
//! for the static system-prompt injection.
//!
//! `invoked()` returns empty by default — tracking which skills were
//! invoked this session requires per-CLI / per-QueryEngine state that
//! is wired via a separate subsystem (follow-up work).

use async_trait::async_trait;
use coco_system_reminder::InvokedSkillEntry;
use coco_system_reminder::SkillsSource;

use crate::SkillManager;

#[async_trait]
impl SkillsSource for SkillManager {
    async fn listing(&self, agent_id: Option<&str>) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        // Build the canonical sorted list once for stable order.
        let mut entries: Vec<(&str, &str)> = self
            .all()
            .map(|s| (s.name.as_str(), s.description.as_str()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let names: Vec<&str> = entries.iter().map(|(n, _)| *n).collect();

        // TS `attachments.ts:2718-2730`: only announce skills the agent
        // has not seen yet. Returns `None` once everything is announced
        // so subsequent turns skip the redundant injection.
        let (delta, _is_initial) = self.take_unannounced_skills(agent_id, &names);
        if delta.is_empty() {
            return None;
        }
        let delta_set: std::collections::HashSet<&str> = delta.iter().map(String::as_str).collect();
        let body = entries
            .iter()
            .filter(|(name, _)| delta_set.contains(*name))
            .map(|(name, desc)| {
                if desc.is_empty() {
                    format!("- {name}")
                } else {
                    format!("- {name}: {desc}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        Some(body)
    }

    async fn invoked(&self, _agent_id: Option<&str>) -> Vec<InvokedSkillEntry> {
        // SkillManager doesn't track invocation state — that's per-session
        // tracking owned by the CLI / session layer. Return empty for now;
        // future work: plumb `InvokedSkillsTracker` through a separate
        // trait impl on a wrapper that combines `SkillManager` + tracker.
        Vec::new()
    }
}

#[cfg(test)]
#[path = "reminder_source.test.rs"]
mod tests;
