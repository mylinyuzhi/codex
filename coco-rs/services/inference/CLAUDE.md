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

## Design Notes

- Thinking-level conversion (`thinking_convert`): `ThinkingLevel` → per-provider `ProviderOptions` is done here because the mapping (`effort`/`budget_tokens` → provider JSON) is generic. Provider-specific thinking extensions go through `ThinkingLevel.options` (HashMap).
- `ApiClient` is provider-agnostic — it holds any `Arc<dyn LanguageModelV4>`, real or mock. This is the only knob callers need; model routing (`ModelRole` → `ModelSpec`) happens in `coco-config` upstream.
