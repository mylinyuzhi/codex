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
use coco_types::DynamicSkillPayload;
use coco_types::SkillDiscoveryPayload;
use coco_types::SkillDiscoverySkill;
use coco_types::SkillDiscoverySource;

use crate::SkillManager;

const MAX_SKILL_DISCOVERY_DESCRIPTION_CHARS: usize = 500;

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out = s.chars().take(max.saturating_sub(3)).collect::<String>();
    out.push_str("...");
    out
}

#[async_trait]
impl SkillsSource for SkillManager {
    async fn listing(&self, agent_id: Option<&str>) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        // Build the canonical sorted list once for stable order.
        // Bind the owned Vec first — `SkillManager::all()` returns owned
        // values (MCP-sourced skills live behind a Mutex), so any `&str`
        // borrows must live no longer than this binding.
        let owned_skills = self.all();
        let mut entries: Vec<(&str, &str)> = owned_skills
            .iter()
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

    /// **TS-divergent**: this is a local keyword-match heuristic, not the
    /// Haiku-class LLM call TS uses in `services/skillSearch/prefetch.ts`.
    /// Returns matches keyed by case-insensitive substring on name (split
    /// on `- _`) plus 5+-char word overlap with description and
    /// `when_to_use`. Sorted alphabetically and capped at 5.
    ///
    /// Signal is stamped `"local_keyword_match"` (not a `DiscoverySignal`
    /// enum value from TS) so downstream telemetry can tell the heuristic
    /// from a future LLM-backed producer. When the LLM-backed path lands,
    /// swap this implementation and update the signal value.
    async fn skill_discovery(&self, user_input: &str) -> Option<SkillDiscoveryPayload> {
        let needle = user_input.to_lowercase();
        let mut matches: Vec<_> = self
            .all()
            .into_iter()
            .filter(|s| !s.disabled)
            .filter(|s| {
                let name = s.name.to_lowercase();
                let desc = s.description.to_lowercase();
                let when = s.when_to_use.as_deref().unwrap_or("").to_lowercase();
                !needle.trim().is_empty()
                    && (needle.contains(&name)
                        || name.split(['-', '_']).any(|part| needle.contains(part))
                        || desc.split_whitespace().any(|word| {
                            word.len() >= 5
                                && needle
                                    .contains(word.trim_matches(|c: char| !c.is_alphanumeric()))
                        })
                        || when.split_whitespace().any(|word| {
                            word.len() >= 5
                                && needle
                                    .contains(word.trim_matches(|c: char| !c.is_alphanumeric()))
                        }))
            })
            .collect();
        matches.sort_by(|a, b| a.name.cmp(&b.name));
        matches.truncate(5);
        if matches.is_empty() {
            return None;
        }
        let skills = matches
            .iter()
            .map(|s| SkillDiscoverySkill {
                name: s.name.clone(),
                description: truncate_chars(&s.description, MAX_SKILL_DISCOVERY_DESCRIPTION_CHARS),
                short_id: None,
            })
            .collect();
        Some(SkillDiscoveryPayload {
            skills,
            signal: "local_keyword_match".to_string(),
            source: SkillDiscoverySource::Native,
        })
    }

    async fn activate_skills_for_paths(
        &self,
        file_paths: &[std::path::PathBuf],
        cwd: &std::path::Path,
    ) -> Vec<String> {
        self.activate_for_paths(file_paths, cwd)
    }

    async fn load_dynamic_skill_dir(
        &self,
        skill_dir: &std::path::Path,
        cwd: &std::path::Path,
    ) -> Option<DynamicSkillPayload> {
        let mut skills = crate::discover_skills(&[skill_dir.to_path_buf()]);
        if skills.is_empty() {
            return None;
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        let skill_names = skills
            .iter()
            .filter(|s| !s.disabled)
            .map(|s| s.name.clone())
            .collect::<Vec<_>>();
        if skill_names.is_empty() {
            return None;
        }
        for skill in skills {
            self.register(skill);
        }
        let display_path = skill_dir
            .strip_prefix(cwd)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| skill_dir.display().to_string());
        Some(DynamicSkillPayload {
            skill_dir: skill_dir.display().to_string(),
            skill_names,
            display_path,
        })
    }
}

#[cfg(test)]
#[path = "reminder_source.test.rs"]
mod tests;
