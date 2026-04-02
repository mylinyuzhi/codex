# Root Module Crates — Combined Plan

TS source: `src/skills/`, `src/schemas/hooks.ts`, `src/utils/hooks/`, `src/tasks/`, `src/Task.ts`, `src/memdir/`, `src/services/extractMemories/`, `src/services/SessionMemory/`, `src/services/autoDream/`, `src/plugins/`, `src/services/plugins/`, `src/keybindings/`

These crates are standalone at workspace root, matching TS's flat top-level layout.

## Dependencies (all root modules)

```
coco-skills depends on:
  - coco-types, coco-config (EffortLevel), utils/frontmatter

coco-hooks depends on:
  - coco-types (HookEventType, HookResult), coco-config (Settings — hooks section as Value)
  - coco-tool (ToolUseContext — for hook execution context)
  - tokio, reqwest (HTTP hooks)

coco-tasks depends on:
  - coco-types (TaskId, TaskStatus, TaskStateBase, AgentId)
  - coco-tool (Tool trait — for agent task spawning)
  - tokio (process spawning, background execution)

coco-memory depends on:
  - coco-types (Message), coco-inference (ApiClient — LLM for auto-extraction)
  - utils/frontmatter (YAML frontmatter parsing)

coco-plugins depends on:
  - coco-types, coco-skills (SkillDefinition), coco-hooks (HooksSettings)

coco-keybindings depends on:
  - coco-types, serde, serde_json

None of these depend on: coco-tools, coco-query, any app/ crate.

None of these does NOT depend on:
  - coco-tools (concrete tool implementations)
  - coco-query (agent loop)
  - coco-state, coco-session, coco-tui, coco-cli (app layer)
  - coco-inference (except coco-memory which uses ApiClient for LLM extraction)
```

---

## coco-skills

TS source: `src/skills/`

### Data Definitions

```rust
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub argument_hint: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub effort: Option<EffortLevel>,
    pub context: SkillContext,  // Inline or Fork (subagent)
    pub agent: Option<String>,
    pub hooks: Option<HooksSettings>,
    pub paths: Option<Vec<String>>,  // glob patterns for applicability
    pub prompt_content: String,       // markdown body
}

pub enum SkillContext { Inline, Fork }
```

### Core Logic

```rust
pub struct SkillManager {
    skills: Vec<SkillDefinition>,
}

impl SkillManager {
    /// Discover skills from: ~/.claude/skills/, .claude/skills/, bundled
    pub fn load(cwd: &Path) -> Self;
    /// Parse YAML frontmatter + markdown body from .md files
    fn parse_skill_file(path: &Path) -> Result<SkillDefinition, SkillError>;
    pub fn find(&self, name: &str) -> Option<&SkillDefinition>;
    pub fn to_commands(&self) -> Vec<Command>;  // Convert to prompt commands
}
```

---

## coco-hooks

TS source: `src/schemas/hooks.ts`, `src/utils/hooks/`

### Data Definitions

```rust
pub struct HooksSettings {
    pub pre_tool_use: Vec<HookMatcher>,
    pub post_tool_use: Vec<HookMatcher>,
    pub session_start: Vec<HookMatcher>,
    pub setup: Vec<HookMatcher>,
    pub subagent_start: Vec<HookMatcher>,
    pub user_prompt_submit: Vec<HookMatcher>,
}

pub struct HookMatcher {
    pub matcher: Option<String>,  // tool name pattern, e.g. "Write", "Bash(git *)"
    pub hooks: Vec<HookCommand>,
}

pub enum HookCommand {
    Bash { command: String, shell: Option<ShellType>, timeout: Option<i64>, once: bool, r#async: bool },
    Prompt { prompt: String, model: Option<String>, timeout: Option<i64>, once: bool },
    Http { url: String, headers: HashMap<String, String>, timeout: Option<i64>, once: bool },
    Agent { prompt: String, model: Option<String>, timeout: Option<i64>, once: bool },
}
```

### Core Logic

```rust
pub struct HookExecutor;

impl HookExecutor {
    /// Execute hooks matching an event.
    /// 1. Evaluate `if` condition (permission rule syntax)
    /// 2. Run hook command (bash/prompt/http/agent)
    /// 3. Process response: continue, block, modify input, permission decision
    pub async fn run_hooks(
        event: HookEventType,
        tool_name: Option<&str>,
        input: &Value,
        context: &ToolUseContext,
        settings: &HooksSettings,
    ) -> Vec<HookResult>;
}
```

---

## coco-tasks

TS source: `src/tasks/`, `src/Task.ts`

### Core Logic

```rust
pub struct TaskManager {
    tasks: HashMap<TaskId, TaskState>,
}

impl TaskManager {
    pub fn spawn_shell(&mut self, input: ShellSpawnInput) -> TaskHandle;
    pub fn spawn_agent(&mut self, input: AgentSpawnInput) -> TaskHandle;
    pub fn get(&self, id: &TaskId) -> Option<&TaskState>;
    pub fn list(&self) -> Vec<&TaskState>;
    pub fn kill(&mut self, id: &TaskId) -> Result<(), TaskError>;
    pub fn read_output(&self, id: &TaskId, offset: i64) -> String;
}

pub struct ShellSpawnInput {
    pub command: String,
    pub description: String,
    pub timeout: Option<Duration>,
    pub agent_id: Option<AgentId>,
}

pub struct AgentSpawnInput {
    pub prompt: String,
    pub agent_type: String,
    pub tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub isolation: Option<IsolationMode>,  // None, Worktree
}
```

---

## coco-memory

TS source: `src/memdir/`, `src/services/extractMemories/`, `src/services/SessionMemory/`, `src/services/autoDream/`

### Data Definitions

```rust
pub enum MemoryEntryType { User, Feedback, Project, Reference }

pub struct MemoryEntry {
    pub name: String,
    pub description: String,
    pub entry_type: MemoryEntryType,
    pub content: String,
    pub file_path: PathBuf,
}

pub struct MemoryIndex {
    pub entries: Vec<MemoryIndexEntry>,  // one-line pointers in MEMORY.md
}
```

### Core Logic

```rust
pub struct MemoryManager {
    pub memory_dir: PathBuf,  // ~/.coco/projects/<hash>/memory/
    pub index: MemoryIndex,
}

impl MemoryManager {
    /// Load MEMORY.md index + all referenced memory files
    pub fn load(project_dir: &Path) -> Self;
    /// Save a new memory entry (write file + update MEMORY.md)
    pub fn save(&mut self, entry: MemoryEntry) -> Result<(), MemoryError>;
    /// Delete a memory entry
    pub fn delete(&mut self, name: &str) -> Result<(), MemoryError>;
    /// Auto-extract memories from conversation (LLM call)
    pub async fn auto_extract(&mut self, messages: &[Message], api: &ApiClient) -> Vec<MemoryEntry>;
}
```

---

## coco-plugins

TS source: `src/plugins/`, `src/types/plugin.ts`

### Data Definitions

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

### Core Logic

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

---

## coco-keybindings

TS source: `src/keybindings/`

```rust
pub struct Keybinding {
    pub key: String,          // e.g. "ctrl+s", "ctrl+shift+p"
    pub action: String,       // e.g. "submit", "newline"
    pub context: Option<String>,
}

pub fn load_keybindings() -> Vec<Keybinding>;
pub fn save_keybindings(bindings: &[Keybinding]);
```
