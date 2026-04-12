# coco-messages

Message creation (13 functions), normalization (10), filtering (11), predicates (19), lookups (8), history, cost tracking.

## TS Source
- `src/utils/messages.ts` (193K -- the largest utility file)
- `src/utils/messages/` (mappers, system init helpers)
- `src/history.ts` (session history persistence)
- `src/cost-tracker.ts` (token usage, cost tracking)

## Key Types
MessageHistory, NormalizedMessage, CostTracker, TurnRecord
