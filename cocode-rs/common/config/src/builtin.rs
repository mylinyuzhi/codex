//! Built-in model and provider defaults.
//!
//! This module provides default configurations for well-known models
//! that are compiled into the binary. These serve as the lowest-priority
//! layer in the configuration resolution.

use crate::capability::Capability;
use crate::capability::ReasoningEffort;
use crate::types::ModelInfoConfig;
use crate::types::ProviderJsonConfig;
use crate::types::ProviderType;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Get built-in model defaults for a model ID.
///
/// Returns `None` if no built-in defaults exist for this model.
pub fn get_model_defaults(model_id: &str) -> Option<ModelInfoConfig> {
    BUILTIN_MODELS.get().and_then(|m| m.get(model_id).cloned())
}

/// Get built-in provider defaults for a provider name.
///
/// Returns `None` if no built-in defaults exist for this provider.
pub fn get_provider_defaults(provider_name: &str) -> Option<ProviderJsonConfig> {
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

// Lazily initialized built-in models
static BUILTIN_MODELS: OnceLock<HashMap<String, ModelInfoConfig>> = OnceLock::new();
static BUILTIN_PROVIDERS: OnceLock<HashMap<String, ProviderJsonConfig>> = OnceLock::new();

/// Initialize built-in defaults (called automatically on first access).
fn init_builtin_models() -> HashMap<String, ModelInfoConfig> {
    let mut models = HashMap::new();

    // OpenAI models
    models.insert(
        "gpt-4o".to_string(),
        ModelInfoConfig {
            display_name: Some("GPT-4o".to_string()),
            context_window: Some(128000),
            max_output_tokens: Some(16384),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
                Capability::StructuredOutput,
            ]),
            auto_compact_token_limit: Some(100000),
            effective_context_window_percent: Some(95),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    models.insert(
        "gpt-4o-mini".to_string(),
        ModelInfoConfig {
            display_name: Some("GPT-4o Mini".to_string()),
            context_window: Some(128000),
            max_output_tokens: Some(16384),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
                Capability::StructuredOutput,
            ]),
            auto_compact_token_limit: Some(100000),
            effective_context_window_percent: Some(95),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    models.insert(
        "gpt-4-turbo".to_string(),
        ModelInfoConfig {
            display_name: Some("GPT-4 Turbo".to_string()),
            context_window: Some(128000),
            max_output_tokens: Some(4096),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
            ]),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    models.insert(
        "o1".to_string(),
        ModelInfoConfig {
            display_name: Some("o1".to_string()),
            context_window: Some(200000),
            max_output_tokens: Some(100000),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ExtendedThinking,
            ]),
            default_reasoning_effort: Some(ReasoningEffort::Medium),
            supports_reasoning_summaries: Some(true),
            ..Default::default()
        },
    );

    models.insert(
        "o1-mini".to_string(),
        ModelInfoConfig {
            display_name: Some("o1-mini".to_string()),
            context_window: Some(128000),
            max_output_tokens: Some(65536),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ExtendedThinking,
            ]),
            default_reasoning_effort: Some(ReasoningEffort::Medium),
            ..Default::default()
        },
    );

    models.insert(
        "o3-mini".to_string(),
        ModelInfoConfig {
            display_name: Some("o3-mini".to_string()),
            context_window: Some(200000),
            max_output_tokens: Some(100000),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ExtendedThinking,
                Capability::ToolCalling,
            ]),
            default_reasoning_effort: Some(ReasoningEffort::Medium),
            supports_reasoning_summaries: Some(true),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    // Anthropic models
    models.insert(
        "claude-opus-4-20250514".to_string(),
        ModelInfoConfig {
            display_name: Some("Claude Opus 4".to_string()),
            context_window: Some(200000),
            max_output_tokens: Some(32000),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
                Capability::ExtendedThinking,
            ]),
            default_reasoning_effort: Some(ReasoningEffort::Medium),
            supports_reasoning_summaries: Some(true),
            thinking_budget_default: Some(10000),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    models.insert(
        "claude-sonnet-4-20250514".to_string(),
        ModelInfoConfig {
            display_name: Some("Claude Sonnet 4".to_string()),
            context_window: Some(200000),
            max_output_tokens: Some(64000),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
                Capability::ExtendedThinking,
            ]),
            default_reasoning_effort: Some(ReasoningEffort::Medium),
            supports_reasoning_summaries: Some(true),
            thinking_budget_default: Some(10000),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    models.insert(
        "claude-3-5-sonnet-20241022".to_string(),
        ModelInfoConfig {
            display_name: Some("Claude 3.5 Sonnet".to_string()),
            context_window: Some(200000),
            max_output_tokens: Some(8192),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
            ]),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    models.insert(
        "claude-3-5-haiku-20241022".to_string(),
        ModelInfoConfig {
            display_name: Some("Claude 3.5 Haiku".to_string()),
            context_window: Some(200000),
            max_output_tokens: Some(8192),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
            ]),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    // Google Gemini models
    models.insert(
        "gemini-2.0-flash".to_string(),
        ModelInfoConfig {
            display_name: Some("Gemini 2.0 Flash".to_string()),
            context_window: Some(1000000),
            max_output_tokens: Some(8192),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
                Capability::Audio,
            ]),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    models.insert(
        "gemini-2.0-flash-thinking-exp".to_string(),
        ModelInfoConfig {
            display_name: Some("Gemini 2.0 Flash Thinking".to_string()),
            context_window: Some(1000000),
            max_output_tokens: Some(65536),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ExtendedThinking,
            ]),
            thinking_budget_default: Some(24576),
            ..Default::default()
        },
    );

    models.insert(
        "gemini-1.5-pro".to_string(),
        ModelInfoConfig {
            display_name: Some("Gemini 1.5 Pro".to_string()),
            context_window: Some(2000000),
            max_output_tokens: Some(8192),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
            ]),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    models.insert(
        "gemini-1.5-flash".to_string(),
        ModelInfoConfig {
            display_name: Some("Gemini 1.5 Flash".to_string()),
            context_window: Some(1000000),
            max_output_tokens: Some(8192),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::Vision,
                Capability::ToolCalling,
            ]),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    // DeepSeek models
    models.insert(
        "deepseek-r1".to_string(),
        ModelInfoConfig {
            display_name: Some("DeepSeek R1".to_string()),
            context_window: Some(64000),
            max_output_tokens: Some(8192),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ExtendedThinking,
            ]),
            default_reasoning_effort: Some(ReasoningEffort::Medium),
            ..Default::default()
        },
    );

    models.insert(
        "deepseek-chat".to_string(),
        ModelInfoConfig {
            display_name: Some("DeepSeek Chat".to_string()),
            context_window: Some(64000),
            max_output_tokens: Some(8192),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
            ]),
            supports_parallel_tool_calls: Some(true),
            ..Default::default()
        },
    );

    // Qwen models
    models.insert(
        "qwen-max".to_string(),
        ModelInfoConfig {
            display_name: Some("Qwen Max".to_string()),
            context_window: Some(32000),
            max_output_tokens: Some(8192),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
            ]),
            ..Default::default()
        },
    );

    models.insert(
        "qwen-plus".to_string(),
        ModelInfoConfig {
            display_name: Some("Qwen Plus".to_string()),
            context_window: Some(131072),
            max_output_tokens: Some(8192),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
            ]),
            ..Default::default()
        },
    );

    models.insert(
        "qwen-turbo".to_string(),
        ModelInfoConfig {
            display_name: Some("Qwen Turbo".to_string()),
            context_window: Some(131072),
            max_output_tokens: Some(8192),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
            ]),
            ..Default::default()
        },
    );

    // GLM models (Z.AI)
    models.insert(
        "glm-4-plus".to_string(),
        ModelInfoConfig {
            display_name: Some("GLM-4 Plus".to_string()),
            context_window: Some(128000),
            max_output_tokens: Some(4096),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
            ]),
            ..Default::default()
        },
    );

    models.insert(
        "glm-4-flash".to_string(),
        ModelInfoConfig {
            display_name: Some("GLM-4 Flash".to_string()),
            context_window: Some(128000),
            max_output_tokens: Some(4096),
            capabilities: Some(vec![
                Capability::TextGeneration,
                Capability::Streaming,
                Capability::ToolCalling,
            ]),
            ..Default::default()
        },
    );

    models
}

fn init_builtin_providers() -> HashMap<String, ProviderJsonConfig> {
    let mut providers = HashMap::new();

    providers.insert(
        "openai".to_string(),
        ProviderJsonConfig {
            name: "OpenAI".to_string(),
            provider_type: ProviderType::Openai,
            env_key: Some("OPENAI_API_KEY".to_string()),
            base_url: Some("https://api.openai.com/v1".to_string()),
            default_model: Some("gpt-4o".to_string()),
            timeout_secs: Some(600),
            ..Default::default()
        },
    );

    providers.insert(
        "anthropic".to_string(),
        ProviderJsonConfig {
            name: "Anthropic".to_string(),
            provider_type: ProviderType::Anthropic,
            env_key: Some("ANTHROPIC_API_KEY".to_string()),
            base_url: Some("https://api.anthropic.com".to_string()),
            default_model: Some("claude-sonnet-4-20250514".to_string()),
            timeout_secs: Some(600),
            ..Default::default()
        },
    );

    providers.insert(
        "gemini".to_string(),
        ProviderJsonConfig {
            name: "Google Gemini".to_string(),
            provider_type: ProviderType::Gemini,
            env_key: Some("GOOGLE_API_KEY".to_string()),
            base_url: Some("https://generativelanguage.googleapis.com".to_string()),
            default_model: Some("gemini-2.0-flash".to_string()),
            timeout_secs: Some(600),
            ..Default::default()
        },
    );

    providers.insert(
        "volcengine".to_string(),
        ProviderJsonConfig {
            name: "Volcengine Ark".to_string(),
            provider_type: ProviderType::Volcengine,
            env_key: Some("ARK_API_KEY".to_string()),
            base_url: Some("https://ark.cn-beijing.volces.com/api/v3".to_string()),
            timeout_secs: Some(600),
            ..Default::default()
        },
    );

    providers.insert(
        "zai".to_string(),
        ProviderJsonConfig {
            name: "Z.AI".to_string(),
            provider_type: ProviderType::Zai,
            env_key: Some("ZAI_API_KEY".to_string()),
            base_url: Some("https://open.bigmodel.cn/api/paas/v4".to_string()),
            default_model: Some("glm-4-plus".to_string()),
            timeout_secs: Some(600),
            ..Default::default()
        },
    );

    providers.insert(
        "deepseek".to_string(),
        ProviderJsonConfig {
            name: "DeepSeek".to_string(),
            provider_type: ProviderType::OpenaiCompat,
            env_key: Some("DEEPSEEK_API_KEY".to_string()),
            base_url: Some("https://api.deepseek.com/v1".to_string()),
            default_model: Some("deepseek-chat".to_string()),
            timeout_secs: Some(600),
            ..Default::default()
        },
    );

    providers.insert(
        "dashscope".to_string(),
        ProviderJsonConfig {
            name: "Alibaba DashScope".to_string(),
            provider_type: ProviderType::OpenaiCompat,
            env_key: Some("DASHSCOPE_API_KEY".to_string()),
            base_url: Some("https://dashscope.aliyuncs.com/compatible-mode/v1".to_string()),
            default_model: Some("qwen-plus".to_string()),
            timeout_secs: Some(600),
            ..Default::default()
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
mod tests {
    use super::*;

    #[test]
    fn test_get_model_defaults() {
        ensure_initialized();

        let gpt4o = get_model_defaults("gpt-4o").unwrap();
        assert_eq!(gpt4o.display_name, Some("GPT-4o".to_string()));
        assert_eq!(gpt4o.context_window, Some(128000));

        let claude = get_model_defaults("claude-sonnet-4-20250514").unwrap();
        assert_eq!(claude.display_name, Some("Claude Sonnet 4".to_string()));
        assert!(claude.thinking_budget_default.is_some());

        let unknown = get_model_defaults("unknown-model");
        assert!(unknown.is_none());
    }

    #[test]
    fn test_get_provider_defaults() {
        ensure_initialized();

        let openai = get_provider_defaults("openai").unwrap();
        assert_eq!(openai.name, "OpenAI");
        assert_eq!(openai.env_key, Some("OPENAI_API_KEY".to_string()));

        let anthropic = get_provider_defaults("anthropic").unwrap();
        assert_eq!(anthropic.provider_type, ProviderType::Anthropic);

        let unknown = get_provider_defaults("unknown-provider");
        assert!(unknown.is_none());
    }

    #[test]
    fn test_list_builtin_models() {
        ensure_initialized();

        let models = list_builtin_models();
        assert!(models.contains(&"gpt-4o"));
        assert!(models.contains(&"claude-sonnet-4-20250514"));
        assert!(models.contains(&"gemini-2.0-flash"));
    }

    #[test]
    fn test_list_builtin_providers() {
        ensure_initialized();

        let providers = list_builtin_providers();
        assert!(providers.contains(&"openai"));
        assert!(providers.contains(&"anthropic"));
        assert!(providers.contains(&"gemini"));
    }

    #[test]
    fn test_model_capabilities() {
        ensure_initialized();

        let gpt4o = get_model_defaults("gpt-4o").unwrap();
        let caps = gpt4o.capabilities.unwrap();
        assert!(caps.contains(&Capability::TextGeneration));
        assert!(caps.contains(&Capability::Vision));
        assert!(caps.contains(&Capability::ToolCalling));

        let o1 = get_model_defaults("o1").unwrap();
        let caps = o1.capabilities.unwrap();
        assert!(caps.contains(&Capability::ExtendedThinking));
    }

    #[test]
    fn test_thinking_models() {
        ensure_initialized();

        let claude = get_model_defaults("claude-sonnet-4-20250514").unwrap();
        assert!(claude.thinking_budget_default.is_some());
        assert!(claude.supports_reasoning_summaries.unwrap_or(false));

        let o1 = get_model_defaults("o1").unwrap();
        assert!(o1.default_reasoning_effort.is_some());
    }
}
