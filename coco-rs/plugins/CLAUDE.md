# coco-plugins

Plugin system: PLUGIN.toml manifests, bundled/user/project/repository sources, contribution discovery (skills / hooks / MCP servers / agents / commands), enable/disable, marketplace, hot-reload, MCPB (MCP Bundle) loading.

## TS Source
- `plugins/builtinPlugins.ts`, `plugins/bundled/` — compiled-in builtins
- `services/plugins/PluginInstallationManager.ts`, `pluginCliCommands.ts`, `pluginOperations.ts` — lifecycle
- `utils/plugins/pluginLoader.ts`, `installedPluginsManager.ts`, `refresh.ts`, `reconciler.ts` — load/reconcile
- `utils/plugins/marketplaceManager.ts`, `marketplaceHelpers.ts`, `officialMarketplace.ts`, `officialMarketplaceStartupCheck.ts`, `officialMarketplaceGcs.ts`, `parseMarketplaceInput.ts` — marketplace
- `utils/plugins/dependencyResolver.ts` — DFS transitive closure + scope demotion
- `utils/plugins/pluginPolicy.ts`, `pluginBlocklist.ts`, `pluginFlagging.ts`, `validatePlugin.ts` — security/policy
- `utils/plugins/mcpbHandler.ts`, `zipCache.ts`, `zipCacheAdapters.ts` — MCPB + cache
- `utils/plugins/loadPluginAgents.ts`, `loadPluginCommands.ts`, `loadPluginHooks.ts`, `loadPluginOutputStyles.ts`, `mcpPluginIntegration.ts`, `lspPluginIntegration.ts` — contribution loaders
- `utils/plugins/headlessPluginInstall.ts`, `pluginAutoupdate.ts`, `pluginStartupCheck.ts`, `performStartupChecks.tsx` — headless/CCR + startup
- `utils/plugins/schemas.ts`, `pluginVersioning.ts`, `pluginIdentifier.ts`, `pluginDirectories.ts`, `walkPluginMarkdown.ts` — support

Paths relative to `/lyz/codespace/3rd/claude-code/src/`.

## Key Types
- `PluginManifest` — name, version, description, skills (names), hooks (raw JSON map), mcp_servers (raw JSON map)
- `LoadedPlugin` — manifest, path, source, enabled flag
- `PluginSource` — `Builtin | User | Project | Repository{url}`
- `PluginManager` — name-keyed map; `register` / `get` / `enable` / `disable` / `enabled()` / `load_from_dirs`
- `PluginContributions` — skills, hooks, mcp_servers, agents, commands (aggregated from manifest + directory scan)
- `get_plugin_dirs(config_dir, project_dir)` — `~/.coco/plugins/*/` + `.claude/plugins/*/`
- `load_plugin_manifest()` — parses PLUGIN.toml
- `discover_plugins()` — scans dirs for root PLUGIN.toml
- `collect_all_contributions()` — merges contributions across all enabled plugins

## Modules
- `loader` — plugin manifest + directory loading
- `marketplace` — manifest fetch/reconcile/dependency resolution
- `hot_reload` — runtime plugin reload
- `schemas` — manifest + marketplace schemas
- `command_bridge` / `hook_bridge` / `skill_bridge` — wire plugin contributions into `CommandRegistry` / `HookRegistry` / `SkillManager`
