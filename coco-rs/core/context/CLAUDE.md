# coco-context

System context assembly: git status, cwd, env info, CLAUDE.md discovery, attachments, file history.

## TS Source
- `src/context.ts` (system/user context injection)
- `src/utils/claudemd.ts` (46K -- CLAUDE.md discovery + loading)
- `src/utils/attachments.ts` (4K LOC -- files, PDFs, memories, hooks)
- `src/utils/cwd.ts` (working directory management)
- `src/utils/systemPromptType.ts` (system prompt building)
- `src/services/AgentSummary/` (agent activity summary)
- `src/utils/fileHistory.ts` (~1110 LOC -- file edit tracking per turn)
- `src/utils/toolResultStorage.ts` (ContentReplacementState)
- `src/utils/fileStateCache.ts` (LRU file read cache)
- `src/utils/pasteStore.ts`, `src/utils/filePersistence/`
- `src/services/awaySummary.ts`

## Key Types
ConversationContext, EnvironmentInfo, Platform, ShellKind, FileHistoryState
