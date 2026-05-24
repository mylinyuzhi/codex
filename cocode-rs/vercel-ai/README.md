# Vercel AI SDK (Rust)

Rust port of the [Vercel AI SDK](https://github.com/vercel/ai) @ `8b1e7ad43c03a75e5d4b81ef5caef8acba342580`.

Multi-provider LLM SDK with streaming, tool use, structured output, embeddings, and multimodal generation.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│  ai (high-level SDK)                                             │
│  generate_text, stream_text, generate_object, stream_object,     │
│  embed, generate_image, generate_speech, generate_video,         │
│  transcribe, rerank                                              │
├──────────────────────────────────────────────────────────────────┤
│  Providers                                                       │
│  openai │ anthropic │ google │ openai-compatible │ bytedance     │
├──────────────────────────────────────────────────────────────────┤
│  provider-utils (HTTP, auth, schema, tool mapping)               │
├──────────────────────────────────────────────────────────────────┤
│  provider (traits: LanguageModelV4, EmbeddingModelV4, ...)       │
└──────────────────────────────────────────────────────────────────┘
```

Dependency flow: `provider` ← `provider-utils` ← providers / `ai`

## Crates

| Crate | Purpose | LoC |
|-------|---------|-----|
| **provider** | V4 model traits and types — no external deps on cocode | ~10k |
| **provider-utils** | HTTP helpers, auth, schema validation, response handlers | ~5.5k |
| **ai** | High-level SDK: `generate_text()`, `stream_text()`, `embed()`, etc. | ~32k |
| **openai** | OpenAI provider (Chat Completions + Responses API, embeddings, images, speech, transcription) | ~9.3k |
| **anthropic** | Anthropic Claude provider (messages API, cache control) | ~8.5k |
| **google** | Google Gemini provider (language, embeddings, images, video) | ~6.2k |
| **openai-compatible** | Generic OpenAI-compatible provider (xAI, Groq, Together, etc.) | ~5k |
| **bytedance** | ByteDance Seedance video provider via ModelArk API | ~0.9k |

**Total: ~78k LoC, 643 files, 248 test files**

## Provider Traits (provider crate)

Seven model traits define the provider contract:

| Trait | Method | Description |
|-------|--------|-------------|
| `LanguageModelV4` | `do_generate()`, `do_stream()` | Text generation and streaming |
| `EmbeddingModelV4` | `do_embed()` | Vector embeddings |
| `ImageModelV4` | `do_generate()` | Image generation |
| `VideoModelV4` | `do_generate()` | Video generation |
| `SpeechModelV4` | `do_generate()` | Text-to-speech |
| `TranscriptionModelV4` | `do_generate()` | Speech-to-text |
| `RerankingModelV4` | `do_rerank()` | Document reranking |

Each trait has a corresponding middleware trait (`LanguageModelV4Middleware`, etc.) for wrapping models with custom behavior.

## SDK Functions (ai crate)

| Function | Description |
|----------|-------------|
| `generate_text()` | Non-streaming text generation with tool use |
| `stream_text()` | Streaming text with real-time tool execution |
| `generate_object()` | Structured output (JSON schema → typed) |
| `stream_object()` | Streaming structured output |
| `embed()` / `embed_many()` | Vector embeddings |
| `generate_image()` | Image generation |
| `generate_speech()` | Text-to-speech |
| `generate_video()` | Video generation |
| `transcribe()` | Audio-to-text |
| `rerank()` | Document reranking |

## Provider Details

### OpenAI

- **Models:** Chat Completions, Responses API, Completions (legacy), embeddings, DALL-E, TTS, Whisper
- **Auth:** `OPENAI_API_KEY` env var
- **Features:** Organization/Project headers, model capability detection, structured output

### Anthropic

- **Models:** Messages API (Claude)
- **Auth:** `ANTHROPIC_API_KEY` (x-api-key header) or `ANTHROPIC_AUTH_TOKEN` (Bearer token)
- **Features:** Cache control validation, container ID forwarding

### Google

- **Models:** Gemini (language, embeddings, Imagen, Veo)
- **Auth:** `GOOGLE_GENERATIVE_AI_API_KEY` env var (query parameter)
- **Features:** OpenAPI schema conversion, file URL support

### OpenAI-Compatible

- **Purpose:** Generic provider for any OpenAI-protocol API (xAI, Groq, Together, Fireworks, DeepSeek, etc.)
- **Auth:** Configurable `api_key_env_var`
- **Features:** Request body transformation hooks, custom metadata extractors, reasoning content support

### ByteDance

- **Models:** Seedance video models via ModelArk API
- **Auth:** `ARK_API_KEY` env var

## Key Design Patterns

- **Factory pattern:** Each provider exposes `provider_name()` and `create_provider_name()` constructors
- **Async-first:** All I/O uses async/await with tokio
- **Streaming events:** Typed `LanguageModelV4StreamPart` enum — not generic strings
- **Middleware:** `FnOnce` + `BoxFuture` callbacks for `do_generate`/`do_stream` interception
- **Error handling:** `thiserror` throughout — standalone from cocode error system
- **Zero cocode deps:** `provider` and `provider-utils` have no dependencies on cocode crates

## Testing

```bash
just test-crate cocode-vercel-ai          # High-level SDK tests
just test-crate cocode-vercel-ai-openai   # OpenAI provider tests
just test-crate cocode-vercel-ai-anthropic # Anthropic provider tests
```

Integration tests in `ai/tests/live/` require provider API keys.

## Further Reading

- [ai/CLAUDE.md](ai/CLAUDE.md) — SDK implementation details
- [provider/CLAUDE.md](provider/CLAUDE.md) — V4 trait specifications
- [provider-utils/CLAUDE.md](provider-utils/CLAUDE.md) — Utility patterns
