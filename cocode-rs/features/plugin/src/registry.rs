//! Plugin registry for runtime plugin management.
//!
//! The registry tracks loaded plugins and provides access to their contributions.

use crate::command::PluginCommand;
use crate::contribution::PluginContribution;
use crate::error::Result;
use crate::error::plugin_error::AlreadyRegisteredSnafu;
use crate::loader::LoadedPlugin;
use crate::lsp_loader::LspServerConfig;
use crate::mcp::McpServerConfig;
use crate::scope::PluginScope;

use cocode_hooks::HookDefinition;
use cocode_hooks::HookRegistry;
use cocode_hooks::HookSource;
use cocode_skill::CommandType;
use cocode_skill::LoadedFrom;
use cocode_skill::SkillContext;
use cocode_skill::SkillManager;
use cocode_skill::SkillPromptCommand;
use cocode_skill::SkillSource;
use cocode_subagent::AgentDefinition;
use cocode_subagent::SubagentManager;
use std::collections::HashMap;
use tracing::debug;
use tracing::info;

/// Generates a typed accessor that collects all contributions matching a
/// specific `PluginContribution` variant.
macro_rules! contribution_accessor {
    ($method:ident, $variant:ident, $inner_type:ty, $field:ident) => {
        pub fn $method(&self) -> Vec<(&$inner_type, &str)> {
            self.plugins
                .values()
                .flat_map(|plugin| {
                    plugin.contributions.iter().filter_map(|c| {
                        if let PluginContribution::$variant {
                            $field,
                            plugin_name,
                        } = c
                        {
                            Some(($field, plugin_name.as_str()))
                        } else {
                            None
                        }
                    })
                })
                .collect()
        }
    };
}

/// Registry for managing loaded plugins.
///
/// The registry tracks plugins and provides access to their contributions.
/// It can also integrate with the skill manager and hook registry.
#[derive(Debug, Default)]
pub struct PluginRegistry {
    /// Loaded plugins indexed by name.
    plugins: HashMap<String, LoadedPlugin>,
}

/// Count name occurrences and register items with both namespaced and
/// optional unambiguous alias names.
fn register_namespaced<T>(
    items: &[(&T, &str)],
    get_name: impl Fn(&T) -> &str,
    mut register_fn: impl FnMut(&T, &str, &str, bool),
) {
    let mut name_counts: HashMap<String, usize> = HashMap::new();
    for &(item, _) in items {
        *name_counts.entry(get_name(item).to_string()).or_default() += 1;
    }

    for &(item, plugin_name) in items {
        let name = get_name(item);
        let namespaced_name = format!("{plugin_name}:{name}");

        // Register with namespaced name
        register_fn(item, plugin_name, &namespaced_name, false);

        // Register with original name if unambiguous
        let is_ambiguous = name_counts.get(name).copied().unwrap_or(0) > 1;
        if !is_ambiguous {
            register_fn(item, plugin_name, name, true);
        } else {
            debug!(
                name = %name,
                plugin = %plugin_name,
                "Skipping ambiguous alias (multiple plugins provide this name)"
            );
        }
    }
}

impl PluginRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a loaded plugin.
    ///
    /// If a plugin with the same name already exists, the higher-priority
    /// scope wins (`PluginScope::priority()`). Returns an error only when
    /// the incoming plugin has equal or lower priority.
    pub fn register(&mut self, plugin: LoadedPlugin) -> Result<()> {
        let name = plugin.name().to_string();

        if let Some(existing) = self.plugins.get(&name) {
            if plugin.scope.priority() > existing.scope.priority() {
                debug!(
                    name = %name,
                    new_scope = %plugin.scope,
                    old_scope = %existing.scope,
                    "Replacing plugin with higher-priority scope"
                );
                self.plugins.insert(name, plugin);
                return Ok(());
            }
            return Err(AlreadyRegisteredSnafu { name }.build());
        }

        debug!(
            name = %name,
            scope = %plugin.scope,
            contributions = plugin.contributions.len(),
            "Registered plugin"
        );

        self.plugins.insert(name, plugin);
        Ok(())
    }

    /// Register multiple plugins.
    ///
    /// Plugins are sorted by ascending scope priority before registration,
    /// so higher-priority scopes naturally override lower ones.
    /// Same-priority duplicates are skipped with a warning.
    pub fn register_all(&mut self, mut plugins: Vec<LoadedPlugin>) {
        plugins.sort_by_key(|p| p.scope.priority());
        for plugin in plugins {
            if let Err(e) = self.register(plugin) {
                tracing::warn!(error = %e, "Skipping duplicate plugin");
            }
        }
    }

    /// Unregister a plugin by name.
    pub fn unregister(&mut self, name: &str) -> Option<LoadedPlugin> {
        self.plugins.remove(name)
    }

    /// Get a plugin by name.
    pub fn get(&self, name: &str) -> Option<&LoadedPlugin> {
        self.plugins.get(name)
    }

    /// Check if a plugin is registered.
    pub fn has(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Get all plugin names.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self.plugins.keys().map(String::as_str).collect();
        names.sort();
        names
    }

    /// Get all plugins.
    pub fn all(&self) -> impl Iterator<Item = &LoadedPlugin> {
        self.plugins.values()
    }

    /// Get the number of registered plugins.
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    contribution_accessor!(skill_contributions, Skill, SkillPromptCommand, skill);
    contribution_accessor!(hook_contributions, Hook, HookDefinition, hook);
    contribution_accessor!(agent_contributions, Agent, AgentDefinition, definition);
    contribution_accessor!(command_contributions, Command, PluginCommand, command);
    contribution_accessor!(mcp_server_contributions, McpServer, McpServerConfig, config);
    contribution_accessor!(lsp_server_contributions, LspServer, LspServerConfig, config);
    contribution_accessor!(
        output_style_contributions,
        OutputStyle,
        crate::contribution::OutputStyleDefinition,
        style
    );

    /// Apply all skill contributions to a skill manager.
    ///
    /// Skills are registered with a namespaced name (`plugin_name:skill_name`).
    /// If the original skill name is unambiguous (no other plugin provides a
    /// skill with the same name), it's also registered as an alias.
    pub fn apply_skills_to(&self, manager: &mut SkillManager) {
        let skills = self.skill_contributions();
        let count = skills.len();

        register_namespaced(
            &skills,
            |skill| &skill.name,
            |skill, plugin_name, reg_name, is_alias| {
                let mut s = skill.clone();
                s.name = reg_name.to_string();
                s.source = SkillSource::Plugin {
                    plugin_name: plugin_name.to_string(),
                };
                s.loaded_from = LoadedFrom::Plugin;
                if is_alias {
                    debug!(skill = %reg_name, plugin = %plugin_name, "Registering unambiguous skill alias");
                } else {
                    debug!(skill = %reg_name, plugin = %plugin_name, "Applying namespaced skill from plugin");
                }
                manager.register(s);
            },
        );

        if count > 0 {
            info!(count = count, "Applied skills from plugins");
        }
    }

    /// Apply all hook contributions to a hook registry.
    pub fn apply_hooks_to(&self, registry: &HookRegistry) {
        let hooks = self.hook_contributions();
        let count = hooks.len();

        for (hook, plugin_name) in hooks {
            let mut hook = hook.clone();
            hook.source = HookSource::Plugin {
                name: plugin_name.to_string(),
            };
            debug!(
                hook = %hook.name,
                plugin = %plugin_name,
                "Applying hook from plugin"
            );
            registry.register(hook);
        }

        if count > 0 {
            info!(count = count, "Applied hooks from plugins");
        }
    }

    /// Apply all agent contributions to a subagent manager.
    ///
    /// Agents are registered with a namespaced name (`plugin_name:agent_name`).
    /// If the original agent name is unambiguous (no other plugin provides an
    /// agent with the same name), it's also registered as an alias.
    pub fn apply_agents_to(&self, manager: &mut SubagentManager) {
        let agents = self.agent_contributions();
        let count = agents.len();

        register_namespaced(
            &agents,
            |def| &def.name,
            |definition, plugin_name, reg_name, is_alias| {
                let mut d = definition.clone();
                d.name = reg_name.to_string();
                if !is_alias {
                    d.agent_type = reg_name.to_string();
                }
                d.source = cocode_subagent::AgentSource::Plugin;
                if is_alias {
                    debug!(agent = %reg_name, plugin = %plugin_name, "Registering unambiguous agent alias");
                } else {
                    debug!(agent = %reg_name, plugin = %plugin_name, "Applying namespaced agent from plugin");
                }
                manager.register_agent_type(d);
            },
        );

        if count > 0 {
            info!(count = count, "Applied agents from plugins");
        }
    }

    /// Apply command contributions to skill manager and subagent manager.
    ///
    /// Commands with `Skill` handlers are registered as skills.
    /// Commands with `Agent` handlers register agent definitions in the
    /// subagent manager (if provided).
    pub fn apply_commands_to(
        &self,
        skill_manager: &mut SkillManager,
        mut subagent_manager: Option<&mut SubagentManager>,
    ) {
        use crate::command::CommandHandler;

        let commands = self.command_contributions();
        if commands.is_empty() {
            return;
        }

        let mut skills_added = 0;
        let mut agents_added = 0;

        for (cmd, plugin_name) in commands {
            match &cmd.handler {
                CommandHandler::Skill { skill_name } => {
                    // Look up the skill by name; if it exists, register a command alias
                    if let Some(skill) = skill_manager.get(skill_name) {
                        let mut alias = skill.clone();
                        alias.name = cmd.name.clone();
                        alias.description = cmd.description.clone();
                        skill_manager.register(alias);
                        skills_added += 1;
                    } else {
                        debug!(
                            command = %cmd.name,
                            skill = %skill_name,
                            plugin = %plugin_name,
                            "Command references unknown skill"
                        );
                    }
                }
                CommandHandler::Agent { agent_type } => {
                    if let Some(ref mut manager) = subagent_manager {
                        let definition = AgentDefinition {
                            name: cmd.name.clone(),
                            description: cmd.description.clone(),
                            agent_type: agent_type.clone(),
                            tools: Vec::new(),
                            disallowed_tools: Vec::new(),
                            identity: None,
                            max_turns: None,
                            permission_mode: None,
                            fork_context: false,
                            color: None,
                            critical_reminder: None,
                            source: cocode_subagent::AgentSource::Plugin,
                            skills: Vec::new(),
                            background: false,
                            memory: None,
                            hooks: None,
                            mcp_servers: None,
                            isolation: None,
                            use_custom_prompt: false,
                        };
                        manager.register_agent_type(definition);
                        agents_added += 1;
                    }
                }
                CommandHandler::Shell { command, .. } => {
                    let prompt =
                        format!("Execute the following shell command:\n```\n{command}\n```");
                    let skill = SkillPromptCommand {
                        name: cmd.name.clone(),
                        description: cmd.description.clone(),
                        prompt,
                        allowed_tools: Some(vec![
                            cocode_protocol::ToolName::Bash.as_str().to_string(),
                        ]),
                        user_invocable: cmd.visible,
                        disable_model_invocation: false,
                        is_hidden: !cmd.visible,
                        source: SkillSource::Plugin {
                            plugin_name: plugin_name.to_string(),
                        },
                        loaded_from: LoadedFrom::Plugin,
                        context: SkillContext::Main,
                        agent: None,
                        model: None,
                        base_dir: None,
                        when_to_use: None,
                        argument_hint: None,
                        aliases: Vec::new(),
                        version: None,
                        arguments: None,
                        paths: None,
                        interface: None,
                        command_type: CommandType::Prompt,
                    };
                    skill_manager.register(skill);
                    skills_added += 1;
                }
            }
        }

        if skills_added > 0 || agents_added > 0 {
            info!(
                skills = skills_added,
                agents = agents_added,
                "Applied commands from plugins"
            );
        }
    }

    /// Get plugins by scope.
    pub fn by_scope(&self, scope: PluginScope) -> Vec<&LoadedPlugin> {
        self.plugins.values().filter(|p| p.scope == scope).collect()
    }

    /// Clear all registered plugins.
    pub fn clear(&mut self) {
        self.plugins.clear();
    }
}

#[cfg(test)]
#[path = "registry.test.rs"]
mod tests;
