# Plan Audit: Comprehensive Gap Analysis

Exhaustive comparison of all plan docs against actual TS source + cocode-rs source.

## Critical Gaps (must fix before implementation)

### 1. coco-messages: 100+ missing functions

`src/utils/messages.ts` exports **114 functions** — plan documents only 7.

**Missing categories:**
- 15 message creation helpers (createUserInterruptionMessage, createSyntheticUserCaveatMessage, etc.)
- 10 normalization functions (mergeUserMessages, mergeAssistantMessages, reorderAttachmentsForAPI)
- 8 tool result handlers (ensureToolResultPairing, filterUnresolvedToolUses, stripToolReferenceBlocks)
- 6 message lookup functions (buildMessageLookups, getSiblingToolUseIDs, getToolResultIDs)
- 5 streaming types (StreamingToolUse, StreamingThinking, handleMessageFromStream)
- 10 compact boundary functions (isCompactBoundaryMessage, findLastCompactBoundaryIndex)
- 20+ system message creators (createPermissionRetryMessage, createBridgeStatusMessage, etc.)
- 30+ utility functions (deriveShortMessageId, stripSignatureBlocks, wrapInSystemReminder, etc.)

**Action**: Don't enumerate all 114 functions in the plan. Instead, document the **categories** and the **core interface**. Implementation will translate function-by-function from TS.

### 2. Missing core concepts not in any plan doc

| Concept | TS source | What it is | Should be in |
|---------|-----------|------------|-------------|
| `ContentReplacementState` | `utils/toolResultStorage.ts` | State machine for tool result size budgets per message | `coco-context` |
| `FileStateCache` | `utils/fileStateCache.ts` (1479 LOC) | LRU cache of file contents before tool execution | `coco-context` |
| `FileHistoryState` | `utils/fileHistory.ts` | Tracks file edits per turn for change detection/undo | `coco-messages` or `coco-context` |
| `processUserInput/` | `utils/processUserInput/` (4 files) | Pre-processes user input (images, slash commands, bash) | `coco-query` |
| `utils/tokens.ts` | Token extraction from messages/API responses | `coco-inference` |
| `utils/api.ts` (26K LOC) | Tool schema conversion, CacheScope, system prompt blocks | `coco-inference` |
| `utils/modelCost.ts` | Per-model pricing calculations | `coco-inference` |
| `utils/worktree.ts` (600 LOC) | Git worktree management | `coco-tools` |
| `utils/theme.ts` | Theme management | `coco-tui` |

### 3. coco-permissions: auto-mode/yolo classifier not documented

`src/utils/permissions/yoloClassifier.ts` (500+ LOC) — entire auto-approve system. Uses LLM (Haiku) to classify bash commands as safe/unsafe without prompting. Integrates with:
- `bashClassifier.ts` (500+ LOC) — ML-based command safety
- `denialTracking.ts` — escalation after repeated denials
- `autoModeState.ts` — state machine for auto-mode
- 24 files total in permissions/ directory

**Action**: Document the **permission evaluation pipeline** as a flowchart, not list every function.

### 4. coco-tools: 7 missing tools

| Tool | Status | Notes |
|------|--------|-------|
| `MCPTool` | **FIXED** | MCP tool proxy (passthrough schema) |
| `McpAuthTool` | **FIXED** | MCP OAuth authentication |
| `PowerShellTool` (14 files) | **FIXED** | Windows-only, CLM security analysis |
| `REPLTool` | **FIXED** | REPL mode (wraps primitive tools) |
| `SleepTool` | **FIXED** | Wait/sleep tool (PROACTIVE/KAIROS gate) |
| `SyntheticOutputTool` | **FIXED** | SDK-only structured output |
| `ScheduleCronTool` path | **FIXED** | Added TS source paths for CronCreate/Delete/List |

### 5. coco-app/state: AppState has 60+ fields, plan has ~10

**Missing entire subsystems in AppState:**
- Bridge state (12 fields: replBridgeEnabled, replBridgeConnected, etc.)
- Tungsten/tmux integration (5 fields)
- WebBrowser/Bagel tool (3 fields)
- Computer-use MCP state (7 sub-fields)
- Coordinator mode (3 fields)
- KAIROS/assistant mode (2 fields)
- Elicitation queue
- Remote agent state
- Thinking toggle
- Session hooks state

### 6. ts-to-rust-mapping.md: 8 unmapped TS util files

| File | LOC | Belongs in |
|------|-----|-----------|
| `utils/processUserInput/` | 4 files | `coco-query` |
| `utils/fileHistory.ts` | 200+ | `coco-context` |
| `utils/tokens.ts` | 100+ | `coco-inference` |
| `utils/api.ts` | 26K | `coco-inference` |
| `utils/worktree.ts` | 600+ | `coco-tools` |
| `utils/modelCost.ts` | 200+ | `coco-inference` |
| `utils/theme.ts` | 200+ | `coco-tui` |
| `utils/config.ts` | 600+ | `coco-config` (GlobalConfig — already partially added) |

### 7. coco-inference: missing 3 major subsystems

| Subsystem | TS source | LOC | What it does |
|-----------|-----------|-----|-------------|
| `filesApi.ts` | `services/api/filesApi.ts` | 600+ | File uploads API (500MB limit, retry, download) |
| `dumpPrompts.ts` | `services/api/dumpPrompts.ts` | 200+ | Debug: cache last 5 API requests to JSONL |
| `utils/auth.ts` | `utils/auth.ts` | 65K | Full auth system: OAuth, Bedrock, Vertex, Foundry, API key |

### 8. coco-compact: 4 undocumented submodules

| Module | What it does |
|--------|-------------|
| `grouping.ts` | Groups messages at API-round boundaries (for reactive compact) |
| `postCompactCleanup.ts` | Clears 10+ caches after compaction |
| `apiMicrocompact.ts` | API-level micro compaction (different from tool-level) |
| `timeBasedMCConfig.ts` | Time-based model context window configuration |

### 9. coco-shell: 4 undocumented modules

| Module | What it does |
|--------|-------------|
| `shouldUseSandbox.ts` (150+ LOC) | Complex decision logic: sandbox enabled → policy → excluded commands → feature flags |
| `destructiveCommandWarning.ts` | Warning system for destructive commands |
| `sedEditParser.ts` (200+ LOC) | Sed in-place edit parsing and validation |
| `modeValidation.ts` | Sandbox mode validation |

### 10. multi-provider-plan.md: missing provider-specific details

| Gap | What's missing |
|-----|---------------|
| Beta headers matrix | Which headers sent to which provider (10+ headers, 4+ providers) |
| Prompt caching by provider | Anthropic supports it with CacheScope; OpenAI doesn't |
| Streaming differences | Model-specific stream event handling |
| Message normalization across providers | How multi-turn messages adapt when switching providers |
| OpenAI Responses API | When to use Chat vs Responses, format differences |

### 11. coco-config: cocode-rs patterns not documented

| Pattern | What it is |
|---------|-----------|
| `ConfigSection` trait | Each config section implements `from_overrides()`, `from_env()`, `merge_json()`, `finalize()` |
| `ConfigResolver` | Resolves relative paths, merges model defaults with user overrides |
| `ConfigManager` with `RwLock` | Thread-safe config access with poison recovery |
| Provider/model JSON files | Separate `providers.json` and `models.json` config files in cocode-rs |

### 12. Hooks system: 15+ executor files not documented

`src/utils/hooks/` has 15+ files. Plan mentions 4 executor types (bash, prompt, http, agent). Missing:
- `fileChangedWatcher.ts` — file change hooks
- `hookEvents.ts` — event type definitions
- `hooksConfigManager.ts` — config management
- `postSamplingHooks.ts` — post-sampling hook pipeline
- `registerFrontmatterHooks.ts` — frontmatter hook registration
- `registerSkillHooks.ts` — skill-level hook registration
- `sessionHooks.ts` — session lifecycle hooks
- `skillImprovement.ts` — skill improvement hooks
- `ssrfGuard.ts` — SSRF protection for HTTP hooks

---

## Fix Status

| # | Gap | Status |
|---|-----|--------|
| 1 | coco-messages 114 functions | **P2**: Document by category during implementation |
| 2 | Missing core concepts (ContentReplacementState, FileStateCache, etc.) | **FIXED**: Added 9 files to ts-to-rust-mapping.md |
| 3 | coco-permissions auto-mode/yolo | **P1**: Document during implementation |
| 4 | coco-tools 7 missing tools | **FIXED**: Added MCPTool, McpAuthTool, PowerShellTool, REPLTool, SleepTool, SyntheticOutputTool + ScheduleCronTool paths to crate-coco-tools.md |
| 5 | AppState 60+ fields | **P2**: Full state documented during coco-state implementation |
| 6 | ts-to-rust-mapping gaps | **FIXED**: Added 24 previously unmapped files |
| 7 | coco-inference auth + filesApi | **P1**: Document during implementation |
| 8 | coco-compact submodules | **P3**: Document during implementation |
| 9 | coco-shell modules | **P3**: Document during implementation |
| 10 | multi-provider beta headers | **FIXED**: Added 13-row beta header matrix to multi-provider-plan.md |
| 11 | coco-config patterns | **P3**: ConfigSection trait documented during implementation |
| 12 | Hooks 15+ executor files | **FIXED**: Added to ts-to-rust-mapping.md |

### Remaining P1 items (to be documented during implementation, not in plan):
- Permissions auto-mode/yolo classifier flow (coco-permissions)
- Auth system 65K LOC (coco-inference)
- These are **implementation detail** — the plan correctly identifies the crate boundaries and TS sources

---

## Cross-Review Fixes (CLAUDE.md audit)

| Issue | What was wrong | Fix |
|-------|---------------|-----|
| 13 type inconsistencies | PermissionResult/PermissionDecision, check_permission/check_permissions, ApiProvider/ProviderApi, Option<i64>/i64 | **FIXED**: Canonical names in CLAUDE.md, updated crate docs |
| 8 redundancies | ModelInfo, ProviderApi defined in 3 places | **FIXED**: multi-provider-plan.md now defers to crate docs |
| Missing dependency sections | 7 crate docs had no Dependencies block | **FIXED**: Added to messages, compact, commands, shell, permissions, tools, modules, app |
| ToolResult.context_modifier | Referenced ToolUseContext from coco-types (circular) | **FIXED**: Removed from coco-types, handled by Tool::modify_context_after() |
| HooksSettings in coco-types | L1 type referencing L4 type | **FIXED**: Changed to `Option<Value>` in PromptCommandData |
| compact uses ToolUseContext | coco-compact shouldn't depend on coco-tool | **FIXED**: Changed to `&ApiClient` parameter |
| Missing TS mappings | MagicDocs, toolUseSummary, setup.ts | **FIXED**: Added to ts-to-rust-mapping.md |

---

## Cross-Review Round 2 (TS file-by-file + architecture deep dive)

### TS Mapping Gaps — FIXED

| Gap | Items | Fix |
|-----|-------|-----|
| Unmapped services/ files | 6 files (awaySummary, diagnosticTracking, internalLogging, mcpServerApproval, preventSleep, claudeAiLimitsHook) | **FIXED**: Added to ts-to-rust-mapping.md |
| Unmapped utils/ dirs | 4 dirs (filePersistence, dxt, deepLink, background) | **FIXED**: Added to ts-to-rust-mapping.md |
| Voice files not enumerated | services/voice*.ts catch-all → 3 specific files | **FIXED**: Enumerated voiceKeyterms, voiceStreamSTT, voice.ts |
| React hooks business logic | 16 hooks with substantial non-React logic | **FIXED**: Added "React Hooks with Business Logic" table to ts-to-rust-mapping.md |
| Stale counts | v1=55, total=75 | **FIXED**: Updated to v1=63, total=87 |

### Architecture Gaps — FIXED

| Gap | What was wrong | Fix |
|-----|---------------|-----|
| ToolUseContext under-specified | 15 fields documented, TS has 40+ | **FIXED**: Expanded to 40+ fields in crate-coco-tool.md with all callbacks, tracking sets, flags |
| Tool trait missing methods | 6 must-port methods not documented | **FIXED**: Added inputs_equivalent, prepare_permission_matcher, to_auto_classifier_input, get_path, backfill_observable_input, output_schema, modify_context_after + 8 v2 methods commented |
| StreamingToolExecutor behavior | "Tools execute after streaming" | **FIXED**: Documented that tools execute DURING API streaming. Added SyntheticToolError enum, context modifier stacking, progress handling |
| QueryEngine missing features | SDKPermissionDenial, orphanedPermission, snipReplay not documented | **FIXED**: Added 6 new fields to QueryEngine, 7 new fields to QueryEngineConfig, 4 new types |
| Message types incomplete | 5 variants, TS has 8+ | **FIXED**: Expanded to 8 variants + 14 system message sub-types + NormalizedMessage + StreamEvent + MessageOrigin |
| crate-coco-app.md missing TS source | No combined TS source header | **FIXED**: Added TS source line |

### Remaining Deferred (implementation-time)

| Priority | Gap | Phase |
|----------|-----|-------|
| P1 | Permissions auto-mode/yolo classifier (500+ LOC) | Phase 3 |
| P1 | Auth system (65K LOC) — OAuth, Bedrock, Vertex, Foundry | Phase 3 |
| P2 | coco-messages: 114 functions (document by category) | Phase 4 |
| P2 | AppState: 60+ fields (remote, notifications, attribution, tungsten) | Phase 7 |
| P2 | ErrorExt::telemetry_msg() — 遥测脱敏方法 (TS 有 TelemetrySafeError，cocode-rs 无对应) | Phase 2 (cocode-error 扩展) |
| P3 | coco-compact submodules | Phase 3 |
| P3 | coco-shell submodules | Phase 4 |
| P3 | coco-config cocode-rs patterns | Phase 2 |
| P3 | 工具执行错误 errno 保留 — 确保 IO 错误在 OTel 中保留操作系统级 errno | Phase 4 |
| P1 | coco-otel L2: span 层级体系 — cocode-rs 仅 session_span，缺 interaction→tool→hook 嵌套 | Phase 1 |
| P1 | coco-otel L3: ~53 应用事件 — cocode-rs 仅 7 事件，缺 query/session/config/oauth/mcp 等 | Phase 3 |
| P2 | coco-otel L4: 8+ 业务 metrics — 缺 token.usage, cost.usage, lines_of_code, session.count 等 | Phase 3 |
| P2 | coco-otel L5: 自定义 exporter — 缺 BigQuery, 1P Event Logging, Perfetto, Beta tracing | Phase 3 |
| — | coco-otel L6: 运营控制 (event sampling, killswitch, metrics opt-out, GrowthBook) | **暂不实现** |
| — | services/analytics/ 映射修正 — 从 coco-inference 移至 coco-otel | **FIXED** |
