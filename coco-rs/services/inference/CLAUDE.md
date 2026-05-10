# coco-inference

Thin multi-provider LLM client wrapper over `vercel-ai`. Generic retry, usage aggregation, cache-break detection, thinking-level conversion — and **nothing Anthropic-specific** (auth/OAuth/prompt-cache/rate-limit policy live in `vercel-ai-anthropic`, not here).

## TS Source
- `services/api/client.ts` — request dispatch, streaming composition
- `services/api/withRetry.ts` — retry policy shape (backoff, auth retry)
- `services/api/errors.ts`, `services/api/errorUtils.ts` — error classification
- `services/api/logging.ts`, `services/api/dumpPrompts.ts` — request/response logs
- `services/api/promptCacheBreakDetection.ts` — cache break detector
- `services/api/usage.ts`, `services/api/emptyUsage.ts` — usage aggregation

**Intentionally NOT ported here** (these belong in `vercel-ai-anthropic` or are Anthropic-only): `services/api/claude.ts`, `services/api/bootstrap.ts`, `services/api/filesApi.ts`, `services/api/referral.ts`, `services/api/sessionIngress.ts`, `services/api/adminRequests.ts`, `services/api/grove.ts`, `services/api/firstTokenDate.ts`, `services/api/metricsOptOut.ts`, `services/api/overageCreditGrant.ts`, `services/api/ultrareviewQuota.ts`, `services/oauth/`, `services/policyLimits/`, `services/claudeAiLimits.ts`, `services/rateLimitMessages.ts`, `utils/auth.ts`, `utils/betas.ts`.

## Key Types

- `ApiClient`, `QueryParams`, `QueryResult` — wraps `Arc<dyn LanguageModelV4>`
- `RetryConfig` — generic cross-provider retry
- `UsageAccumulator` — token/cost accumulation
- `CacheBreakDetector`, `CacheBreakResult`, `CacheState` — prompt-cache boundary detection
- `StreamEvent`, `synthetic_stream_from_content` — streaming primitives
- `InferenceError`, `ErrorLog`, `RequestLog`, `ResponseLog`, `StopReason`, `KnownGateway`
- `merge_provider_options`, `provider_base_options` — provider option merging
- `generate_tool_schemas`, `filter_schemas_by_model`, `estimate_schema_tokens` — tool schema pipeline
- `cache_convert::to_extra_body` — provider-neutral pass-through emission of `cacheStrategy` / `requestedBetas` (Anthropic-only consumer today)
- `cache_convert::session_context_to_extra_body` — pass-through emission of `agenticQuery` / `querySource`, gated on non-disabled cache strategy (Finding 4)
- `build_call_options_with_extra` — returns `(LanguageModelV4CallOptions, BTreeMap<String, Value>)` so the cache-break detector hashes the merged map directly (Finding 5)
- `ProviderClientFingerprint` — extended with `runtime_state_digest` over `account` + `prompt_cache` + per-provider `provider_options` map; settings-reload that flips any of these triggers a turn-boundary client rebuild (design §19.3 attack γ). Per-provider scoping means a knob flip on one Anthropic instance doesn't churn an unrelated instance's client.

## Design Notes

- Thinking-level conversion (`thinking_convert`): `ThinkingLevel` → per-provider `ProviderOptions` is done here because the mapping (`effort`/`budget_tokens` → provider JSON) is generic. Provider-specific thinking extensions go through `ThinkingLevel.options` (HashMap). The typed-arm emission is gated on `effort.is_explicit_level()`; `Disable` and `Auto` skip the typed arm — `Disable` may carry an explicit-off wire toggle in `level.options` (e.g. DeepSeek's `{"thinking":{"type":"disabled"}}`), `Auto` emits nothing at all so the server-side default applies. `level.options` is passed through unconditionally (including for `Disable`/`Auto`). **`budget_tokens` is faithfully forwarded — when `level.budget_tokens` is `None`, the typed Anthropic arm omits the `budgetTokens` key, and `vercel-ai-anthropic` likewise emits `{"type":"enabled"}` with no budget on the wire (no synthesized default, no `max_tokens` bump). Endpoints that require it must declare a budget at the `ModelInfo` layer.**
- `ApiClient` is provider-agnostic — it holds any `Arc<dyn LanguageModelV4>`, real or mock. This is the only knob callers need; model routing (`ModelRole` → `ModelSpec`) happens in `coco-config` upstream.
- **Cache-strategy emission is pass-through, not policy.** This crate emits the typed signals (`cacheStrategy`, `requestedBetas`, `agenticQuery`, `querySource`) into `provider_options["anthropic"]`. All decisions about whether/how to act on them (1h-TTL eligibility latch, allowlist match, marker placement, beta-header gating) live in `vercel-ai-anthropic` (`cache_policy`, `cache_placement`, `beta_resolver`). The raw map lands in the merged `extra_body` with the underlying signal preserved verbatim — no re-encoding hop.
- **Detector hashes the merged map.** `build_call_options_with_extra` snapshots the merged map BEFORE namespace-wrapping; `client::build_prompt_state_input` hashes the snapshot. Adding new pass-through keys later (e.g. a future `cacheBudget`) auto-tracks without touching the detector — no key-by-key plumbing required (Finding 5).
