# coco-plugins — Crate Plan

TS source: `src/plugins/`, `src/types/plugin.ts`, `src/utils/plugins/` (44 files, 20.5K LOC)

## Dependencies

```
coco-plugins depends on:
  - coco-types, coco-skills (SkillDefinition), coco-hooks (HooksSettings)

coco-plugins does NOT depend on:
  - coco-tools, coco-query, coco-inference, any app/ crate
```

## Data Definitions

```rust
pub struct PluginManifest {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub skills: Option<Vec<SkillDefinition>>,
    pub hooks: Option<HooksSettings>,
    pub mcp_servers: Option<HashMap<String, McpServerConfig>>,
}

pub struct LoadedPlugin {
    pub name: String,
    pub manifest: PluginManifest,
    pub path: PathBuf,
    pub source: String,
    pub enabled: bool,
    pub is_builtin: bool,
}
```

## Core Logic

```rust
pub struct PluginManager {
    plugins: Vec<LoadedPlugin>,
}

impl PluginManager {
    /// Load from: builtin plugins + marketplace + local
    pub fn load(settings: &Settings) -> Self;
    pub fn enabled(&self) -> Vec<&LoadedPlugin>;
    pub fn skills(&self) -> Vec<SkillDefinition>;
    pub fn hooks(&self) -> HooksSettings;
    pub fn mcp_servers(&self) -> HashMap<String, McpServerConfig>;
}
```
