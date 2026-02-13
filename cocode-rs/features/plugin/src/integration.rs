//! Plugin integration with runtime components.
//!
//! This module provides the entry point for integrating plugins with the
//! session runtime (SkillManager, HookRegistry, SubagentManager).

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cocode_hooks::HookRegistry;
use cocode_rmcp_client::RmcpClient;
use cocode_skill::SkillManager;
use cocode_subagent::SubagentManager;
use cocode_tools::ToolRegistry;
use tracing::info;
use tracing::warn;

use crate::installed_registry::InstalledPluginsRegistry;
use crate::loader::load_plugins_from_roots;
use crate::mcp::McpTransport;
use crate::plugin_settings::PluginSettings;
use crate::registry::PluginRegistry;
use crate::scope::PluginScope;

/// Configuration for plugin integration.
#[derive(Debug, Clone, Default)]
pub struct PluginIntegrationConfig {
    /// Directory for managed (system-installed) plugins.
    pub managed_dir: Option<PathBuf>,

    /// Directory for user-global plugins.
    pub user_dir: Option<PathBuf>,

    /// Directory for project-local plugins.
    pub project_dir: Option<PathBuf>,

    /// Root plugins directory for installed plugins (`~/.cocode/plugins/`).
    pub plugins_dir: Option<PathBuf>,

    /// Extra plugin directories from `--plugin-dir` flags (Flag scope).
    pub inline_dirs: Vec<PathBuf>,
}

impl PluginIntegrationConfig {
    /// Create a new configuration with default directories.
    ///
    /// - User directory: `~/.cocode/plugins/`
    /// - Project directory: `.cocode/plugins/`
    /// - Plugins directory: `~/.cocode/plugins/`
    pub fn with_defaults(
        cocode_home: &std::path::Path,
        project_root: Option<&std::path::Path>,
    ) -> Self {
        let user_dir = Some(cocode_home.join("plugins"));

        let project_dir = project_root.map(|p| p.join(".cocode").join("plugins"));

        let plugins_dir = Some(cocode_home.join("plugins"));

        Self {
            managed_dir: None,
            user_dir,
            project_dir,
            plugins_dir,
            inline_dirs: Vec::new(),
        }
    }

    /// Set the managed plugin directory.
    pub fn with_managed_dir(mut self, dir: PathBuf) -> Self {
        self.managed_dir = Some(dir);
        self
    }

    /// Set the user plugin directory.
    pub fn with_user_dir(mut self, dir: PathBuf) -> Self {
        self.user_dir = Some(dir);
        self
    }

    /// Set the project plugin directory.
    pub fn with_project_dir(mut self, dir: PathBuf) -> Self {
        self.project_dir = Some(dir);
        self
    }

    /// Set the plugins directory for installed plugins.
    pub fn with_plugins_dir(mut self, dir: PathBuf) -> Self {
        self.plugins_dir = Some(dir);
        self
    }

    /// Add inline plugin directories (Flag scope).
    pub fn with_inline_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.inline_dirs = dirs;
        self
    }

    /// Build the list of plugin roots with their scopes.
    fn plugin_roots(&self) -> Vec<(PathBuf, PluginScope)> {
        let mut roots = Vec::new();

        if let Some(dir) = &self.managed_dir {
            roots.push((dir.clone(), PluginScope::Managed));
        }
        if let Some(dir) = &self.user_dir {
            roots.push((dir.clone(), PluginScope::User));
        }
        if let Some(dir) = &self.project_dir {
            roots.push((dir.clone(), PluginScope::Project));
        }

        // Add installed plugins from cache
        if let Some(plugins_dir) = &self.plugins_dir {
            let registry_path = plugins_dir.join("installed_plugins.json");
            let settings_path = plugins_dir.join("settings.json");

            let registry = InstalledPluginsRegistry::load(&registry_path);
            let settings = PluginSettings::load(&settings_path);

            for (plugin_id, entries) in &registry.plugins {
                if !settings.is_enabled(plugin_id) {
                    continue;
                }
                for entry in entries {
                    if entry.install_path.exists() {
                        let scope = match entry.scope.as_str() {
                            "managed" => PluginScope::Managed,
                            "project" => PluginScope::Project,
                            "local" => PluginScope::Local,
                            "flag" => PluginScope::Flag,
                            _ => PluginScope::User,
                        };
                        roots.push((entry.install_path.clone(), scope));
                    }
                }
            }
        }

        // Add inline dirs (Flag scope)
        for dir in &self.inline_dirs {
            roots.push((dir.clone(), PluginScope::Flag));
        }

        roots
    }
}

/// Integrate plugins with runtime components.
///
/// This function:
/// 1. Discovers plugins from configured directories
/// 2. Loads installed plugins from cache (enabled only)
/// 3. Loads plugins from inline dirs
/// 4. Applies skills to the skill manager
/// 5. Applies hooks to the hook registry
/// 6. Applies agents to the subagent manager (if provided)
/// 7. Applies command contributions (skill/agent commands)
///
/// Returns the populated plugin registry.
///
/// MCP server contributions are *not* connected here because they require
/// async I/O. Use [`connect_plugin_mcp_servers`] after this call to start
/// plugin MCP servers and register their tools.
pub fn integrate_plugins(
    config: &PluginIntegrationConfig,
    skill_manager: &mut SkillManager,
    hook_registry: &HookRegistry,
    subagent_manager: Option<&mut SubagentManager>,
) -> PluginRegistry {
    let roots = config.plugin_roots();

    info!(
        roots = roots.len(),
        managed = config.managed_dir.is_some(),
        user = config.user_dir.is_some(),
        project = config.project_dir.is_some(),
        installed = config.plugins_dir.is_some(),
        inline = config.inline_dirs.len(),
        "Integrating plugins"
    );

    // Load all plugins
    let plugins = load_plugins_from_roots(&roots);

    // Build registry
    let mut registry = PluginRegistry::new();
    registry.register_all(plugins);

    // Apply contributions
    registry.apply_skills_to(skill_manager);
    registry.apply_hooks_to(hook_registry);

    if let Some(manager) = subagent_manager {
        registry.apply_agents_to(manager);
        registry.apply_commands_to(skill_manager, Some(manager));
    } else {
        registry.apply_commands_to(skill_manager, None);
    }

    info!(
        plugins = registry.len(),
        skills = registry.skill_contributions().len(),
        hooks = registry.hook_contributions().len(),
        agents = registry.agent_contributions().len(),
        commands = registry.command_contributions().len(),
        mcp_servers = registry.mcp_server_contributions().len(),
        "Plugin integration complete"
    );

    registry
}

/// Load plugins without applying to runtime components.
///
/// Use this when you need to inspect plugins before integration.
pub fn load_plugins(config: &PluginIntegrationConfig) -> PluginRegistry {
    let roots = config.plugin_roots();
    let plugins = load_plugins_from_roots(&roots);

    let mut registry = PluginRegistry::new();
    registry.register_all(plugins);

    registry
}

/// Connect plugin MCP servers to the tool registry.
///
/// For each auto-start MCP server contribution from plugins, starts the server
/// process and registers its tools in the tool registry. Returns the active
/// MCP clients so the caller can keep them alive for the session lifetime.
pub async fn connect_plugin_mcp_servers(
    registry: &PluginRegistry,
    tool_registry: &mut ToolRegistry,
    cocode_home: &std::path::Path,
) -> Vec<Arc<RmcpClient>> {
    let servers = registry.mcp_server_contributions();
    let mut clients = Vec::new();

    for (config, plugin_name) in servers {
        if !config.auto_start {
            continue;
        }
        match connect_mcp_server(config, plugin_name, tool_registry, cocode_home).await {
            Ok(client) => clients.push(client),
            Err(e) => {
                warn!(
                    server = %config.name,
                    plugin = %plugin_name,
                    error = %e,
                    "Failed to connect plugin MCP server"
                );
            }
        }
    }

    if !clients.is_empty() {
        info!(count = clients.len(), "Connected plugin MCP servers");
    }

    clients
}

async fn connect_mcp_server(
    config: &crate::mcp::McpServerConfig,
    plugin_name: &str,
    tool_registry: &mut ToolRegistry,
    cocode_home: &std::path::Path,
) -> anyhow::Result<Arc<RmcpClient>> {
    use cocode_mcp_types::ClientCapabilities;
    use cocode_mcp_types::Implementation;
    use cocode_mcp_types::InitializeRequestParams;
    use cocode_mcp_types::MCP_SCHEMA_VERSION;
    use futures::FutureExt as _;

    info!(
        server = %config.name,
        plugin = %plugin_name,
        "Connecting plugin MCP server"
    );

    let client = match &config.transport {
        McpTransport::Stdio { command, args } => Arc::new(
            RmcpClient::new_stdio_client(
                OsString::from(command),
                args.iter().map(OsString::from).collect(),
                Some(config.env.clone()),
                &[],
                None,
            )
            .await?,
        ),
        McpTransport::Http { url } => Arc::new(
            RmcpClient::new_streamable_http_client(
                &config.name,
                url,
                None,
                None,
                None,
                Default::default(),
                cocode_home.to_path_buf(),
            )
            .await?,
        ),
    };

    // Initialize the MCP server
    let init_params = InitializeRequestParams {
        capabilities: ClientCapabilities {
            elicitation: None,
            experimental: None,
            roots: None,
            sampling: None,
        },
        client_info: Implementation {
            name: "cocode-plugin".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            title: Some(format!("cocode plugin: {plugin_name}")),
            user_agent: None,
        },
        protocol_version: MCP_SCHEMA_VERSION.to_string(),
    };

    let no_elicitation: cocode_rmcp_client::SendElicitation = Box::new(|_, _| {
        async {
            Err(anyhow::anyhow!(
                "Elicitation not supported for plugin MCP servers"
            ))
        }
        .boxed()
    });

    client
        .initialize(init_params, Some(Duration::from_secs(30)), no_elicitation)
        .await?;

    // List and register tools
    let tools_result = client
        .list_tools(None, Some(Duration::from_secs(30)))
        .await?;
    let tools_count = tools_result.tools.len();

    tool_registry.register_mcp_tools_executable(
        &config.name,
        tools_result.tools,
        client.clone(),
        Duration::from_secs(60),
    );

    info!(
        server = %config.name,
        plugin = %plugin_name,
        tools = tools_count,
        "Connected plugin MCP server"
    );

    Ok(client)
}

#[cfg(test)]
#[path = "integration.test.rs"]
mod tests;
