# vercel-ai-google

Google Generative AI (Gemini) provider for Vercel AI SDK v4. Supports language, embedding, image, and video generation.

## TS Source

Ports `@ai-sdk/google` v4 (not from `claude-code/src/`).

## Key Types

- `GoogleGenerativeAIProvider`, `GoogleGenerativeAIProviderSettings`, `google()` (default), `create_google_generative_ai()`
- Models: `GoogleGenerativeAILanguageModel` + config, `GoogleGenerativeAIEmbeddingModel` + config, `GoogleGenerativeAIImageModel` + config, `GoogleGenerativeAIVideoModel` + config
- Settings: `GoogleGenerativeAIImageSettings`, `GoogleGenerativeAIVideoSettings`
- Error handling: `GoogleErrorData`, `GoogleFailedResponseHandler`
- Prompt conversion: `convert_to_google_generative_ai_messages`, `ConvertOptions`, `convert_json_schema_to_openapi_schema`, `convert_usage`, `GoogleUsageMetadata`, `get_model_path`, `is_supported_file_url`, `map_finish_reason`
- Tools: `prepare_tools`, `PreparedTools` + Google-specific tool constructors

## Conventions

- Reads `GOOGLE_GENERATIVE_AI_API_KEY` by default.
- Prompt format diverges from OpenAI — content parts convert to Gemini `parts` array in `convert_to_google_generative_ai_messages`.
- JSON schema in tool definitions is rewritten to OpenAPI schema via `convert_json_schema_to_openapi_schema` before sending (Gemini has stricter schema rules).
