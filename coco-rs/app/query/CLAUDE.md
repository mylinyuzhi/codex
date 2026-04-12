# coco-query

Multi-turn agent loop, single-turn execution, token budget, steering, mid-turn injection.

## TS Source
- `src/QueryEngine.ts` (46.6K -- multi-turn agent loop)
- `src/query.ts` (68.7K -- single-turn execution)
- `src/query/` (token budget, query config)
- `src/utils/processUserInput/` (user input pre-processing)
- `src/utils/suggestions/` (command/directory/history suggestions)

## Key Types
QueryEngine, QueryConfig, BudgetTracker, CommandQueue, QueryGuard, PromptInputMode
