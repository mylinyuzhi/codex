use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
