# cocode-rs

Reference implementation of a multi-provider LLM SDK and CLI. **Read-only reference** — active development is in `coco-rs/`.

## Architecture (78 crates)

```
┌──────────────────────────────────────────────────────────────────────┐
│  App: cli, tui, session                                              │
│  App Server: app-server, app-server-protocol                         │
├──────────────────────────────────────────────────────────────────────┤
│  Core: loop → executor → inference                                   │
│            ↓        ↓                                                │
│      tools-api ← context ← prompt                                   │
│         ↓                                                            │
│    tools, message, system-reminder, subagent, file-backup            │
├──────────────────────────────────────────────────────────────────────┤
│  Features: skill, hooks, llm-check, plugin, plan-mode, team,        │
│            cron, auto-memory, keybindings, ide                       │
│  Exec: shell, sandbox, arg0, process-hardening                       │
│  MCP: mcp-types, rmcp-client                                         │
│  Standalone: retrieval, lsp                                           │
├──────────────────────────────────────────────────────────────────────┤
│  Provider SDKs: anthropic, openai, volcengine-ark, z-ai,            │
│                 google-genai, hyper-sdk                               │
│  Vercel AI: ai → openai, openai-compatible, google, anthropic,       │
│                  bytedance (on provider + provider-utils)              │
├──────────────────────────────────────────────────────────────────────┤
│  Common: protocol, config, policy, error, otel, stack-trace-macro    │
│  Utils: 24 utility crates                                             │
└──────────────────────────────────────────────────────────────────────┘
```

## Layer Summary

| Layer | Count | Crates |
|-------|-------|--------|
| Common | 6 | error, protocol, config, policy, otel, stack-trace-macro |
| Core | 11 | inference, message, tools-api, tools, context, prompt, system-reminder, loop, subagent, executor, file-backup |
| Provider SDKs | 6 | anthropic, openai, volcengine-ark, z-ai, google-genai, hyper-sdk |
| Vercel AI | 8 | provider, provider-utils, ai, openai, openai-compatible, google, anthropic, bytedance |
| Features | 10 | skill, hooks, llm-check, plugin, plan-mode, team, cron, auto-memory, keybindings, ide |
| Exec | 4 | shell, sandbox, arg0, process-hardening |
| MCP | 2 | mcp-types, rmcp-client |
| App | 3 | cli, tui, session |
| App Server | 2 | app-server, app-server-protocol |
| Standalone | 2 | retrieval, lsp |
| Utils | 24 | absolute-path, apply-patch, async-utils, cache, cargo-bin, common, file-encoding, file-ignore, file-search, file-watch, git, image, json-to-toml, keyring-store, pty, readiness, rustls-provider, secret-redact, shell-parser, sleep-inhibitor, stdio-to-uds, stream-parser, string, symbol-search |

## Key Differences from coco-rs

| cocode-rs | coco-rs | Notes |
|-----------|---------|-------|
| `common/protocol` | `common/types` | Renamed, zero-dep foundational types |
| `core/loop`, `core/executor` | `app/query` | Agent loop moved to app layer as QueryEngine |
| `core/tools-api` | `core/tool` | Tool trait + executor split out |
| `core/prompt`, `core/system-reminder` | Merged into `core/context` | Context assembly consolidated |
| `features/*` (10 crates) | Root modules (7 crates) | commands, skills, hooks, tasks, memory, plugins, keybindings |
| `provider-sdks/*` (6 crates) | Removed | Provider logic consolidated into vercel-ai layer |
| `exec/arg0` | `exec/process-hardening` | Renamed |
| No equivalent | `services/` (6 crates) | New layer: inference, compact, mcp, rmcp-client, mcp-types, lsp |
| No equivalent | `app/query`, `app/state` | New app crates for agent loop + state tree |
| No equivalent | `bridge` | IDE bridge (VS Code/JetBrains) + REPL bridge |
| `app-server`, `app-server-protocol` | Removed | Server functionality integrated into cli |
| `features/llm-check`, `features/cron`, `features/ide` | Absorbed | Merged into other crates |
| 24 utils | 27 utils | Added: cursor, frontmatter, test-harness |

## Agent Turn Lifecycle

```
User input
  → SystemReminderOrchestrator.generate_all()    [system-reminder]
  → SystemPromptBuilder.build()                   [prompt]
  → ToolRegistry.definitions_for_model()          [tools-api]
  → ApiClient.stream_request()                    [inference → hyper-sdk → provider-sdks]
  → StreamProcessor yields StreamEvents           [inference]
  → StreamingToolExecutor:
      safe tools → execute concurrently           [tools]
      unsafe tools → queue, execute after stop    [tools]
  → HookRegistry: PreToolUse / PostToolUse        [hooks]
  → Tool results → MessageHistory.add_turn()      [message]
  → If tool calls exist → loop back
  → If needs compaction → micro-compact or session memory
  → Emit LoopEvent::TurnCompleted                 [protocol]
```

## Key Types

| Crate | Primary Types |
|-------|---------------|
| `protocol` | `ModelInfo`, `ProviderApi`, `ProviderInfo`, `ModelRole`, `PermissionMode`, `LoopEvent`, `LoopConfig`, `Feature`, `SecurityRisk`, `Capability` |
| `config` | `ConfigManager` (RwLock), `ConfigLoader`, `ConfigResolver`, `Config`, `RuntimeOverrides`, `RoleSelections` |
| `inference` | `ApiClient` (wraps hyper-sdk::Model), `UnifiedStream`, `StreamingQueryResult`, `CollectedResponse`, `RetryContext` |
| `loop` | `AgentLoop` (run → LoopResult), `AgentStatus` (watch::Sender), compaction + fallback logic |
| `executor` | `AgentExecutor`, `ExecutorBuilder`, permission pipeline: `PermissionRule` → `PermissionRuleEvaluator` → `ApprovalStore` |
| `tools-api` | `Tool` trait (5-stage pipeline), `ToolContext`, `StreamingToolExecutor`, `ToolRegistry`, `FileTracker` |
| `message` | `TrackedMessage`, `MessageSource`, `Turn`, `MessageHistory` |
| `context` | `ConversationContext`, `ContextBudget`, `BudgetCategory`, `EnvironmentInfo` |
| `prompt` | `SystemPromptBuilder`, `PromptSection` (14 sections), injection positions |
| `system-reminder` | `SystemReminderOrchestrator`, `SystemReminder` (tiers), generators: ChangedFiles/PlanMode/TodoReminders/LspDiagnostics |
| `subagent` | `AgentDefinition`, `SubagentManager`, `AgentInstance`, foreground vs background spawning |
| `hyper-sdk` | Unified provider abstraction over all provider SDKs |
| `tui` | `App`, `AppState`, `TuiEvent`, `UserCommand`, `Overlay`, Elm (TEA) architecture |
