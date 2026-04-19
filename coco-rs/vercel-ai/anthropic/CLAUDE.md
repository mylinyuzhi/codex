# vercel-ai-anthropic

Anthropic (Claude) provider for Vercel AI SDK v4 — Messages API.

## TS Source

Ports `@ai-sdk/anthropic` v4 (not from `claude-code/src/`). All Anthropic-specific SDK concerns (prompt caching, beta headers, OAuth, policy limits, 529 retry, cache breakpoint detection) belong in **this crate**, not in `coco-inference` — see the "Multi-Provider SDK" design decision in the workspace `CLAUDE.md`.

## Key Types

- `AnthropicProvider`, `AnthropicProviderSettings`, `anthropic()` (default), `create_anthropic()`
- `AnthropicConfig`
- `AnthropicMessagesLanguageModel` — Messages API implementation
- `CacheControlValidator` — validates `cache_control` breakpoints (max 4 per request, positional rules)
- `forward_anthropic_container_id_from_last_step` — carries `container_id` across multi-step conversations (for tool_use containers)

## Modules

- `anthropic_provider` — provider + settings + factory
- `anthropic_config` — resolved request config
- `anthropic_error` — provider-specific error mapping
- `anthropic_metadata` — provider metadata extraction
- `messages` — `AnthropicMessagesLanguageModel` (the language model impl)
- `tool` — Anthropic-specific tool types (computer_use, bash, text_editor, web_search, web_fetch, code_execution, etc.)
- `cache_control` — breakpoint validator
- `forward_container_id` — container_id forwarding helper

## Conventions

- Reads `ANTHROPIC_API_KEY` by default; settings allow OAuth token / custom headers.
- Cache control: enforce 4-breakpoint limit and positional rules (system → last_user → last_assistant) via `CacheControlValidator`.
- Extended thinking: exposed through `ProviderOptions` (budget_tokens, interleaved) — mapped from `coco_types::ThinkingLevel` by `coco-inference::thinking_convert`.
