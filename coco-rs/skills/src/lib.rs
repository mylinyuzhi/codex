//! Skill system — markdown workflow loading, discovery, execution.
//!
//! TS: skills/ (SkillDefinition, SkillManager, bundled + user + project + plugin skills)

pub mod bundled;
pub mod shell_exec;
pub mod watcher;

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

/// Execution context for a skill.
///
/// TS: `context: 'inline' | 'fork'`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillContext {
    /// Expand prompt into the current conversation.
    #[default]
    Inline,
    /// Run as an isolated sub-agent.
    Fork,
}

/// A skill definition loaded from a markdown file.
///
/// TS: `SkillDefinition` + frontmatter fields from `loadSkillsDir.ts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub source: SkillSource,
    /// Alternative names for this skill (e.g., short forms).
    ///
    /// TS: `BundledSkillDefinition.aliases`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Guidance for when the model should invoke this skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    /// Named parameters the skill accepts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argument_names: Vec<String>,
    /// Glob patterns for file paths this skill applies to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    /// Planning/exploration depth override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Execution context: inline (default) or fork (sub-agent).
    #[serde(default)]
    pub context: SkillContext,
    /// Agent type when `context` is `Fork`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Semantic version of the skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Whether the skill is disabled (skipped during loading).
    #[serde(default)]
    pub disabled: bool,
    /// Hook configuration (opaque JSON, interpreted by coco-hooks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<serde_json::Value>,
    /// Display hint for arguments (e.g., `[filename]`).
    ///
    /// TS: `argument-hint` frontmatter key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    /// Whether users can type `/name` to invoke this skill. Default: true.
    ///
    /// TS: `user-invocable` frontmatter key.
    #[serde(default = "default_true")]
    pub user_invocable: bool,
    /// Prevents the model from invoking this skill via the Skill tool.
    ///
    /// TS: `disable-model-invocation` frontmatter key.
    #[serde(default)]
    pub disable_model_invocation: bool,
    /// Shell configuration for the skill (opaque JSON).
    ///
    /// TS: `shell` frontmatter key (FrontmatterShell).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<serde_json::Value>,
    /// Character count of the skill's prompt content (for token estimation).
    ///
    /// TS: `contentLength` on PromptCommand.
    #[serde(default)]
    pub content_length: i64,
    /// Whether this skill is hidden from typeahead/help but still invocable.
    ///
    /// TS: `isHidden` — separate from `user_invocable` (which blocks user invocation entirely).
    #[serde(default)]
    pub is_hidden: bool,
}

fn default_true() -> bool {
    true
}

/// Where a skill was loaded from.
///
/// This is the canonical enum — also used by `skill_advanced.rs` in coco-tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    Bundled,
    User {
        path: PathBuf,
    },
    Project {
        path: PathBuf,
    },
    Plugin {
        plugin_name: String,
    },
    /// Enterprise/policy-managed skills (e.g., `/etc/claude-code/.claude/skills/`).
    ///
    /// TS: `policySettings` source in `getSkillsPath()`.
    Managed {
        path: PathBuf,
    },
    /// Skills discovered from an MCP server.
    Mcp {
        server_name: String,
    },
}

/// Skill manager — discovery, loading, deduplication.
#[derive(Default)]
pub struct SkillManager {
    skills: HashMap<String, SkillDefinition>,
}

impl SkillManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, skill: SkillDefinition) {
        self.skills.insert(skill.name.clone(), skill);
    }

    pub fn get(&self, name: &str) -> Option<&SkillDefinition> {
        self.skills.get(name).or_else(|| {
            self.skills
                .values()
                .find(|s| s.aliases.iter().any(|a| a == name))
        })
    }

    pub fn all(&self) -> impl Iterator<Item = &SkillDefinition> {
        self.skills.values()
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Discover and register skills from multiple directories.
    ///
    /// Uses SKILL.md directory format for standard skill dirs and legacy
    /// format (flat .md) for `.claude/commands/` directories.
    /// Skills from later directories overwrite earlier ones with the same name (last wins).
    pub fn load_from_dirs(&mut self, dirs: &[PathBuf]) {
        for dir in dirs {
            // Legacy .claude/commands/ supports flat .md files
            let format = if dir.ends_with("commands") {
                SkillDirFormat::Legacy
            } else {
                SkillDirFormat::SkillMdOnly
            };
            for skill in discover_skills_with_format(std::slice::from_ref(dir), format) {
                self.register(skill);
            }
        }
    }
}

/// Parse a skill definition from a markdown file.
///
/// Format:
/// - First line `# Name` → skill name
/// - Optional YAML-like frontmatter between `---` markers (description, allowed_tools, model)
/// - Remaining content → prompt field
pub fn load_skill_from_file(path: &Path) -> anyhow::Result<SkillDefinition> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read skill file {}: {e}", path.display()))?;

    parse_skill_markdown(&content, path)
}

/// Whether a directory uses the SKILL.md-only format (`.claude/skills/`)
/// or also allows flat `.md` files (`.claude/commands/` legacy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillDirFormat {
    /// Only `skill-name/SKILL.md` directories (TS `.claude/skills/` format).
    SkillMdOnly,
    /// Both `SKILL.md` directories and flat `.md` files (legacy `.claude/commands/`).
    Legacy,
}

/// Walk directories and discover skill files, deduplicating by canonical path.
///
/// TS: `getSkillDirCommands()` — discovers skills from multiple directories,
/// deduplicates by `realpath()`, supports both SKILL.md directories and flat .md files.
pub fn discover_skills(dirs: &[PathBuf]) -> Vec<SkillDefinition> {
    discover_skills_with_format(dirs, SkillDirFormat::SkillMdOnly)
}

/// Walk directories with explicit format control.
pub fn discover_skills_with_format(
    dirs: &[PathBuf],
    format: SkillDirFormat,
) -> Vec<SkillDefinition> {
    let mut skills = Vec::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();

    for dir in dirs {
        if !dir.is_dir() {
            tracing::debug!("skipping non-existent skill directory: {}", dir.display());
            continue;
        }

        // Check immediate children for SKILL.md directories
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.filter_map(Result::ok) {
                let entry_path = entry.path();

                if entry_path.is_dir() {
                    // Look for SKILL.md inside the directory (case-insensitive)
                    if let Some(skill_md) = find_skill_md(&entry_path) {
                        try_load_skill(
                            &skill_md,
                            &mut skills,
                            &mut seen_paths,
                            /*name_from_dir*/ true,
                        );
                    }
                } else if format == SkillDirFormat::Legacy
                    && entry_path.extension().is_some_and(|ext| ext == "md")
                    && entry_path.is_file()
                {
                    // Legacy: flat .md files in .claude/commands/
                    try_load_skill(
                        &entry_path,
                        &mut skills,
                        &mut seen_paths,
                        /*name_from_dir*/ false,
                    );
                }
            }
        }
    }

    skills
}

/// Find a SKILL.md file in a directory (case-insensitive).
fn find_skill_md(dir: &Path) -> Option<PathBuf> {
    let skill_md = dir.join("SKILL.md");
    if skill_md.is_file() {
        return Some(skill_md);
    }
    // Case-insensitive fallback
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name();
            if name.to_string_lossy().eq_ignore_ascii_case("skill.md") && entry.path().is_file() {
                return Some(entry.path());
            }
        }
    }
    None
}

/// Try to load a skill from a file, deduplicating by canonical path.
fn try_load_skill(
    path: &Path,
    skills: &mut Vec<SkillDefinition>,
    seen_paths: &mut HashSet<PathBuf>,
    name_from_dir: bool,
) {
    // Deduplicate by canonical path (TS: realpath)
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !seen_paths.insert(canonical) {
        tracing::debug!("skipping duplicate skill at {}", path.display());
        return;
    }

    match load_skill_from_file(path) {
        Ok(skill) if skill.disabled => {
            tracing::debug!("skipping disabled skill: {}", skill.name);
        }
        Ok(mut skill) => {
            // For SKILL.md format, derive name from parent directory
            if name_from_dir
                && let Some(parent) = path.parent()
                && let Some(dir_name) = parent.file_name()
            {
                skill.name = dir_name.to_string_lossy().to_string();
            }
            skills.push(skill);
        }
        Err(e) => {
            tracing::warn!("failed to load skill from {}: {e}", path.display());
        }
    }
}

/// Parse a comma-separated list from a frontmatter value.
fn parse_csv_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse skill markdown content into a `SkillDefinition`.
fn parse_skill_markdown(content: &str, path: &Path) -> anyhow::Result<SkillDefinition> {
    let mut lines = content.lines();

    // Extract name from first heading line `# Name`
    let name = loop {
        match lines.next() {
            Some(line) if line.trim().is_empty() => continue,
            Some(line) if line.starts_with("# ") => {
                break line.trim_start_matches("# ").trim().to_string();
            }
            Some(line) => {
                anyhow::bail!("expected `# Name` heading as first non-empty line, got: {line:?}");
            }
            None => anyhow::bail!("skill file is empty"),
        }
    };

    // Collect remaining lines to parse frontmatter + prompt
    let remaining: Vec<&str> = lines.collect();
    let (frontmatter, prompt_lines) = extract_frontmatter(&remaining);

    let description = frontmatter.get("description").cloned().unwrap_or_default();
    let allowed_tools = frontmatter
        .get("allowed-tools")
        .or(frontmatter.get("allowed_tools"))
        .map(|v| parse_csv_list(v));
    let model = frontmatter.get("model").cloned();
    let when_to_use = frontmatter
        .get("when-to-use")
        .or(frontmatter.get("when_to_use"))
        .cloned();
    let argument_names = frontmatter
        .get("argument-names")
        .or(frontmatter.get("argument_names"))
        .map(|v| parse_csv_list(v))
        .unwrap_or_default();
    let aliases = frontmatter
        .get("aliases")
        .map(|v| parse_csv_list(v))
        .unwrap_or_default();
    let paths = frontmatter
        .get("paths")
        .map(|v| {
            // Use brace-aware splitting so *.{ts,tsx} isn't broken on the inner comma
            split_top_level_commas(v)
                .into_iter()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .flat_map(expand_braces)
                .collect()
        })
        .unwrap_or_default();
    let effort = frontmatter.get("effort").cloned();
    let context = match frontmatter.get("context").map(String::as_str) {
        Some("fork") => SkillContext::Fork,
        _ => SkillContext::Inline,
    };
    let agent = frontmatter.get("agent").cloned();
    let version = frontmatter.get("version").cloned();
    let disabled = frontmatter
        .get("disabled")
        .is_some_and(|v| v == "true" || v == "yes");
    let argument_hint = frontmatter
        .get("argument-hint")
        .or(frontmatter.get("argument_hint"))
        .cloned();
    let user_invocable = frontmatter
        .get("user-invocable")
        .or(frontmatter.get("user_invocable"))
        .is_none_or(|v| v != "false" && v != "no");
    let disable_model_invocation = frontmatter
        .get("disable-model-invocation")
        .or(frontmatter.get("disable_model_invocation"))
        .is_some_and(|v| v == "true" || v == "yes");
    // Parse hooks as opaque JSON (single-line JSON object or plain string)
    let hooks = frontmatter.get("hooks").and_then(|v| {
        serde_json::from_str(v)
            .ok()
            .or_else(|| Some(serde_json::Value::String(v.clone())))
    });
    // Parse shell: plain string → Value::String, JSON object → Value::Object
    let shell = frontmatter
        .get("shell")
        .map(|v| serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.clone())));

    let prompt = prompt_lines.join("\n").trim().to_string();
    let content_length = prompt.len() as i64;
    // TS: isHidden = !(userInvocable ?? true)
    let is_hidden = !user_invocable;

    Ok(SkillDefinition {
        name,
        description,
        prompt,
        source: SkillSource::User {
            path: path.to_path_buf(),
        },
        aliases,
        allowed_tools,
        model,
        when_to_use,
        argument_names,
        paths,
        effort,
        context,
        agent,
        version,
        disabled,
        hooks,
        argument_hint,
        user_invocable,
        disable_model_invocation,
        shell,
        content_length,
        is_hidden,
    })
}

/// Extract YAML-like frontmatter between `---` markers and return
/// (key-value pairs, remaining lines after frontmatter).
fn extract_frontmatter<'a>(lines: &'a [&'a str]) -> (HashMap<String, String>, Vec<&'a str>) {
    let mut idx = 0;
    let mut frontmatter = HashMap::new();

    // Skip leading blank lines
    while idx < lines.len() && lines[idx].trim().is_empty() {
        idx += 1;
    }

    // Check for opening `---`
    if idx < lines.len() && lines[idx].trim() == "---" {
        idx += 1;
        let start = idx;

        // Find closing `---`
        while idx < lines.len() && lines[idx].trim() != "---" {
            idx += 1;
        }

        // Parse key: value pairs within frontmatter
        for line in &lines[start..idx] {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_string();
                let value = value.trim().to_string();
                if !key.is_empty() {
                    frontmatter.insert(key, value);
                }
            }
        }

        // Skip closing `---`
        if idx < lines.len() {
            idx += 1;
        }
    }

    let remaining = lines[idx..].to_vec();
    (frontmatter, remaining)
}

/// Platform-specific managed skills directory.
///
/// TS: `getManagedFilePath()` in `managedPath.ts`.
pub fn get_managed_skills_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from("/Library/Application Support/ClaudeCode/.claude/skills")
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Linux and other Unix platforms
        PathBuf::from("/etc/claude-code/.claude/skills")
    }
}

/// Standard skill directory paths by source, in loading priority order.
///
/// TS: `getSkillsPath()` — maps SettingSource to directory path.
/// Order: managed → user → project → legacy commands.
pub fn get_skill_paths(config_dir: &Path, project_dir: &Path) -> Vec<PathBuf> {
    vec![
        // Enterprise/policy-managed skills (highest priority)
        get_managed_skills_path(),
        // User-level skills: ~/.coco/skills/
        config_dir.join("skills"),
        // Project-level skills: .claude/skills/
        project_dir.join(".claude").join("skills"),
        // Legacy: .claude/commands/
        project_dir.join(".claude").join("commands"),
    ]
}

/// Maximum characters per skill entry in the listing.
const MAX_LISTING_DESC_CHARS: usize = 250;

/// Result of building a skill listing with budget constraints.
pub struct SkillListingResult {
    pub listing: String,
    pub included: usize,
    pub total: usize,
}

/// Inject skill descriptions into a system prompt, respecting a character budget.
///
/// TS: `formatCommandsWithinBudget()` in `prompt.ts` — caps at 1% of context
/// window, max 250 chars per entry, bundled skills never truncated.
pub fn inject_skill_listing(
    skills: &[&SkillDefinition],
    max_budget_chars: usize,
) -> SkillListingResult {
    if skills.is_empty() {
        return SkillListingResult {
            listing: String::new(),
            included: 0,
            total: 0,
        };
    }

    let total = skills.len();
    let mut listing = String::from("Available slash commands (skills):\n");
    let mut included = 0;

    // Bundled skills are always included (never truncated)
    for skill in skills
        .iter()
        .filter(|s| matches!(s.source, SkillSource::Bundled))
    {
        listing.push_str(&format_skill_entry(skill));
        included += 1;
    }

    // Non-bundled skills, subject to budget
    for skill in skills
        .iter()
        .filter(|s| !matches!(s.source, SkillSource::Bundled))
    {
        let entry = format_skill_entry(skill);
        if listing.len() + entry.len() > max_budget_chars {
            break;
        }
        listing.push_str(&entry);
        included += 1;
    }

    SkillListingResult {
        listing,
        included,
        total,
    }
}

/// Format a single skill entry for the listing, capping description length.
fn format_skill_entry(skill: &SkillDefinition) -> String {
    let mut entry = format!("- /{}", skill.name);
    if !skill.description.is_empty() {
        let desc = if skill.description.len() > MAX_LISTING_DESC_CHARS {
            format!("{}...", &skill.description[..MAX_LISTING_DESC_CHARS - 3])
        } else {
            skill.description.clone()
        };
        entry.push_str(&format!(": {desc}"));
    }
    if let Some(when) = &skill.when_to_use {
        let remaining = MAX_LISTING_DESC_CHARS.saturating_sub(entry.len());
        if remaining > 20 {
            let when_text = if when.len() > remaining - 5 {
                format!("{}...", &when[..remaining - 8])
            } else {
                when.clone()
            };
            entry.push_str(&format!(" - {when_text}"));
        }
    }
    entry.push('\n');
    entry
}

/// Get the invocable skills (those available as /commands).
pub fn get_invocable_skills(manager: &SkillManager) -> Vec<&SkillDefinition> {
    manager.all().filter(|s| !s.disabled).collect()
}

/// Generate the SkillTool system prompt with skill listing.
///
/// TS: `getPrompt()` in `tools/SkillTool/prompt.ts` — generates instruction
/// text explaining how to invoke skills, plus the formatted skill listing.
pub fn generate_skill_tool_prompt(
    skills: &[&SkillDefinition],
    context_window_tokens: i64,
) -> SkillListingResult {
    // Budget: 1% of context window × 4 chars/token (TS: default 8000 chars)
    let budget = ((context_window_tokens as f64 * 0.01 * 4.0) as usize).max(2000);

    let mut result = inject_skill_listing(skills, budget);

    if !result.listing.is_empty() {
        // Prepend instruction text (TS: getPrompt() static text)
        let instructions = "\
The following skills are available for use with the Skill tool:

";
        result.listing = format!("{instructions}{}", result.listing);
    }

    result
}

/// Dynamically discover skills from a directory encountered during file operations.
///
/// TS: Dynamic skill discovery triggered during Read/Write/Glob tool execution.
/// Skills found here are inserted after plugins but before built-in commands.
pub fn discover_dynamic_skills(dir: &Path) -> Vec<SkillDefinition> {
    let skills_dir = dir.join(".claude").join("skills");
    if !skills_dir.is_dir() {
        return Vec::new();
    }
    discover_skills(&[skills_dir])
}

/// Expand brace patterns in a glob string.
///
/// TS: `expandBraces()` in `frontmatterParser.ts` — recursively expands
/// `*.{ts,tsx}` → `["*.ts", "*.tsx"]` and nested `{a,{b,c}}` patterns.
pub fn expand_braces(pattern: &str) -> Vec<String> {
    // Find the first top-level brace group
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    // Find matching close brace (respecting nesting)
    let mut depth = 0;
    let mut close = None;
    for (i, ch) in pattern[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(close) = close else {
        return vec![pattern.to_string()];
    };

    let prefix = &pattern[..open];
    let suffix = &pattern[close + 1..];
    let inner = &pattern[open + 1..close];

    // Split on top-level commas only (not nested)
    let alternatives = split_top_level_commas(inner);

    alternatives
        .into_iter()
        .flat_map(|alt| {
            let combined = format!("{prefix}{alt}{suffix}");
            expand_braces(&combined)
        })
        .collect()
}

/// Split a string on commas that are not inside nested braces.
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Estimate the token count for a skill's frontmatter (name + description).
///
/// TS: estimateSkillFrontmatterTokens() — rough estimate based on char count.
pub fn estimate_skill_tokens(skill: &SkillDefinition) -> i64 {
    let chars = skill.name.len() + skill.description.len() + 20; // overhead
    (chars / 4) as i64
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
