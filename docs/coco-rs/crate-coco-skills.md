# coco-skills — Crate Plan

TS source: `src/skills/` (20 files, 1.35K LOC)

## Dependencies

```
coco-skills depends on:
  - coco-types (ThinkingLevel), coco-config, utils/frontmatter

coco-skills does NOT depend on:
  - coco-tools, coco-query, coco-inference, any app/ crate
```

## Data Definitions

```rust
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub argument_hint: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub thinking_level: Option<ThinkingLevel>,
    pub context: SkillContext,  // Inline or Fork (subagent)
    pub agent: Option<String>,
    pub hooks: Option<HooksSettings>,
    pub paths: Option<Vec<String>>,  // glob patterns for applicability
    pub prompt_content: String,       // markdown body
}

pub enum SkillContext { Inline, Fork }
```

## Core Logic

```rust
pub struct SkillManager {
    skills: Vec<SkillDefinition>,
}

impl SkillManager {
    /// Multi-source discovery (loading order determines precedence):
    /// 1. Bundled skills (remember, verify, loop, claude-api, batch, etc.)
    /// 2. Plugin skills (via PluginContributions)
    /// 3. User skills: ~/.claude/skills/*.md
    /// 4. Project skills: .claude/skills/*.md
    /// 5. Managed skills (enterprise-provisioned)
    ///
    /// Deduplication: realpath canonical path comparison.
    /// If same file reached via symlink, loaded only once.
    pub fn load(cwd: &Path) -> Self;

    /// Parse YAML frontmatter + markdown body from .md files.
    /// Frontmatter fields: name, description, whenToUse, argumentHint,
    /// tools, model, effort, context (Inline|Fork), agent, hooks,
    /// paths (glob patterns for conditional activation).
    fn parse_skill_file(path: &Path) -> Result<SkillDefinition, SkillError>;
    pub fn find(&self, name: &str) -> Option<&SkillDefinition>;
    pub fn to_commands(&self) -> Vec<Command>;

    /// Conditional activation: only activate skills whose `paths` glob
    /// matches the current working directory or edited files.
    pub fn activate_for_paths(&mut self, paths: &[&Path]);

    /// Memoization: skill list cached per session.
    /// Invalidated on: skill file change (via file watcher), plugin reload.
    pub fn clear_caches(&mut self);
}

/// Bundled skills registry (compiled into binary).
/// Each has a SHA-256 fingerprint for integrity checking.
pub fn get_bundled_skills() -> Vec<SkillDefinition>;

/// Token estimation for skill frontmatter (used in context budgeting).
pub fn estimate_skill_frontmatter_tokens(skill: &SkillDefinition) -> i64;
```
