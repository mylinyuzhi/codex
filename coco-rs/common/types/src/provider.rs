use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

/// Which LLM provider implementation to use.
/// Consumed by coco-config (ProviderInfo) and coco-inference (ProviderFactory).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderApi {
    Anthropic,
    Openai,
    Gemini,
    Volcengine,
    Zai,
    OpenaiCompat,
}

impl ProviderApi {
    /// Canonical display name used in banners, config labels, and wire strings.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Openai => "openai",
            Self::Gemini => "google",
            Self::Volcengine => "volcengine",
            Self::Zai => "zai",
            Self::OpenaiCompat => "openai-compat",
        }
    }
}

impl fmt::Display for ProviderApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which purpose a model serves. Multiple roles can map to different models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    Main,
    Fast,
    Compact,
    Plan,
    Explore,
    Review,
    HookAgent,
    Memory,
    /// Forked-agent spawn (AgentTool / SkillTool). Distinct from
    /// `Explore` — Explore is an investigative subagent type that
    /// happens to often be a "small fast" model; Subagent is the
    /// generic spawn role used by tools/Agent and the swarm runtime.
    Subagent,
}

impl ModelRole {
    /// Canonical snake_case spelling. Matches the serde wire form so
    /// `ModelRole::Subagent.as_str() == serde_json::to_string(&Subagent)?`
    /// modulo the surrounding quotes.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Fast => "fast",
            Self::Compact => "compact",
            Self::Plan => "plan",
            Self::Explore => "explore",
            Self::Review => "review",
            Self::HookAgent => "hook_agent",
            Self::Memory => "memory",
            Self::Subagent => "subagent",
        }
    }
}

impl fmt::Display for ModelRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ModelRole {
    type Err = String;

    /// Accept the canonical snake_case spelling plus the camelCase form
    /// `hookAgent` for symmetry with TS-flavored config files. Trim and
    /// lowercase first so YAML scalars like `Explore` / ` plan ` parse.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "main" => Ok(Self::Main),
            "fast" => Ok(Self::Fast),
            "compact" => Ok(Self::Compact),
            "plan" => Ok(Self::Plan),
            "explore" => Ok(Self::Explore),
            "review" => Ok(Self::Review),
            "hook_agent" | "hookagent" => Ok(Self::HookAgent),
            "memory" => Ok(Self::Memory),
            "subagent" => Ok(Self::Subagent),
            _ => Err(format!("unknown model role: {s}")),
        }
    }
}

/// A resolved model identity: provider + model ID.
/// Produced by coco-config, consumed by coco-inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    /// Sub-provider routing string (e.g., "bedrock", "vertex").
    pub provider: String,
    /// Resolved ProviderApi for dispatch.
    pub api: ProviderApi,
    /// Model identifier (e.g., "claude-opus-4-6", "gpt-5").
    pub model_id: String,
    /// Human-readable display name.
    pub display_name: String,
}

impl PartialEq for ModelSpec {
    fn eq(&self, other: &Self) -> bool {
        self.provider == other.provider && self.api == other.api && self.model_id == other.model_id
    }
}

impl Eq for ModelSpec {}

impl std::hash::Hash for ModelSpec {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.provider.hash(state);
        self.api.hash(state);
        self.model_id.hash(state);
    }
}

/// Model capabilities (checked at request time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    TextGeneration,
    Streaming,
    Vision,
    Audio,
    ToolCalling,
    Embedding,
    ExtendedThinking,
    StructuredOutput,
    ReasoningSummaries,
    ParallelToolCalls,
    FastMode,
    /// Wire supports Anthropic-style `cache_control` blocks.
    PromptCache,
    /// `context-1m-2025-08-07` beta — 1M token context window.
    /// Wire name forced to `"context_1m"` (serde's snake_case treats digits
    /// as part of the preceding word and would emit `"context1m"`).
    #[serde(rename = "context_1m")]
    Context1m,
    /// `interleaved-thinking-2025-05-14` beta. Also gates
    /// `redact-thinking-2026-02-12` (TS `betas.ts:272` reuses the
    /// same `modelSupportsISP` predicate for both).
    InterleavedThinking,
    /// `context-management-2025-06-27` beta.
    ContextManagement,
    /// `token-efficient-tools-2026-03-28` beta. Mutually
    /// exclusive with structured outputs.
    TokenEfficientTools,
}

/// How a model handles file editing / apply_patch tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyPatchToolType {
    /// String-schema function tool (GPT-5.2+, codex models).
    #[default]
    Freeform,
    /// JSON function tool (gpt-oss).
    Function,
    /// Shell-based, prompt instructions only (GPT-5, o3, o4-mini).
    Shell,
}

/// Communication protocol (OpenAI has two APIs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireApi {
    Chat,
    Responses,
}

/// A set of capabilities for convenience.
pub type CapabilitySet = HashSet<Capability>;

#[cfg(test)]
#[path = "provider.test.rs"]
mod tests;
