# vercel-ai-openai

OpenAI provider for Vercel AI SDK v4. Covers Chat Completions, Responses, Completions, Embeddings, Images, Speech, Transcription APIs.

## TS Source

Ports `@ai-sdk/openai` v4 (not from `claude-code/src/`).

## Key Types

- `OpenAIProvider`, `OpenAIProviderSettings`, `openai()` (default), `create_openai()` (custom)
- `OpenAIConfig`, `OpenAIModelCapabilities`, `SystemMessageMode`, `get_capabilities()`
- Models: `OpenAIChatLanguageModel`, `OpenAIResponsesLanguageModel`, `OpenAICompletionLanguageModel`, `OpenAIEmbeddingModel`, `OpenAIImageModel`, `OpenAISpeechModel`, `OpenAITranscriptionModel`

## Conventions

- `provider.language_model(id)` defaults to the Responses API (not Chat); call `provider.chat(id)` explicitly for Chat Completions.
- Reads `OPENAI_API_KEY` by default; `OpenAIProviderSettings` overrides org/project/baseURL/headers.
- Capabilities detection (reasoning, system message handling, tool-choice flavor) lives in `openai_capabilities` — applied per model at request time.
