# vercel-ai-anthropic

Anthropic (Claude) provider for Vercel AI SDK v4 — Messages API.

## TS Source

Ports `@ai-sdk/anthropic` v4 (not from `claude-code/src/`). All Anthropic-specific SDK concerns (prompt caching, beta headers, OAuth, policy limits, 529 retry, cache breakpoint detection) belong in **this crate**, not in `coco-inference` — see the "Multi-Provider SDK" design decision in the workspace `CLAUDE.md`.

## Key Types

- `AnthropicProvider`, `AnthropicProviderSettings`, `anthropic()` (default), `create_anthropic()`
- `AnthropicConfig` — session-stable resolved request config (carries capabilities, topology, knobs, allowlist, account_kind, in_overage)
- `AdapterAccountKind`, `AnthropicModelCapabilities`, `ProviderTopology` — adapter-local mirrors of `coco_types` enums (kept here so `vercel-ai-anthropic` stays L0)
- `AnthropicProviderOptionsConfig`, `parse_provider_options`, `ProviderOptionsError` — adapter-owned schema for `ProviderConfig.provider_options`. Four typed bool knobs (`experimental_betas`, `disable_interleaved_thinking`, `show_thinking_summaries`, `non_interactive`) parsed from the opaque per-instance `BTreeMap<String, Value>`; `deny_unknown_fields` catches typos at startup.
- `AnthropicMessagesLanguageModel` — Messages API implementation
- `CacheControlValidator` — validates `cache_control` breakpoints (max 4 per request, positional rules)
- `CachePolicy` — `OnceLock` 1h-TTL eligibility + allowlist latches (R3-F3 session-stable)
- `ResolvedBetas`, `resolve_betas`, `should_emit_context_management` — single source of truth for which betas a request emits
- `map_capability`, `CLAUDE_CODE_BASELINE` — typed enum → kebab-case Anthropic header string
- `compute_marker_index_post_group`, `build_cache_control_value`, `attach_marker_at` — auto cache-marker placement
- `forward_anthropic_container_id_from_last_step` — carries `container_id` across multi-step conversations (for tool_use containers)

## Modules

- `anthropic_provider` — provider + settings + factory
- `anthropic_config` — session-stable resolved request config
- `anthropic_error` — provider-specific error mapping
- `anthropic_metadata` — provider metadata extraction
- `messages` — `AnthropicMessagesLanguageModel` (the language model impl)
- `tool` — Anthropic-specific tool types (computer_use, bash, text_editor, web_search, web_fetch, code_execution, etc.)
- `cache_control` — breakpoint validator
- `provider_options` — adapter-owned schema for `ProviderConfig.provider_options`; parses the per-instance opaque knob map into `AnthropicProviderOptionsConfig`
- `cache_policy` — TS `should1hCacheTTL` mirror; eligibility latch + per-call allowlist match
- `cache_placement` — auto-marker placement on last user content block (design §10.3)
- `beta_resolver` — capability + topology + knob → wire header set; central source of truth (R3-F2)
- `beta_capabilities` — typed enum ↔ Anthropic kebab-case header string
- `forward_container_id` — container_id forwarding helper

## Conventions

- Reads `ANTHROPIC_API_KEY` by default; settings allow OAuth token / custom headers.
- Cache control: enforce 4-breakpoint limit and positional rules (system → last_user → last_assistant) via `CacheControlValidator`.
- Extended thinking: exposed through `ProviderOptions` (budget_tokens, interleaved) — mapped from `coco_types::ThinkingLevel` by `coco-inference::thinking_convert`.
- **L0 layer rule:** the crate cannot import `coco-*` types. Inputs cross the boundary as adapter-local mirror enums (`AdapterAccountKind`, `AdapterCacheMode`, `AdapterCacheTtl`, `AdapterCacheScope`, `AdapterBetaCapability`) with **identical wire JSON** to `coco_types::*`. Translation happens in `services/inference::model_factory::build_anthropic`.
- **Single source of truth for context-management:** body insert / memory tool / `context-management-2025-06-27` beta header all gate on `beta_resolver::should_emit_context_management`. Half-emitted state is structurally impossible (R3-F2).
- **Internal-only signals never reach the wire:** `INTERNAL_ANTHROPIC_OPTION_KEYS` (`cacheStrategy` / `requestedBetas` / `agenticQuery` / `querySource`) is stripped from the raw map before shallow-merge (Finding 2).
- **Deterministic beta header:** `betas` is a sorted `BTreeSet` in `ResolvedBetas`; the wire header is `sort_unstable + join(',')` so output is byte-stable across runs (Finding 7).
- **Per-instance behavior knobs live under `ProviderConfig.provider_options`** (opaque `BTreeMap<String, Value>`), not in `coco-config`. Schema is owned here in `provider_options.rs` (`deny_unknown_fields`, defaults match TS `betas.ts`). Settings.json shape: `providers.anthropic.provider_options.{experimental_betas, disable_interleaved_thinking, show_thinking_summaries, non_interactive}`. `services/inference::model_factory::build_anthropic` calls `parse_provider_options` and threads the four typed bools into `AnthropicProviderSettings`. There are intentionally **no `COCO_ANTHROPIC_*` env vars** — settings.json is canonical.
