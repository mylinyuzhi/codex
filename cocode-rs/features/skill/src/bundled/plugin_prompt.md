<command-name>/plugin</command-name>

# Plugin Command

You are handling the `/plugin` command. This command manages the plugin system: installing, uninstalling, enabling, disabling plugins, and managing marketplaces.

## Available Subcommands

Parse the arguments to determine the action:

### Plugin Management
- `install <name>` or `install <name>@<marketplace>`: Install a plugin from a marketplace
- `uninstall <name>` or `uninstall <name>@<marketplace>`: Uninstall a plugin
- `update <name>`: Update an installed plugin to the latest version
- `enable <name>`: Enable a disabled plugin
- `disable <name>`: Disable a plugin without uninstalling
- `list`: List all installed plugins with their status

### Marketplace Management
- `marketplace add <source>`: Add a marketplace source
- `marketplace remove <name>`: Remove a marketplace source
- `marketplace list`: List registered marketplaces
- `marketplace update [name]`: Refresh marketplace data (all or specific)

### Help
- No arguments or `help`: Show help information

## Implementation

For each subcommand, use the Bash tool to perform the operation:

### Install
1. Check if the plugin exists in any registered marketplace
2. Use the plugin system to install from marketplace to versioned cache
3. Report success with plugin name, version, and scope

### Uninstall
1. Check if the plugin is installed
2. Remove cached files and registry entry
3. Report success

### Enable/Disable
1. Read `~/.cocode/plugins/settings.json`
2. Update the enabled state for the plugin
3. Report the change

### List
1. Read `~/.cocode/plugins/installed_plugins.json` for installed plugins
2. Read `~/.cocode/plugins/settings.json` for enabled state
3. Display a formatted table

### Marketplace Add
Parse the source argument to determine the type:
- `--github <owner/repo>`: GitHub repository marketplace
- `--git <url>`: Generic git repository
- `--directory <path>`: Local directory
- `--file <path>`: Local file
- `--url <url>`: Remote URL
- If none of the above, treat as GitHub repo if it matches `owner/repo` pattern

### Marketplace List
1. Read `~/.cocode/plugins/known_marketplaces.json`
2. Display registered marketplaces with their sources

### Marketplace Update
1. Re-fetch marketplace data from sources
2. Report which marketplaces were refreshed

## Response Format

### For `list`:
```
Installed Plugins:

  Name              Version    Scope     Enabled
  ───────────────   ────────   ───────   ───────
  my-plugin         1.0.0      user      yes
  other-plugin      2.1.0      project   no

Total: 2 plugins (1 enabled, 1 disabled)
```

### For `install`:
```
Plugin installed successfully:
  Name: my-plugin
  Version: 1.0.0
  Scope: user
  Path: ~/.cocode/plugins/cache/marketplace/my-plugin/1.0.0/
```

### For `marketplace list`:
```
Registered Marketplaces:

  Name              Source                          Last Updated
  ───────────────   ─────────────────────────────   ────────────
  community         github: org/plugins             2025-01-15
  local-dev         directory: /path/to/plugins     2025-01-10

Total: 2 marketplaces
```

### For `help`:
```
/plugin - Manage plugins and marketplaces

Usage:
  /plugin list                           List installed plugins
  /plugin install <name>[@marketplace]   Install a plugin
  /plugin uninstall <name>               Uninstall a plugin
  /plugin update <name>                  Update a plugin
  /plugin enable <name>                  Enable a plugin
  /plugin disable <name>                 Disable a plugin
  /plugin marketplace list               List marketplaces
  /plugin marketplace add <source>       Add a marketplace
  /plugin marketplace remove <name>      Remove a marketplace
  /plugin marketplace update [name]      Refresh marketplace data
  /plugin help                           Show this help
```

## Important Notes

- The plugins directory is at `~/.cocode/plugins/`
- Plugin state files: `installed_plugins.json`, `settings.json`, `known_marketplaces.json`
- Installed plugin cache: `~/.cocode/plugins/cache/<marketplace>/<plugin>/<version>/`
- Marketplace data: `~/.cocode/plugins/marketplaces/<name>/`
- Plugins are enabled by default when installed
- Disabling a plugin keeps it installed but inactive
- Plugin changes take effect on the next session start
