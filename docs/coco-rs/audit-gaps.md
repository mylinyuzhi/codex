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

`src/utils/permissions/yoloClassifier.ts` (1495 LOC) — two-stage XML classifier system:
- Stage 1: FAST (64 tokens, nudged for quick block decision)
- Stage 2: THINKING (4096+ tokens, full chain-of-thought reasoning)
- Shared prompt prefix for cache hits between stages
- Integrates with:
  - `bashClassifier.ts` — semantic command safety (stub for external builds)
  - `classifierDecision.ts` (98 LOC) — classifier → PermissionDecision mapping
  - `classifierShared.ts` — safe-tool allowlist (read-only tools skip classifier)
  - `denialTracking.ts` (45 LOC) — 3 consecutive or 20 total denials → fallback to prompting
  - `autoModeState.ts` (39 LOC) — session state machine with GrowthBook circuit breaker
  - `dangerousPatterns.ts` — code-exec pattern stripping at auto-mode entry
  - `shellRuleMatching.ts` — wildcard/prefix/exact command matching
  - `PermissionContext.ts` (388 LOC) — decision lifecycle wrapper
  - 26 files total in permissions/ directory

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
- Bridge state (11 fields: replBridgeEnabled, replBridgeConnected, replBridgeSessionActive, etc.)
- Tungsten/tmux integration (5 fields: tungstenActiveSession, tungstenPanelVisible, etc.)
- WebBrowser/Bagel tool (3 fields: bagelActive, bagelUrl, bagelPanelVisible)
- Computer-use MCP state (computerUseMcpState)
- Coordinator mode (coordinatorTaskIndex, viewSelectionMode)
- KAIROS/assistant mode (kairosEnabled)
- Elicitation queue (elicitation.queue)
- Remote agent state (remoteAgentTaskSuggestions, remoteConnectionStatus)
- Thinking toggle (thinkingEnabled)
- Session hooks state (sessionHooks)
- Speculation/pipelining (speculation, speculationSessionTimeSavedMs)
- Fast mode & advisor (fastMode, advisorModel, effortValue)
- Inbox messages (inbox.messages)
- Notifications (notifications.current, notifications.queue)
- Worker sandbox permissions (workerSandboxPermissions)
- Ultraplan state (ultraplanLaunching, isUltraplanMode, etc.)
- Plugin system (plugins.enabled, plugins.disabled, plugins.errors, plugins.installationStatus)
- MCP system (mcp.clients, mcp.tools, mcp.commands, mcp.resources)
- Prompt suggestion (promptSuggestion)

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

### 7. coco-inference: missing 5 major subsystems

| Subsystem | TS source | LOC | What it does |
|-----------|-----------|-----|-------------|
| `claude.ts` | `services/api/claude.ts` | 3,419 | Full query orchestration: streaming/non-streaming, beta headers, system prompt, fallback, media limits |
| `withRetry.ts` | `services/api/withRetry.ts` | 550 | Two-layer retry: exponential backoff + auth-aware retry + fast-mode aware + persistent mode |
| `filesApi.ts` | `services/api/filesApi.ts` | 748 | File upload/download: 500MB limit, retry, path security, concurrency pool |
| `dumpPrompts.ts` | `services/api/dumpPrompts.ts` | 227 | Non-blocking debug trace: fingerprint dedup, JSONL output, session-scoped |
| `utils/auth.ts` | `utils/auth.ts` | 2,002 | OAuth/API key: token refresh pipeline, distributed lock, 401 dedup, AWS/GCP auth |

### 8. coco-compact: 4 undocumented submodules

| Module | What it does |
|--------|-------------|
| `grouping.ts` (64 LOC) | Groups messages at API-round boundaries (by assistant.id, not human turns) |
| `postCompactCleanup.ts` (78 LOC) | Clears 10+ caches after compaction (classifier approvals, memory files, system prompt, etc.) |
| `apiMicrocompact.ts` (154 LOC) | API-level context management (clear_tool_uses, clear_thinking strategies) |
| `timeBasedMCConfig.ts` (44 LOC) | GrowthBook config: 60-min gap threshold, keep-recent=5 |

### 9. coco-shell: 4 undocumented modules

| Module | What it does |
|--------|-------------|
| `shouldUseSandbox.ts` (154 LOC) | Complex decision: sandbox enabled → policy → excluded commands (GrowthBook + user config) → feature flags |
| `destructiveCommandWarning.ts` (103 LOC) | ~20 regex patterns for destructive commands (git reset --hard, rm -rf, kubectl delete, terraform destroy, etc.) |
| `sedEditParser.ts` (200+ LOC) | Sed in-place edit parsing and constraint validation |
| `modeValidation.ts` (116 LOC) | Permission mode auto-allow: acceptEdits → mkdir/touch/rm/mv/cp/sed |

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
| 1 | coco-messages 114 functions | **FIXED**: Documented 15 categories with 114 function signatures in crate-coco-messages.md |
| 2 | Missing core concepts (ContentReplacementState, FileStateCache, etc.) | **FIXED**: Added 9 files to ts-to-rust-mapping.md |
| 3 | coco-permissions auto-mode/yolo | **FIXED**: Full classifier documentation added to crate-coco-permissions.md (two-stage XML, denial tracking, safe-tool allowlist, dangerous patterns, CLAUDE.md integration) |
| 4 | coco-tools 7 missing tools | **FIXED**: Added MCPTool, McpAuthTool, PowerShellTool, REPLTool, SleepTool, SyntheticOutputTool + ScheduleCronTool paths to crate-coco-tools.md |
| 5 | AppState 60+ fields | **P2**: Full state documented during coco-state implementation |
| 6 | ts-to-rust-mapping gaps | **FIXED**: Added 24 previously unmapped files |
| 7 | coco-inference auth + filesApi + retry + claude.ts | **FIXED**: Added auth system (OAuth, token refresh, distributed lock, 401 dedup, AWS/GCP), filesApi (upload/download pipeline, path security), retry engine (two-layer, fast-mode aware, persistent), query options, bootstrap, dump_prompts, HTTP utils to crate-coco-inference.md |
| 8 | coco-compact submodules | **FIXED**: Added grouping (API-round boundaries), postCompactCleanup (10+ cache clears), apiMicrocompact (context strategies), timeBasedMCConfig (GrowthBook config) to crate-coco-compact.md |
| 9 | coco-shell modules | **FIXED**: Added destructiveCommandWarning (20 patterns), shouldUseSandbox (decision logic), modeValidation (acceptEdits auto-allow), bash permission pipeline to crate-coco-shell.md |
| 10 | multi-provider beta headers | **FIXED**: Added 13-row beta header matrix to multi-provider-plan.md |
| 11 | coco-config patterns | **P3**: ConfigSection trait documented during implementation |
| 12 | Hooks 15+ executor files | **FIXED**: Added to ts-to-rust-mapping.md |
| 13 | String→Enum type safety (Round 5) | **FIXED**: ToolId, AgentTypeId, 8 new enums, 31 enum derive annotations, String→ToolId across 10+ docs |
| R5-1 | ToolName enum missing from crate-coco-types.md | **FIXED**: Added 41-variant enum with as_str(), FromStr |
| R5-2 | SubagentType enum missing from crate-coco-types.md | **FIXED**: Added 7-variant enum with as_str(), FromStr |
| R5-3 | ShellType vs ShellKind inconsistency | **FIXED**: Unified to ShellKind in coco-hooks + coco-config |
| R5-4 | EffortValue vs EffortLevel inconsistency | **FIXED**: Unified to EffortLevel in coco-types + coco-inference |
| R5-5 | BuiltinPluginDefinition layer violation (L1→L4) | **P2** |
| R5-6 | SkillDefinition.hooks type mismatch | **P2** |
| R5-7 | MessageRole undefined in coco-messages | **P3** |
| R5-8 | ThinkingLevel name collision (config enum vs inference struct) | **FIXED**: Removed config enum, struct→coco-types, ModelInfo restored |
| R5-9 | TaskStateBase dual definition (types vs tasks) | **P2** |
| R5-10 | OAuthTokens collision (inference vs mcp) | **P2** |

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

---

## Cross-Review Round 3 (Deep TS comparison — April 2026)

### Newly Fixed (this round)

| Gap | What was missing | Fix |
|-----|------------------|-----|
| coco-permissions: full classifier architecture | 74-line stub → TS has 1495-line 2-stage XML classifier | **FIXED**: Full documentation in crate-coco-permissions.md (two-stage, denial tracking, CLAUDE.md integration, safe-tool allowlist, dangerous pattern stripping, security invariants) |
| coco-messages: 114 functions | 7 documented | **FIXED**: 15 categories with signatures in crate-coco-messages.md |
| coco-inference: auth system | "65K LOC" placeholder | **FIXED**: Actual 2002 LOC with OAuth, token refresh, distributed lock, 401 dedup, AWS/GCP auth |
| coco-inference: claude.ts | Not documented | **FIXED**: 3419 LOC query orchestration (streaming/non-streaming, beta headers, fallback, media limits) |
| coco-inference: withRetry.ts | Skeleton only | **FIXED**: 550 LOC two-layer retry (auth-aware, fast-mode, persistent, context overflow) |
| coco-inference: filesApi.ts | Listed only | **FIXED**: 748 LOC upload/download pipeline, path security, concurrency |
| coco-inference: dumpPrompts.ts | Not documented | **FIXED**: 227 LOC non-blocking debug trace |
| coco-inference: bootstrap.ts | Not documented | **FIXED**: 141 LOC lazy-fetch org config |
| coco-compact: grouping.ts | Listed only | **FIXED**: API-round boundary grouping (by assistant.id) |
| coco-compact: postCompactCleanup.ts | Listed only | **FIXED**: 10+ cache clears, main-thread guard |
| coco-compact: apiMicrocompact.ts | Listed only | **FIXED**: API-level clear_tool_uses / clear_thinking strategies |
| coco-compact: timeBasedMCConfig.ts | Listed only | **FIXED**: GrowthBook config (60-min gap, keep-recent=5) |
| coco-shell: destructiveCommandWarning | Listed only | **FIXED**: 20 regex patterns documented |
| coco-shell: shouldUseSandbox | Listed only | **FIXED**: Decision logic (GrowthBook + user config + policy) |
| coco-shell: modeValidation | Listed only | **FIXED**: acceptEdits auto-allow commands |
| coco-shell: bash permissions pipeline | Not documented | **FIXED**: Full 7-step pipeline |

## Cross-Review Round 4 (35-Area TS-First Validation — April 2026)

### Major Corrections

| Issue | What was wrong | Fix |
|-------|---------------|-----|
| Steering ≠ GrowthBook | Area 21 "steering" wrongly identified as feature flags. Actual: mid-turn message queue & injection (user sends guidance while LLM working) | **FIXED**: Added steering section to crate-coco-query.md (QueuedCommand, CommandQueue, QueryGuard, mid-turn attachment injection, inbox) |
| Background execution = v1, not v2 | Both BashTool and AgentTool support `run_in_background`. Task framework is v1 core, coordinator is v2 orchestration on top | **FIXED**: Expanded coco-tasks in modules.md with TaskState union, isBackgrounded, auto-background, task output, notifications |
| Subagent = agent-as-task | Agents are not separate from tasks — they register as LocalAgentTaskState. No separate SubagentManager needed | **FIXED**: Added AgentTool architecture to crate-coco-tools.md (spawn routing, fork, worktree, tool filtering, lifecycle) |
| Prompt cache undocumented | 727 LOC cache break detection algorithm missing from inference doc | **FIXED**: Added CacheScope, CacheBreakDetector, 2-phase detection to crate-coco-inference.md |
| ts-utils-mapping B12 error | forkedAgent.ts etc. wrongly mapped to `memory/` | **FIXED**: Remapped to `coco-tools` (AgentTool submodule) |
| ThinkingConfig boolean | Rust doc had bool+budget, TS has 3-variant union (adaptive/enabled/disabled) | **FIXED**: Changed to enum in crate-coco-inference.md |
| Fast mode stub | Only enum documented, org-level behavior missing | **FIXED**: Added availability check, cooldown semantics, prefetch to crate-coco-config.md |
| Rewind 1-line | Only "Rewind to earlier turn" | **FIXED**: Added mechanism (message selector, file snapshots, restoration) to crate-coco-commands.md |
| FileHistory missing | Not in any doc | **FIXED**: Added FileHistoryState, encoding detection, FileStateCache to crate-coco-context.md |
| OAuth PKCE missing | Auth outline only | **FIXED**: Added 7-step PKCE flow, auth-code listener, crypto to crate-coco-inference.md |

### Stub Expansions

| Crate | What was expanded | Key additions |
|-------|------------------|---------------|
| coco-skills (modules.md) | 3-line load() → full multi-source discovery | Loading order (bundled→plugin→user→project→managed), dedup via realpath, conditional activation, memoization, bundled registry |
| coco-hooks (modules.md) | 6 event types → 15 | FileChanged, CwdChanged, PermissionDenied, PostToolUseFailure, PermissionRequest, Notification, Elicitation, ElicitationResult, WorktreeCreate + AsyncHookRegistry + HookScope priority |
| coco-memory (modules.md) | Basic CRUD → full feature | 4-type taxonomy scope rules, staleness detection (memoryAge), MEMORY.md truncation (200 lines/25KB), Sonnet-based recall selector, two-step save |
| coco-keybindings (modules.md) | 3 struct fields → full system | 18 KeybindingContext variants, 50+ KeybindingAction variants, chord support, platform defaults, reserved shortcuts (ctrl+c/d double-press), user binding merge |
| coco-tasks (modules.md) | Basic TaskManager → full background exec | TaskState 7-variant union, isBackgrounded flag, 3 entry points (explicit/auto/Ctrl+B), task output persistence (5GB cap), `<task-notification>` XML, PlanFileManager CRUD |

### False Positives Identified

| Area | Original assessment | Correction |
|------|-------------------|------------|
| 04 System Reminder | MISSING (CRITICAL) | N/A — Rust architectural improvement, not a TS module port |
| 14 Code Indexing | N/A | Confirmed: TS has only detection (22 tool types), no indexing |
| 21 Steering (GrowthBook) | MISSING (HIGH) | L6 intentionally deferred per mapping. Actual steering is message queue (fixed separately) |
| 26 Background Agents | PARTIAL (HIGH) | Merged into task system (#13). Background exec is v1 core |
| 27 LSP Integration | MISSING (CRITICAL) | cocode strategy (copy from cocode-rs). No TS-first doc needed |

### Remaining Deferred (implementation-time)

| Priority | Gap | Phase |
|----------|-----|-------|
| P2 | AppState: 60+ fields (remote, notifications, attribution, tungsten, speculation, plugins, MCP, inbox) | Phase 7 |
| P2 | ErrorExt::telemetry_msg() — 遥测脱敏方法 (TS 有 TelemetrySafeError，cocode-rs 无对应) | Phase 2 (cocode-error 扩展) |
| P3 | coco-config cocode-rs patterns (ConfigSection trait, ConfigResolver) | Phase 2 |
| P3 | 工具执行错误 errno 保留 — 确保 IO 错误在 OTel 中保留操作系统级 errno | Phase 4 |
| P1 | coco-otel L2: span 层级体系 — cocode-rs 仅 session_span，缺 interaction→tool→hook 嵌套 | Phase 1 |
| P1 | coco-otel L3: ~53 应用事件 — cocode-rs 仅 7 事件，缺 query/session/config/oauth/mcp 等 | Phase 3 |
| P2 | coco-otel L4: 8+ 业务 metrics — 缺 token.usage, cost.usage, lines_of_code, session.count 等 | Phase 3 |
| P2 | coco-otel L5: 自定义 exporter — 缺 BigQuery, 1P Event Logging, Perfetto, Beta tracing | Phase 3 |
| — | coco-otel L6: 运营控制 (event sampling, killswitch, metrics opt-out, GrowthBook) | **暂不实现** |

## Cross-Review Round 5 (String→Enum Audit + Cross-Verification — April 2026)

### String→Enum Type Safety Audit — Completed

Systematic review of all struct/enum definitions across crate docs. Every `String` field evaluated
for enum replacement. 67 String fields confirmed correct (dynamic values); all identity fields
converted to typed enums.

| Change | Scope | Details |
|--------|-------|---------|
| ToolId enum added | coco-types | `Builtin(ToolName) \| Mcp { server, tool } \| Custom(String)` — custom serde via Display/FromStr (flat string wire format) |
| AgentTypeId enum added | coco-types | `Builtin(SubagentType) \| Custom(String)` — same pattern as ToolId |
| HookEventType expanded | coco-types | 7 → 27 variants, `#[non_exhaustive]`, strum derives |
| 6 new enums added | coco-types | MessageKind, HookOutcome, CommandAvailability, CommandSource, UserType, Entrypoint |
| NormalizedMessage redesigned | coco-types | Replaced `role: String` with enum variants User/Assistant |
| tool_name→tool_id | 10+ crate docs | All identity fields across query, permissions, coordinator, remote, tool, hooks |
| agent_type→AgentTypeId | 4 crate docs | tools, tasks, coordinator, tool |
| Tool input enums | coco-tools | GrepOutputMode, ConfigAction, LspAction — all with full derives |
| Context enums | coco-context | Platform, ShellKind — replaced String fields in SystemContext |
| All 31 enums annotated | coco-types | Proper `#[derive]`, `#[serde(rename_all)]`, Copy where applicable, Default where applicable |
| CLAUDE.md updated | CLAUDE.md | Type Ownership, Canonical Names, Document Map — all reflect new enums |

### New Gaps Found (Cross-Verification)

| # | Gap | What's wrong | Severity | Fix |
|---|-----|-------------|----------|-----|
| R5-1 | ToolName enum missing | Referenced by `ToolId::Builtin(ToolName)` but enum not defined | **FIXED** | Added 41-variant enum with as_str(), FromStr, serde to crate-coco-types.md |
| R5-2 | SubagentType enum missing | Referenced by `AgentTypeId::Builtin(SubagentType)` but enum not defined | **FIXED** | Added 7-variant enum with as_str(), FromStr, serde to crate-coco-types.md |
| R5-3 | ShellType vs ShellKind inconsistency | coco-hooks and coco-config used `ShellType` (undefined); coco-context defines `ShellKind` | **FIXED** | Unified to `ShellKind` in coco-hooks.md and coco-config.md |
| R5-4 | EffortValue vs EffortLevel inconsistency | crate-coco-types.md and crate-coco-inference.md used `EffortValue` (undefined); 10+ other refs use `EffortLevel` | **FIXED** | Unified to `EffortLevel` in coco-types.md and coco-inference.md |
| R5-5 | BuiltinPluginDefinition layer violation | coco-types (L1) references `PluginManifest` from coco-plugins (L4) | **Architecture** | Move `BuiltinPluginDefinition` to coco-plugins, or change `manifest` field to `Value` |
| R5-6 | SkillDefinition.hooks type mismatch | Uses `Option<HooksSettings>` but coco-skills doesn't declare coco-hooks dependency | **Architecture** | Change to `Option<Value>` per config isolation pattern (same as Settings.hooks) |
| R5-7 | MessageRole undefined | `filter_by_role(messages, role: MessageRole)` in coco-messages but MessageRole never defined | **Minor** | Replace with `MessageKind` (already defined in coco-types) |

### Type Collision Audit (comprehensive cross-doc review)

Systematically collected all ~238 struct/enum definitions across 27 crate docs.
Cross-referenced with TS source to identify redundancy and collisions.

| # | Collision | Files | Analysis | Resolution |
|---|-----------|-------|----------|------------|
| R5-8 | ThinkingLevel name collision | coco-config (was enum None/Low/Med/High) vs coco-inference (struct {effort, budget, interleaved}) | Config enum had NO TS equivalent — TS uses capability checks. The struct IS needed for multi-provider (cocode-rs proven design). | **FIXED**: Removed config enum. ThinkingLevel struct moved to coco-types as canonical shared type. ModelInfo restored with `default_thinking_level: Option<ThinkingLevel>` and `supported_thinking_levels`. Rationale: multi-provider needs richer thinking abstraction than TS's simple ThinkingConfig. |
| R5-9 | TaskStateBase dual definition | coco-types (11 fields) vs coco-tasks (5 fields) | Different field sets for same type name. coco-types version is more complete. | **P2**: Unify at implementation time. coco-types is canonical owner. |
| R5-10 | OAuthTokens collision | coco-inference (API OAuth: 6 fields) vs coco-mcp (MCP OAuth: 4 fields, different expires_at type) | Genuinely different structs for different OAuth contexts. | **P2**: Rename to `ApiOAuthTokens` / `McpOAuthTokens` at implementation time. |

**Unified to ThinkingLevel only** (EffortLevel + ThinkingConfig eliminated):
- TS has EffortLevel (4 levels) + ThinkingConfig (3 variants) as separate types
- cocode-rs has only ThinkingLevel struct — proven design, no EffortLevel/ThinkingConfig
- ThinkingLevel is a strict superset: can express everything both TS types can, plus budget+interleaved
- ReasoningEffort (6 levels) is the effort dimension WITHIN ThinkingLevel (not a standalone type)
- Flow: user settings → ThinkingLevel → ModelInfo resolution → per-provider API params

### Stale Entry Cleanup

Round 4 "Remaining Deferred" items reviewed — all still valid:
- P2 AppState, P2 telemetry_msg, P3 config patterns, P3 errno, P1/P2 coco-otel L2-L5: no changes

## Cross-Review Round 6 (opencode Multi-Provider Comparison — April 2026)

Deep comparison of coco-rs multi-provider design against opencode (TS, 40+ providers) and cocode-rs (Rust, 6 providers).

### Design Decisions

| # | Decision | Analysis | Outcome |
|---|----------|----------|---------|
| R6-1 | ThinkingLevel.options (HashMap) | cocode-rs used typed fields per provider param (include_thoughts, reasoning_summary, interleaved) → poor extensibility (3 crate changes per new param). opencode uses variant dicts (data-driven, zero code changes for new params). | **ADOPTED**: ThinkingLevel keeps only 2 typed fields (effort, budget_tokens — truly universal). All provider-specific thinking params move to `options: HashMap<String, Value>`. thinking_convert retains `&ModelInfo` for budget validation/clamping, does typed conversion for effort/budget, then merges options as passthrough. |
| R6-2 | default_thinking_level type | Was `Option<ThinkingLevel>` (full struct with params). Causes duplication — same params defined in both default and supported_thinking_levels. | **FIXED**: Changed to `Option<ReasoningEffort>` — just a ref to an entry in supported_thinking_levels. Single source of truth. |
| R6-3 | ModelInfo.slug naming | "slug" is a web URL term. vercel-ai uses `model_id()`, opencode uses `id`, all LLM APIs use `model`. | **RENAMED**: `model_id` in coco-rs plan docs. Aligns with vercel-ai and industry convention. |
| R6-4 | sdk_namespace for Bedrock/Vertex | Proposed adding ProviderInfo.sdk_namespace to route ProviderOptions to different namespaces for same ProviderApi. | **REJECTED**: vercel-ai provider impls handle Bedrock/Vertex parameter differences internally. Adding sdk_namespace would leak provider impl details to config layer. |
| R6-5 | opencode-style variant concept | opencode has `model.variants: Record<string, Record<string, any>>` — named presets user can select. | **NOT ADOPTED as separate concept**: ThinkingLevel.options + supported_thinking_levels achieves the same: user selects effort name, system resolves full param set from supported list. More type-safe. |
| R6-6 | ReasoningSummary enum | Was a typed enum (None/Auto/Concise/Detailed) on ModelInfo. | **REMOVED**: Now a string value in ThinkingLevel.options (e.g., `"reasoningSummary": "auto"`). No longer needs a standalone type. |
| R6-7 | ModelInfo.options vs ThinkingLevel.options | cocode-rs has ModelInfo.options for all extensions. Need clear separation. | **CLARIFIED**: ModelInfo.options = non-thinking per-model params (store:false). ThinkingLevel.options = thinking-related per-effort-level params (reasoningSummary, interleaved). Different merge points in RequestBuilder (Step 4 vs Step 3b). |
| R6-8 | ModelSpec.provider type | Was `ProviderApi` enum. Doesn't support sub-provider routing (bedrock, vertex). | **CHANGED**: `provider: String` (free-form) + `api: ProviderApi` (for dispatch). String supports "bedrock"/"vertex" without enum expansion. Aligned with cocode-rs actual impl. |
| R6-9 | Plan docs missing cocode-rs fields | ModelInfo, ProviderInfo, InferenceContext, RequestBuilder pipeline were incomplete vs cocode-rs actual code. | **FIXED**: All plan docs updated to reflect cocode-rs actual fields (top_k, timeout_secs, shell_type, max_tool_output_chars, ProviderModel, interceptors, request_options_merge module). |
| R6-10 | Capability enum alignment | coco-rs had different variant names than cocode-rs (ToolUse vs ToolCalling, Thinking vs ExtendedThinking). | **FIXED**: Aligned with cocode-rs Capability enum (TextGeneration, ToolCalling, ExtendedThinking, ReasoningSummaries, ParallelToolCalls, etc.). |

### Files Modified

- `crate-coco-types.md`: ThinkingLevel (options field, removed interleaved/max_output_tokens), ModelSpec (provider: String + api), Capability aligned, ApplyPatchToolType aligned
- `crate-coco-config.md`: ModelInfo full alignment (all cocode-rs fields), default_thinking_level as ReasoningEffort, ProviderInfo with models/interceptors, ProviderModel
- `crate-coco-inference.md`: thinking_convert simplified (no ModelInfo param), RequestBuilder full pipeline, request_options_merge module, InferenceContext full fields
- `multi-provider-plan.md`: config examples updated, design decisions table, removed redundant type definitions
- `CLAUDE.md`: type ownership, canonical names updated
