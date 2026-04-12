# coco-inference

LLM client via vercel-ai, retry engine, auth (OAuth/API key/Bedrock/Vertex), rate limiting, token estimation.

## TS Source
- `src/services/api/` (claude.ts, client.ts, withRetry.ts, errors.ts, logging.ts, usage.ts)
- `src/utils/auth.ts`, `src/services/oauth/`
- `src/services/tokenEstimation.ts`, `src/services/claudeAiLimits.ts`
- `src/services/rateLimitMessages.ts`, `src/services/policyLimits/`
- `src/utils/api.ts` (26K LOC -- tool schema conversion, CacheScope)
- `src/utils/betas.ts` (18 beta headers)
- `src/utils/tokens.ts`, `src/utils/modelCost.ts`
- `src/utils/thinking.ts` (provider-specific thinking config)

## Key Types
ApiClient, ModelHub, RetryContext, AuthProvider, CacheScope, CacheBreakDetector
