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
use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::installed_registry::InstalledPluginsRegistry;
use crate::loader::load_plugins_from_roots;
use crate::marketplace_manager::MarketplaceManager;
use crate::marketplace_types::MarketplaceSource;
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

    /// Plugin enable/disable overrides from main config settings.
    ///
    /// Keys are `"pluginName"` or `"pluginName@marketplaceName"`.
    /// These are merged with (and override) the per-file plugin settings.
    pub config_enabled_plugins: std::collections::HashMap<String, bool>,

    /// Extra marketplace sources from project settings.
    ///
    /// Each entry is `(name, source, auto_update)`. Registered with the
    /// `MarketplaceManager` before loading plugins, so team-shared
    /// marketplaces are automatically available.
    pub extra_known_marketplaces: Vec<ExtraMarketplaceEntry>,
}

/// An extra marketplace source to register during plugin integration.
///
/// This is the plugin-crate representation of `ExtraMarketplaceConfig`
/// from the config crate (converted at the call site in session state).
#[derive(Debug, Clone)]
pub struct ExtraMarketplaceEntry {
    /// Display name for the marketplace.
    pub name: String,
    /// Source type and location.
    pub source: MarketplaceSource,
    /// Whether to automatically update this marketplace.
    pub auto_update: bool,
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
            config_enabled_plugins: std::collections::HashMap::new(),
            extra_known_marketplaces: Vec::new(),
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

    /// Set plugin enable/disable overrides from main config.
    pub fn with_config_enabled_plugins(
        mut self,
        enabled: std::collections::HashMap<String, bool>,
    ) -> Self {
        self.config_enabled_plugins = enabled;
        self
    }

    /// Set extra marketplace sources from project settings.
    pub fn with_extra_known_marketplaces(mut self, extras: Vec<ExtraMarketplaceEntry>) -> Self {
        self.extra_known_marketplaces = extras;
        self
    }

    /// Build the list of plugin roots with their scopes and the loaded settings.
    fn plugin_roots_and_settings(&self) -> (Vec<(PathBuf, PluginScope)>, PluginSettings) {
        let mut roots = Vec::new();
        let mut settings = PluginSettings::default();

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
            settings = PluginSettings::load(&settings_path);

            for (plugin_id, entries) in &registry.plugins {
                // Config-level overrides take priority over per-file settings
                let enabled = self
                    .config_enabled_plugins
                    .get(plugin_id)
                    .copied()
                    .unwrap_or_else(|| settings.is_enabled(plugin_id));
                if !enabled {
                    continue;
                }
                for entry in entries {
                    if entry.install_path.exists() {
                        let scope = entry
                            .scope
                            .parse::<PluginScope>()
                            .unwrap_or(PluginScope::User);
                        roots.push((entry.install_path.clone(), scope));
                    }
                }
            }
        }

        // Add inline dirs (Flag scope)
        for dir in &self.inline_dirs {
            roots.push((dir.clone(), PluginScope::Flag));
        }

        (roots, settings)
    }
}

/// Result of plugin integration.
#[derive(Debug)]
pub struct PluginIntegrationResult {
    /// The populated plugin registry.
    pub registry: PluginRegistry,

    /// Default agent name from a plugin's `settings.json`, if any.
    ///
    /// When a plugin specifies `"agent": "agent-name"` in its root
    /// `settings.json`, the session should activate that agent as the
    /// default thread agent.
    pub default_agent: Option<String>,
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
/// Returns the populated plugin registry and any agent override.
///
/// MCP server contributions are *not* connected here because they require
/// async I/O. Use [`connect_plugin_mcp_servers`] after this call to start
/// plugin MCP servers and register their tools.
pub fn integrate_plugins(
    config: &PluginIntegrationConfig,
    skill_manager: &mut SkillManager,
    hook_registry: &HookRegistry,
    subagent_manager: Option<&mut SubagentManager>,
) -> PluginIntegrationResult {
    // Register extra marketplaces from project settings (if any)
    if !config.extra_known_marketplaces.is_empty()
        && let Some(ref plugins_dir) = config.plugins_dir
    {
        let mm = MarketplaceManager::new(plugins_dir.clone());
        match mm.register_extra(&config.extra_known_marketplaces) {
            Ok(added) if added > 0 => {
                info!(added, "Extra marketplaces registered");
            }
            Err(e) => {
                warn!(error = %e, "Failed to register extra marketplaces");
            }
            _ => {}
        }
    }

    let (roots, settings) = config.plugin_roots_and_settings();

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
    let plugins = load_plugins_from_roots(&roots, &settings);

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

    // Check for default agent override from plugin settings.json
    let default_agent = registry
        .all()
        .filter_map(|p| p.settings.agent.clone())
        .next();
    if let Some(ref agent) = default_agent {
        info!(agent, "Plugin specifies default agent");
    }

    info!(
        plugins = registry.len(),
        skills = registry.skill_contributions().len(),
        hooks = registry.hook_contributions().len(),
        agents = registry.agent_contributions().len(),
        commands = registry.command_contributions().len(),
        mcp_servers = registry.mcp_server_contributions().len(),
        lsp_servers = registry.lsp_server_contributions().len(),
        output_styles = registry.output_style_contributions().len(),
        "Plugin integration complete"
    );

    PluginIntegrationResult {
        registry,
        default_agent,
    }
}

/// Load plugins without applying to runtime components.
///
/// Use this when you need to inspect plugins before integration.
pub fn load_plugins(config: &PluginIntegrationConfig) -> PluginRegistry {
    let (roots, settings) = config.plugin_roots_and_settings();
    let plugins = load_plugins_from_roots(&roots, &settings);

    let mut registry = PluginRegistry::new();
    registry.register_all(plugins);

    registry
}

/// Connect plugin MCP servers to the tool registry.
///
/// For each auto-start MCP server contribution from plugins, connects all
/// servers concurrently and then registers their tools sequentially.
/// Returns the active MCP clients so the caller can keep them alive for
/// the session lifetime.
pub async fn connect_plugin_mcp_servers(
    registry: &PluginRegistry,
    tool_registry: &mut ToolRegistry,
    cocode_home: &std::path::Path,
) -> Vec<Arc<RmcpClient>> {
    let servers = registry.mcp_server_contributions();
    let auto_start: Vec<_> = servers
        .into_iter()
        .filter(|(config, _)| config.auto_start)
        .collect();

    if auto_start.is_empty() {
        return Vec::new();
    }

    // Connect all servers concurrently (I/O-bound: process spawn + MCP handshake)
    let connection_futures: Vec<_> = auto_start
        .iter()
        .map(|&(config, plugin_name)| establish_mcp_connection(config, plugin_name, cocode_home))
        .collect();

    let results = futures::future::join_all(connection_futures).await;

    // Register tools sequentially (requires &mut ToolRegistry)
    let mut clients = Vec::new();
    for (result, &(config, plugin_name)) in results.into_iter().zip(auto_start.iter()) {
        match result {
            Ok((client, tools_result)) => {
                let tools_count = tools_result.tools.len();
                // Namespace MCP server name as plugin_{pluginName}_{serverName}
                // so tool names become mcp__plugin_myPlugin_serverName__toolName
                let namespaced = format!("plugin_{}_{}", plugin_name, config.name);
                tool_registry.register_mcp_tools_executable(
                    &namespaced,
                    tools_result.tools,
                    client.clone(),
                    Duration::from_secs(60),
                );
                info!(
                    server = %config.name,
                    namespaced = %namespaced,
                    plugin = %plugin_name,
                    tools = tools_count,
                    "Connected plugin MCP server"
                );
                clients.push(client);
            }
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

/// Establish an MCP connection to a plugin server.
///
/// Creates the client, initializes the MCP protocol, and lists available tools.
/// Tool registration is deferred to the caller since it requires `&mut ToolRegistry`.
async fn establish_mcp_connection(
    config: &crate::mcp::McpServerConfig,
    plugin_name: &str,
    cocode_home: &std::path::Path,
) -> crate::error::Result<(Arc<RmcpClient>, cocode_mcp_types::ListToolsResult)> {
    use crate::error::plugin_error::McpConnectionFailedSnafu;
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
            .await
            .map_err(|e| {
                McpConnectionFailedSnafu {
                    server: config.name.clone(),
                    message: e.to_string(),
                }
                .build()
            })?,
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
            .await
            .map_err(|e| {
                McpConnectionFailedSnafu {
                    server: config.name.clone(),
                    message: e.to_string(),
                }
                .build()
            })?,
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
        .await
        .map_err(|e| {
            McpConnectionFailedSnafu {
                server: config.name.clone(),
                message: e.to_string(),
            }
            .build()
        })?;

    // List available tools
    let tools_result = client
        .list_tools(None, Some(Duration::from_secs(30)))
        .await
        .map_err(|e| {
            McpConnectionFailedSnafu {
                server: config.name.clone(),
                message: e.to_string(),
            }
            .build()
        })?;

    Ok((client, tools_result))
}

/// Register plugin-contributed LSP servers with the LSP server manager.
///
/// For each LSP server contribution from plugins, converts the plugin config
/// to the LSP crate's format and merges it into the manager's configuration.
/// Servers start on-demand when matching file types are opened.
///
/// Call this after [`integrate_plugins`] when an `LspServerManager` is available.
pub async fn connect_plugin_lsp_servers(
    registry: &PluginRegistry,
    lsp_manager: &cocode_lsp::LspServerManager,
) {
    let servers = registry.lsp_server_contributions();
    if servers.is_empty() {
        return;
    }

    let mut lsp_config = cocode_lsp::LspServersConfig::default();

    for (config, plugin_name) in &servers {
        let server_id = format!("{plugin_name}-{}", config.name);
        // Normalize plugin file_patterns (e.g. "*.rs", "Cargo.toml") to
        // LSP file_extensions format (e.g. ".rs", ".toml").
        let file_extensions = normalize_file_patterns(&config.file_patterns);
        let lsp_server_config = cocode_lsp::LspServerConfig {
            command: Some(config.command.clone()),
            args: config.args.clone(),
            file_extensions,
            languages: config.languages.clone(),
            env: config.env.clone(),
            ..Default::default()
        };
        debug!(
            server = %server_id,
            plugin = %plugin_name,
            languages = ?config.languages,
            "Registering plugin LSP server"
        );
        lsp_config.servers.insert(server_id, lsp_server_config);
    }

    lsp_manager.merge_config(lsp_config).await;

    info!(count = servers.len(), "Connected plugin LSP servers");
}

/// Normalize plugin file patterns to LSP file extensions.
///
/// Converts patterns like `"*.rs"`, `"Cargo.toml"`, `"**/*.py"` to
/// dot-prefixed extensions like `".rs"`, `".toml"`, `".py"` that the
/// LSP server manager expects for extension matching.
fn normalize_file_patterns(patterns: &[String]) -> Vec<String> {
    let mut extensions = Vec::new();
    for pattern in patterns {
        if let Some(dot_pos) = pattern.rfind('.') {
            let ext = format!(".{}", &pattern[dot_pos + 1..]);
            if !extensions.contains(&ext) {
                extensions.push(ext);
            }
        }
    }
    extensions
}

#[cfg(test)]
#[path = "integration.test.rs"]
mod tests;
