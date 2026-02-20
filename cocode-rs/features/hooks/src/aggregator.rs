//! Hook aggregator for combining hooks from multiple sources.
//!
//! The aggregator collects hooks from different sources (policy, plugins, session, skills)
//! and produces a properly prioritized list for execution.

use crate::definition::HookDefinition;
use crate::scope::HookScope;
use crate::scope::HookSource;
use crate::settings::HookSettings;

/// Aggregates hooks from multiple sources into a single prioritized collection.
///
/// This struct handles:
/// - Setting the source field on hooks
/// - Filtering hooks based on `allow_managed_hooks_only` setting
/// - Ordering hooks by scope priority (Policy > Session > Agent > Skill > Plugin)
#[derive(Debug, Default)]
pub struct HookAggregator {
    hooks: Vec<HookDefinition>,
}

impl HookAggregator {
    /// Creates a new empty aggregator.
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Adds hooks from a policy source.
    pub fn add_policy_hooks(&mut self, hooks: impl IntoIterator<Item = HookDefinition>) {
        for mut hook in hooks {
            hook.source = HookSource::Policy;
            self.hooks.push(hook);
        }
    }

    /// Adds hooks from a plugin.
    pub fn add_plugin_hooks(
        &mut self,
        plugin_name: impl Into<String>,
        hooks: impl IntoIterator<Item = HookDefinition>,
    ) {
        let name = plugin_name.into();
        for mut hook in hooks {
            hook.source = HookSource::Plugin { name: name.clone() };
            self.hooks.push(hook);
        }
    }

    /// Adds hooks for the current session.
    pub fn add_session_hooks(&mut self, hooks: impl IntoIterator<Item = HookDefinition>) {
        for mut hook in hooks {
            hook.source = HookSource::Session;
            self.hooks.push(hook);
        }
    }

    /// Adds hooks from an agent or subagent.
    pub fn add_agent_hooks(
        &mut self,
        agent_name: impl Into<String>,
        hooks: impl IntoIterator<Item = HookDefinition>,
    ) {
        let name = agent_name.into();
        for mut hook in hooks {
            hook.source = HookSource::Agent { name: name.clone() };
            self.hooks.push(hook);
        }
    }

    /// Adds hooks from a skill.
    pub fn add_skill_hooks(
        &mut self,
        skill_name: impl Into<String>,
        hooks: impl IntoIterator<Item = HookDefinition>,
    ) {
        let name = skill_name.into();
        for mut hook in hooks {
            hook.source = HookSource::Skill { name: name.clone() };
            self.hooks.push(hook);
        }
    }

    /// Builds the aggregated hooks, applying settings and sorting by priority.
    ///
    /// When `settings.allow_managed_hooks_only` is true, only Policy and Plugin hooks
    /// are included. Hooks are sorted by scope priority (Policy first, Plugin last).
    pub fn build(mut self, settings: &HookSettings) -> Vec<HookDefinition> {
        // If all hooks are disabled, return empty
        if settings.disable_all_hooks {
            return Vec::new();
        }

        // Filter by managed-only setting
        if settings.allow_managed_hooks_only {
            self.hooks.retain(|h| h.source.is_managed());
        }

        // Sort by scope priority (lower scope value = higher priority)
        self.hooks.sort_by_key(|h| h.source.scope());

        self.hooks
    }

    /// Returns the number of hooks currently aggregated (before filtering).
    pub fn len(&self) -> usize {
        self.hooks.len()
    }

    /// Returns true if no hooks have been added.
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// Returns hooks grouped by scope.
    pub fn hooks_by_scope(&self) -> Vec<(HookScope, Vec<&HookDefinition>)> {
        let mut policy = Vec::new();
        let mut plugin = Vec::new();
        let mut session = Vec::new();
        let mut agent = Vec::new();
        let mut skill = Vec::new();

        for hook in &self.hooks {
            match hook.source.scope() {
                HookScope::Policy => policy.push(hook),
                HookScope::Plugin => plugin.push(hook),
                HookScope::Session => session.push(hook),
                HookScope::Agent => agent.push(hook),
                HookScope::Skill => skill.push(hook),
            }
        }

        [
            (HookScope::Policy, policy),
            (HookScope::Plugin, plugin),
            (HookScope::Session, session),
            (HookScope::Agent, agent),
            (HookScope::Skill, skill),
        ]
        .into_iter()
        .filter(|(_, hooks)| !hooks.is_empty())
        .collect()
    }
}

/// Helper to aggregate hooks from all sources at once.
pub fn aggregate_hooks(
    policy_hooks: impl IntoIterator<Item = HookDefinition>,
    plugin_hooks: impl IntoIterator<Item = (String, Vec<HookDefinition>)>,
    session_hooks: impl IntoIterator<Item = HookDefinition>,
    skill_hooks: impl IntoIterator<Item = (String, Vec<HookDefinition>)>,
    settings: &HookSettings,
) -> Vec<HookDefinition> {
    let mut aggregator = HookAggregator::new();

    aggregator.add_policy_hooks(policy_hooks);

    for (name, hooks) in plugin_hooks {
        aggregator.add_plugin_hooks(name, hooks);
    }

    aggregator.add_session_hooks(session_hooks);

    for (name, hooks) in skill_hooks {
        aggregator.add_skill_hooks(name, hooks);
    }

    aggregator.build(settings)
}

#[cfg(test)]
#[path = "aggregator.test.rs"]
mod tests;
