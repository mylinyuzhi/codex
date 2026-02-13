//! Built-in model and provider defaults.
//!
//! This module provides default configurations for well-known models
//! that are compiled into the binary. These serve as the lowest-priority
//! layer in the configuration resolution.

// Built-in prompt templates (embedded at compile time)
const DEFAULT_PROMPT: &str = include_str!("../prompt_with_apply_patch_instructions.md");
const GEMINI_PROMPT: &str = include_str!("../gemini_prompt.md");
const GPT_5_2_PROMPT: &str = include_str!("../gpt_5_2_prompt.md");
const GPT_5_2_CODEX_PROMPT: &str = include_str!("../gpt-5.2-codex_prompt.md");

// Built-in output style templates (embedded at compile time)
const OUTPUT_STYLE_EXPLANATORY: &str = include_str!("../output_style_explanatory.md");
const OUTPUT_STYLE_LEARNING: &str = include_str!("../output_style_learning.md");

use crate::types::ProviderConfig;
use crate::types::ProviderType;
use cocode_protocol::ApplyPatchToolType;
use cocode_protocol::Capability;
use cocode_protocol::ConfigShellToolType;
use cocode_protocol::ModelInfo;
use cocode_protocol::ThinkingLevel;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Get built-in model defaults for a model ID.
///
/// Returns `None` if no built-in defaults exist for this model.
pub fn get_model_defaults(model_id: &str) -> Option<ModelInfo> {
    BUILTIN_MODELS.get().and_then(|m| m.get(model_id).cloned())
}

/// Get built-in provider defaults for a provider name.
///
/// Returns `None` if no built-in defaults exist for this provider.
pub fn get_provider_defaults(provider_name: &str) -> Option<ProviderConfig> {
    BUILTIN_PROVIDERS
        .get()
        .and_then(|p| p.get(provider_name).cloned())
}

/// Get all built-in model IDs.
pub fn list_builtin_models() -> Vec<&'static str> {
    BUILTIN_MODELS
        .get()
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

/// Get all built-in provider names.
pub fn list_builtin_providers() -> Vec<&'static str> {
    BUILTIN_PROVIDERS
        .get()
        .map(|p| p.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

/// Get a built-in output style by name (case-insensitive).
///
/// Supported styles:
/// - `"explanatory"` - Educational insights while completing tasks
/// - `"learning"` - Hands-on learning with TODO(human) contributions
///
/// Returns `None` if the style name is not recognized.
pub fn get_output_style(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "explanatory" => Some(OUTPUT_STYLE_EXPLANATORY),
        "learning" => Some(OUTPUT_STYLE_LEARNING),
        _ => None,
    }
}

/// List all built-in output style names.
pub fn list_builtin_output_styles() -> Vec<&'static str> {
    vec!["explanatory", "learning"]
}

/// A custom output style loaded from a file.
#[derive(Debug, Clone)]
pub struct CustomOutputStyle {
    /// Style name (derived from filename).
    pub name: String,
    /// Style description (from frontmatter or first line).
    pub description: Option<String>,
    /// Full style content (the markdown body).
    pub content: String,
    /// Source file path.
    pub path: PathBuf,
}

/// Output style metadata parsed from YAML frontmatter.
#[derive(Debug, Clone, Default)]
pub struct OutputStyleFrontmatter {
    /// Style name override (defaults to filename).
    pub name: Option<String>,
    /// Human-readable description.
    pub description: Option<String>,
    /// Whether to keep the coding-instructions marker.
    pub keep_coding_instructions: Option<bool>,
}

/// Parse YAML frontmatter from markdown content.
///
/// Frontmatter is delimited by `---` at the start and end.
/// Returns (frontmatter, remaining_content).
fn parse_frontmatter(content: &str) -> (OutputStyleFrontmatter, &str) {
    let content = content.trim_start();

    // Check for frontmatter delimiter
    if !content.starts_with("---") {
        return (OutputStyleFrontmatter::default(), content);
    }

    // Find the closing delimiter
    let after_first = &content[3..].trim_start_matches(['\r', '\n']);
    if let Some(end_idx) = after_first.find("\n---") {
        let yaml_content = &after_first[..end_idx];
        let remaining = &after_first[end_idx + 4..].trim_start_matches(['\r', '\n', '-']);

        // Parse YAML content (simple key: value parsing)
        let mut fm = OutputStyleFrontmatter::default();
        for line in yaml_content.lines() {
            let line = line.trim();
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim().trim_matches('"').trim_matches('\'');
                match key {
                    "name" => fm.name = Some(value.to_string()),
                    "description" => fm.description = Some(value.to_string()),
                    "keep-coding-instructions" | "keep_coding_instructions" => {
                        fm.keep_coding_instructions = value.parse().ok();
                    }
                    _ => {} // Ignore unknown keys
                }
            }
        }

        return (fm, remaining);
    }

    (OutputStyleFrontmatter::default(), content)
}

/// Load custom output styles from the specified directory.
///
/// Scans for `*.md` files and parses them as output styles.
/// Files should optionally have YAML frontmatter with:
/// - `name`: Style name (defaults to filename without extension)
/// - `description`: Human-readable description
/// - `keep-coding-instructions`: Whether to preserve coding instruction markers
///
/// # Example File Structure
///
/// ```markdown
/// ---
/// name: concise
/// description: Short, direct responses without explanations
/// ---
/// You should be concise and direct.
/// Avoid unnecessary explanations.
/// ```
pub fn load_custom_output_styles(dir: &Path) -> Vec<CustomOutputStyle> {
    if !dir.is_dir() {
        return Vec::new();
    }

    let mut styles = Vec::new();

    // Read directory entries
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Only process .md files
        if path.extension().is_some_and(|ext| ext == "md") {
            if let Some(style) = load_single_style(&path) {
                styles.push(style);
            }
        }
    }

    // Sort by name for consistent ordering
    styles.sort_by(|a, b| a.name.cmp(&b.name));
    styles
}

/// Load a single output style from a file.
fn load_single_style(path: &Path) -> Option<CustomOutputStyle> {
    let content = fs::read_to_string(path).ok()?;

    // Parse frontmatter
    let (frontmatter, body) = parse_frontmatter(&content);

    // Derive name from frontmatter or filename
    let name = frontmatter.name.unwrap_or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string()
    });

    // Use frontmatter description or extract from first line
    let description = frontmatter.description.or_else(|| {
        body.lines()
            .next()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                // Truncate long descriptions
                if line.len() > 100 {
                    format!("{}...", &line[..97])
                } else {
                    line.to_string()
                }
            })
    });

    Some(CustomOutputStyle {
        name,
        description,
        content: body.trim().to_string(),
        path: path.to_path_buf(),
    })
}

/// Get the default output styles directory.
///
/// Returns `{cocode_home}/output-styles/`.
pub fn default_output_styles_dir(cocode_home: &std::path::Path) -> PathBuf {
    cocode_home.join("output-styles")
}

/// Load all output styles (built-in + custom).
///
/// Returns a combined list with built-in styles first, then custom styles.
/// Custom styles can shadow built-in styles with the same name.
pub fn load_all_output_styles(cocode_home: &std::path::Path) -> Vec<OutputStyleInfo> {
    let mut styles = Vec::new();

    // Add built-in styles
    for name in list_builtin_output_styles() {
        if let Some(content) = get_output_style(name) {
            styles.push(OutputStyleInfo {
                name: name.to_string(),
                description: builtin_style_description(name),
                content: content.to_string(),
                source: OutputStyleSource::Builtin,
            });
        }
    }

    // Add custom styles from default directory
    let dir = default_output_styles_dir(cocode_home);
    for custom in load_custom_output_styles(&dir) {
        styles.push(OutputStyleInfo {
            name: custom.name,
            description: custom.description,
            content: custom.content,
            source: OutputStyleSource::Custom(custom.path),
        });
    }

    styles
}

/// Get description for built-in styles.
fn builtin_style_description(name: &str) -> Option<String> {
    match name {
        "explanatory" => Some("Educational insights while completing tasks".to_string()),
        "learning" => Some("Hands-on learning with TODO(human) contributions".to_string()),
        _ => None,
    }
}

/// Information about an output style.
#[derive(Debug, Clone)]
pub struct OutputStyleInfo {
    /// Style name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Full style content.
    pub content: String,
    /// Source of the style.
    pub source: OutputStyleSource,
}

/// Source of an output style.
#[derive(Debug, Clone)]
pub enum OutputStyleSource {
    /// Built-in style compiled into the binary.
    Builtin,
    /// Custom style loaded from a file.
    Custom(PathBuf),
}

impl OutputStyleSource {
    /// Check if this is a built-in style.
    pub fn is_builtin(&self) -> bool {
        matches!(self, Self::Builtin)
    }

    /// Check if this is a custom style.
    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom(_))
    }
}

/// Find an output style by name.
///
/// Searches both built-in and custom styles. Custom styles take precedence
/// when there's a name conflict.
pub fn find_output_style(name: &str, cocode_home: &std::path::Path) -> Option<OutputStyleInfo> {
    let name_lower = name.to_lowercase();

    // Check custom styles first (they take precedence)
    let dir = default_output_styles_dir(cocode_home);
    for custom in load_custom_output_styles(&dir) {
        if custom.name.to_lowercase() == name_lower {
            return Some(OutputStyleInfo {
                name: custom.name,
                description: custom.description,
                content: custom.content,
                source: OutputStyleSource::Custom(custom.path),
            });
        }
    }

    // Fall back to built-in styles
    if let Some(content) = get_output_style(name) {
        return Some(OutputStyleInfo {
            name: name.to_string(),
            description: builtin_style_description(&name_lower),
            content: content.to_string(),
            source: OutputStyleSource::Builtin,
        });
    }

    None
}

// Lazily initialized built-in models
static BUILTIN_MODELS: OnceLock<HashMap<String, ModelInfo>> = OnceLock::new();
static BUILTIN_PROVIDERS: OnceLock<HashMap<String, ProviderConfig>> = OnceLock::new();

/// Initialize built-in defaults (called automatically on first access).
fn init_builtin_models() -> HashMap<String, ModelInfo> {
    let mut models = HashMap::new();

    // OpenAI GPT-5
    models.insert(
        "gpt-5".to_string(),
        ModelInfo {
            display_name: Some("GPT-5".to_string()),
            base_instructions: Some(DEFAULT_PROMPT.to_string()),
            context_window: Some(272000),
            max_output_tokens: Some(32000),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
                Capability::StructuredOutput,
                Capability::ParallelToolCalls,
            ]),

            auto_compact_pct: Some(95),
            default_thinking_level: Some(ThinkingLevel::medium()),
            supported_thinking_levels: Some(vec![
                ThinkingLevel::low(),
                ThinkingLevel::medium(),
                ThinkingLevel::high(),
            ]),
            apply_patch_tool_type: Some(ApplyPatchToolType::Shell),
            excluded_tools: Some(vec![
                "Edit".to_string(),
                "Write".to_string(),
                "ReadManyFiles".to_string(),
                "NotebookEdit".to_string(),
                "SmartEdit".to_string(),
            ]),
            ..Default::default()
        },
    );

    // OpenAI GPT-5.2
    models.insert(
        "gpt-5.2".to_string(),
        ModelInfo {
            display_name: Some("GPT-5.2".to_string()),
            base_instructions: Some(GPT_5_2_PROMPT.to_string()),
            context_window: Some(272000),
            max_output_tokens: Some(64000),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
                Capability::ExtendedThinking,
                Capability::ReasoningSummaries,
                Capability::ParallelToolCalls,
            ]),

            auto_compact_pct: Some(95),
            default_thinking_level: Some(ThinkingLevel::medium()),
            supported_thinking_levels: Some(vec![
                ThinkingLevel::low(),
                ThinkingLevel::medium(),
                ThinkingLevel::high(),
                ThinkingLevel::xhigh(),
            ]),
            shell_type: Some(ConfigShellToolType::ShellCommand),
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            excluded_tools: Some(vec![
                "Edit".to_string(),
                "Write".to_string(),
                "ReadManyFiles".to_string(),
                "NotebookEdit".to_string(),
                "SmartEdit".to_string(),
            ]),
            ..Default::default()
        },
    );

    // OpenAI GPT-5.2 Codex (optimized for coding)
    models.insert(
        "gpt-5.2-codex".to_string(),
        ModelInfo {
            display_name: Some("GPT-5.2 Codex".to_string()),
            description: Some("GPT-5.2 optimized for coding tasks".to_string()),
            base_instructions: Some(GPT_5_2_CODEX_PROMPT.to_string()),
            context_window: Some(272000),
            max_output_tokens: Some(64000),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
                Capability::ExtendedThinking,
                Capability::ReasoningSummaries,
                Capability::ParallelToolCalls,
            ]),

            auto_compact_pct: Some(95),
            default_thinking_level: Some(ThinkingLevel::medium()),
            supported_thinking_levels: Some(vec![
                ThinkingLevel::low(),
                ThinkingLevel::medium(),
                ThinkingLevel::high(),
                ThinkingLevel::xhigh(),
            ]),
            shell_type: Some(ConfigShellToolType::ShellCommand),
            apply_patch_tool_type: Some(ApplyPatchToolType::Freeform),
            excluded_tools: Some(vec![
                "Edit".to_string(),
                "Write".to_string(),
                "ReadManyFiles".to_string(),
                "NotebookEdit".to_string(),
                "SmartEdit".to_string(),
            ]),
            ..Default::default()
        },
    );

    // Google Gemini 3 Pro
    models.insert(
        "gemini-3-pro".to_string(),
        ModelInfo {
            display_name: Some("Gemini 3 Pro".to_string()),
            base_instructions: Some(GEMINI_PROMPT.to_string()),
            context_window: Some(300000),
            max_output_tokens: Some(32000),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
                Capability::ParallelToolCalls,
            ]),

            auto_compact_pct: Some(95),
            ..Default::default()
        },
    );

    // Google Gemini 3 Flash
    models.insert(
        "gemini-3-flash".to_string(),
        ModelInfo {
            display_name: Some("Gemini 3 Flash".to_string()),
            base_instructions: Some(GEMINI_PROMPT.to_string()),
            context_window: Some(300000),
            max_output_tokens: Some(16000),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
                Capability::ParallelToolCalls,
            ]),

            auto_compact_pct: Some(95),
            ..Default::default()
        },
    );

    models
}

fn init_builtin_providers() -> HashMap<String, ProviderConfig> {
    use crate::types::WireApi;

    let mut providers = HashMap::new();

    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            name: "openai".to_string(),
            provider_type: ProviderType::Openai,
            base_url: "https://api.openai.com/v1".to_string(),
            timeout_secs: 600,
            env_key: Some("OPENAI_API_KEY".to_string()),
            api_key: None,
            streaming: true,
            wire_api: WireApi::Responses,

            models: Vec::new(),
            options: None,
            interceptors: Vec::new(),
        },
    );

    providers.insert(
        "gemini".to_string(),
        ProviderConfig {
            name: "gemini".to_string(),
            provider_type: ProviderType::Gemini,
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            timeout_secs: 600,
            env_key: Some("GOOGLE_API_KEY".to_string()),
            api_key: None,
            streaming: true,
            wire_api: WireApi::Chat,

            models: Vec::new(),
            options: None,
            interceptors: Vec::new(),
        },
    );

    providers
}

// Force initialization by accessing the locks
pub(crate) fn ensure_initialized() {
    let _ = BUILTIN_MODELS.get_or_init(init_builtin_models);
    let _ = BUILTIN_PROVIDERS.get_or_init(init_builtin_providers);
}

#[cfg(test)]
#[path = "builtin.test.rs"]
mod tests;
