# vercel-ai-openai-compatible

Generic OpenAI-compatible provider for any API following the OpenAI protocol (xAI, Groq, Together, Fireworks, DeepSeek, etc.).

## TS Source

Ports `@ai-sdk/openai-compatible` v4 (not from `claude-code/src/`).

## Key Types

- `OpenAICompatibleProvider`, `OpenAICompatibleProviderSettings`, `create_openai_compatible()`
- `OpenAICompatibleConfig`, `SupportedUrlsFn`
- `MetadataExtractor`, `StreamMetadataExtractor` — per-call metadata extension hook
- Models: `OpenAICompatibleChatLanguageModel`, `OpenAICompatibleCompletionLanguageModel`, `OpenAICompatibleEmbeddingModel`, `OpenAICompatibleImageModel`

## Differences from `vercel-ai-openai`

- No OpenAI-specific features: capabilities detection, organization/project headers, Responses API.
- Extensibility hooks: `MetadataExtractor` trait, `transform_request_body`, `query_params`.
- Generic API key env var (not hardcoded to `OPENAI_API_KEY`); callers set `api_key_env_var` in settings.
- Reasoning support in responses via `reasoning_content` / `reasoning` fields.
- Default `language_model()` routes to Chat Completions (not Responses).
