# CLAUDE.md

Multi-provider LLM SDK and CLI. All development in `cocode-rs/`.

## Commands

Run from `cocode-rs/` directory:

```bash
just fmt          # After Rust changes (auto-approve)
just pre-commit   # REQUIRED before commit
just test         # If changed vercel-ai or core crates
just check        # Type-check all crates
just clippy       # Run clippy on all crates
just help         # All commands
```

## Code Style

### Format and Lint

- When using `format!` and you can inline variables into `{}`, always do that
  ```rust
  // Correct
  format!("{name} is {age}")
  // Avoid
  format!("{} is {}", name, age)
  ```
- Always collapse if statements per [collapsible_if](https://rust-lang.github.io/rust-clippy/master/index.html#collapsible_if)
- Use method references over closures when possible per [redundant_closure_for_method_calls](https://rust-lang.github.io/rust-clippy/master/index.html#redundant_closure_for_method_calls)
- When possible, make `match` statements exhaustive and avoid wildcard arms

### Integer Types

- Use `i32`/`i64` instead of `u32`/`u64` for most cases
- This avoids subtle overflow bugs and matches common API conventions

### Error Handling

- Never use `.unwrap()` in non-test code
- Use `?` for propagation or `.expect("reason")` with clear context
- Avoid mixing `Result` and `Option` without clear conversion
- See Error Handling section below for per-layer error types

### Serde Conventions

- Add `#[serde(default)]` for optional config fields
- Add `#[derive(Default)]` for structs used with `..Default::default()`
- Use `#[serde(rename_all = "snake_case")]` for enums

### Parameter Design

- Avoid bool or ambiguous `Option` parameters that force callers to write hard-to-read code such as `foo(false)` or `bar(None)`
- Prefer enums, named methods, newtypes, or other idiomatic Rust API shapes when they keep the callsite self-documenting

### Argument Comments

When you cannot avoid a small positional-literal callsite, use an exact `/*param_name*/` comment before opaque literal arguments:

```rust
// Good — self-documenting
connect(/*timeout*/ None, /*retries*/ 3, /*verbose*/ false)

// Bad — opaque
connect(None, 3, false)
```

Rules:
- Add `/*param*/` for `None`, booleans, and numeric literals passed by position
- Do not add for string or char literals unless the comment adds real clarity
- The parameter name must exactly match the callee signature

### Module Size

- Target Rust modules under 500 LoC (excluding tests)
- If a file exceeds ~800 LoC, add new functionality in a new module instead of extending the existing file
- Prefer adding new modules over growing existing ones

### Comments

- Keep concise — describe purpose, not implementation details
- Field docs: 1-2 lines max, no example configs/commands
- Code comments: state intent only when non-obvious

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│  App: cli, tui, session                                              │
├──────────────────────────────────────────────────────────────────────┤
│  Core: loop → executor → api                                        │
│            ↓        ↓                                                │
│         tools ← context ← prompt                                    │
│            ↓                                                         │
│      message, system-reminder, subagent, file-backup                 │
├──────────────────────────────────────────────────────────────────────┤
│  Features: skill, hooks, plugin, plan-mode, team, auto-memory        │
│  Exec: shell, sandbox, arg0                                          │
│  MCP: mcp-types, rmcp-client                                         │
│  Standalone: retrieval, lsp                                           │
├──────────────────────────────────────────────────────────────────────┤
│  Vercel AI: ai → openai, openai-compatible, google, anthropic,       │
│                  bytedance (on provider + provider-utils)              │
├──────────────────────────────────────────────────────────────────────┤
│  Common: protocol, config, error, otel, stack-trace-macro             │
│  Utils: 20 utility crates (see table below)                           │
└──────────────────────────────────────────────────────────────────────┘
```

## Key Data Flows

### Agent Turn Lifecycle (18-step cycle in `core/loop`)

```
User input
  → SystemReminderOrchestrator.generate_all()    [system-reminder]
  → SystemPromptBuilder.build()                   [prompt]
  → ToolRegistry.definitions_for_model()          [tools]
  → ApiClient.stream_request()                    [api → vercel-ai]
  → StreamProcessor yields StreamEvents           [api]
  → StreamingToolExecutor:
      safe tools → execute concurrently           [tools]
      unsafe tools → queue, execute after stop    [tools]
  → HookRegistry: PreToolUse / PostToolUse        [hooks]
  → Tool results → MessageHistory.add_turn()      [message]
  → If tool calls exist → loop back to step 1
  → If needs compaction → micro-compact or session memory
  → Emit LoopEvent::TurnCompleted                 [protocol]
```

### Configuration Resolution

```
~/.cocode/*provider.json + *model.json + config.json + env vars
  → ConfigLoader → ConfigResolver → ConfigManager
  → RuntimeOverrides (per-role ModelSpec + ThinkingLevel)
  → Config snapshot → core/executor → core/loop
```

### Shell Execution Flow

```
Bash tool → ShellExecutor.execute(CommandInput)
  → analyze_command_safety() [shell-parser: 15 analyzers]
  → SandboxSettings.is_sandboxed() check
  → spawn shell with CWD tracking markers
  → CommandResult (exit_code, stdout, stderr, new_cwd)
```

## Crate Guide (59 crates)

### Common (5)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `protocol` | Foundational types shared across all crates | `ModelInfo`, `ProviderApi` (6 providers), `ProviderInfo`, `ModelRole` (Main/Fast/Vision/Review/Plan/Explore), `PermissionMode`, `LoopEvent`, `LoopConfig`, `Feature` (14 toggleable features), `SecurityRisk`, `Capability` |
| `config` | Layered config: JSON files + env + runtime overrides | `ConfigManager` (thread-safe, `RwLock`), `ConfigLoader`, `ConfigResolver`, `Config` (complete snapshot), `RuntimeOverrides`, `RoleSelections` |
| `error` | Unified errors with StatusCode classification | `StatusCode` (5-digit `XX_YYY`), `ErrorExt` trait (`status_code()`, `is_retryable()`, `retry_after()`), snafu + snafu-virtstack |
| `otel` | OpenTelemetry tracing and metrics | `OtelManager` (counters, histograms, timers, session span) |
| `stack-trace-macro` | Proc macro for snafu error enums with virtual stack traces | `#[stack_trace_debug]` (proc macro attribute) |

### Core (10)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `api` | Provider-agnostic LLM client with retry, fallback, stall detection | `ApiClient` (wraps `hyper-sdk::Model`), `UnifiedStream` (streaming/non-streaming abstraction), `StreamingQueryResult`, `CollectedResponse`, `RetryContext` |
| `message` | Conversation history with turn tracking and compaction | `TrackedMessage` (uuid, turn_id, source, tombstoned), `MessageSource` (User/Assistant/System/Tool/Subagent/SystemReminder), `Turn` (user+assistant+tool_calls+usage), `MessageHistory` (turns, compaction, micro-compact) |
| `tools` | Tool trait, 5-stage pipeline, concurrent execution | `Tool` trait (validate→check_permission→execute→post_process→cleanup), `ToolContext` (cwd, permissions, shell_executor, lsp_manager, cancel_token, spawn_agent_fn), `StreamingToolExecutor` (safe=concurrent, unsafe=queued), `ToolRegistry`, `FileTracker` |
| `context` | Context window assembly and token budgeting | `ConversationContext` (environment, budget, tool_names, memory_files, injections), `ContextBudget` with `BudgetCategory` (SystemPrompt/Tools/Memory/Messages/Output/Reserved), `EnvironmentInfo` |
| `prompt` | System prompt from 14 embedded templates | `SystemPromptBuilder` (pure sync string assembly), `PromptSection` (Identity/ToolPolicy/Security/GitWorkflow/TaskManagement/MCP/Environment/Permission/MemoryFiles/Injections), injection positions: BeforeTools/AfterTools/EndOfPrompt |
| `system-reminder` | Per-turn dynamic context injection | `SystemReminderOrchestrator` (parallel generators), `SystemReminder` (tier: MainAgentOnly/UserPrompt/Always), generators: ChangedFiles/PlanMode/TodoReminders/LspDiagnostics/NestedMemory, injected as `is_meta: true` user messages with `<system-reminder>` XML tags |
| `loop` | Agent loop driver: multi-turn 18-step cycle | `AgentLoop` (`run(prompt) → LoopResult`), compaction (micro-compact: clear old tool results; session memory: background summarization agent), fallback (stream→non-stream, context overflow→reduce max_tokens 25%, model fallback), `AgentStatus` via `watch::Sender` |
| `subagent` | Isolated agent spawning for parallel tasks | `AgentDefinition` (tools, disallowed_tools, identity, max_turns), `SubagentManager` (register, spawn_full), `AgentInstance` (Running/Completed/Failed/Backgrounded), four-layer tool filtering, foreground (blocks) vs background (output_file) |
| `executor` | Top-level session driver wiring all components | `AgentExecutor` (`execute(prompt) → String`), `ExecutorBuilder` → `AgentExecutor` → `AgentLoop` → `StreamingToolExecutor`, permission pipeline: `PermissionRule` → `PermissionRuleEvaluator` → `ApprovalStore` |
| `file-backup` | File backup and snapshot management with diff-based checkpoints | `FileBackupStore` (backup coordination, `Mutex`-based), `SnapshotManager`, `BackupEntry`, `BackupIndex`, `TurnSnapshot` (turn-level state), `RewindResult` |

### App (3)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `cli` | CLI entry point via `arg0_dispatch_or_else(cli_main)` | `Cli` (clap), `Commands` (Chat/Config/Resume/Sessions/Status), profile support, `--no-tui` for REPL mode |
| `tui` | Terminal UI using Elm architecture (TEA) | `App` (async run loop), `AppState` (model), `TuiEvent` (messages: keyboard/agent/file-search), `UserCommand` (TUI→Core: SubmitInput/Interrupt/ApprovalResponse), `Overlay` (permission prompts, model picker), `tokio::select!` on agent_rx + events + file_search + symbol_search channels |
| `session` | Session persistence and state aggregation | `Session` (id, timestamps, model, working_dir), `SessionState` (message_history + tool_registry + hook_registry + executor), `SessionManager` (create/load/save/list), JSON persistence at `~/.cocode/sessions/` |

### Features (6)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `skill` | `/command` slash commands from bundled/project/user/plugin sources | `SkillInterface` (name, description, prompt), `SkillManager` (loading, validation, dedup), `SkillScanner` (multi-source discovery), `BundledSkill` (SHA-256 fingerprint) |
| `hooks` | Pre/post event interception with scoped priority | `HookDefinition` (event + matcher + handler), `HookEventType` (BeforeToolCall/AfterToolCall/SessionStart), `HookHandler` (Command/Prompt/Agent/Webhook/Inline), `HookScope` (Skill > Plugin > Project > User > Global), `AsyncHookTracker` |
| `plugin` | Plugin system via PLUGIN.toml manifests | `PluginManifest` (PLUGIN.toml), `PluginContributions` (skills, hooks, agents, commands, MCP servers), `PluginScope` (Managed > User > Project), `PluginLoader`, `PluginRegistry` |
| `plan-mode` | Plan file management for plan-then-execute workflow | `PlanFileManager` (CRUD), `PlanModeState`, plans at `~/.cocode/plans/{adjective}-{action}-{noun}.md` |
| `team` | Multi-agent team orchestration with dual-layer persistence | `Team` (container), `TeamMember` (agent definition), `TeamStore` (`Arc<Mutex<T>>`), `Mailbox` (JSONL inter-agent messaging), `AgentMessage` (envelope), `ShutdownTracker`, `TeamConfig` |
| `auto-memory` | Persistent cross-session knowledge via per-project MEMORY.md | `AutoMemoryState` (`Arc` + `RwLock`), `MemoryIndex`, `MemoryFrontmatter`, `AutoMemoryEntry`, `ResolvedAutoMemoryConfig`, `StalenessInfo` |

### Exec (3)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `shell` | Shell execution with CWD tracking, safety analysis, snapshotting | `ShellExecutor` (CWD tracking via markers, shell env snapshotting), `CommandResult` (exit_code, stdout, stderr, paths, new_cwd), `SafetyResult` (Safe/RequiresApproval/Denied), `BackgroundTaskRegistry`, `fork_for_subagent()` for isolated CWD |
| `sandbox` | Sandbox config (disabled by default, three modes) | `SandboxMode` (None/ReadOnly/Strict), `SandboxConfig`, `SandboxSettings` (enabled, auto_allow_bash_if_sandboxed), `PermissionChecker`, `SandboxPlatform` trait (Unix/Windows stubs) |
| `arg0` | Binary dispatcher and PATH setup | `arg0_dispatch_or_else(main_fn)`, argv[0] dispatch (apply_patch, linux-sandbox), `--cocode-run-as-apply-patch` flag, temp dir with symlinks for PATH, dotenv loading (filters `COCODE_*` vars) |

### MCP (2)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `mcp-types` | Auto-generated MCP message types (from spec 2025-06-18) | `InitializeRequest`/`Result`, `CallToolRequest`/`Result`, `ListToolsRequest`/`Result`, `ModelContextProtocolRequest` trait, tagged enums with `#[serde(tag = "method")]` |
| `rmcp-client` | MCP client: stdio + HTTP/SSE transport, OAuth | `RmcpClient` (state machine: Connecting→Ready), dual transport (TokioChildProcess for stdio, StreamableHttp for SSE), `OAuthPersistor` (auto-refresh), bidirectional type conversion with mcp-types |

### Standalone (2)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `retrieval` | Code search: BM25 + vector + AST via Facade pattern | `RetrievalFacade` (search, build_index, generate_repomap), `SearchRequest` (fluent: `.bm25()`, `.vector()`, `.hybrid()`, `.limit()`), `CodeChunk`, `IndexManager`, `RepoMapGenerator` (PageRank), presets: NONE/MINIMAL/STANDARD/FULL, LRU cache by workdir (max 16), SQLite + LanceDB storage |
| `lsp` | AI-friendly LSP client (query by name+kind, not position) | `LspServerManager` (multi-server lifecycle), `LspClient` (e.g. `client.definition(path, "Config", Some(SymbolKind::Struct))`), `SymbolKind`, `ResolvedSymbol`, `DiagnosticsStore` (300ms debounce), `ServerLifecycle` (max 5 restarts, exponential backoff), built-in: rust-analyzer, gopls, pyright, typescript-language-server |

### Vercel AI (8)

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `vercel-ai-provider` | Standalone types matching @ai-sdk/provider v4 spec | `LanguageModelV4` trait, `EmbeddingModelV4` trait, `ImageModelV4` trait, `ProviderV4` trait, `LanguageModelV4Prompt`, `UserContentPart`/`AssistantContentPart`/`ToolContentPart` (content enums), `AISdkError` |
| `vercel-ai-provider-utils` | Utilities for implementing AI SDK v4 providers | `ApiResponse`, `ResponseHandler`/`JsonResponseHandler`/`StreamResponseHandler`, `ErrorHandler`, `Fetch`/`FetchOptions`, `Schema`/`ValidationError`, `ToolMapping`, `DataUri` |
| `vercel-ai` | High-level SDK matching @ai-sdk/ai (generate_text, stream_text, embed) | `GenerateTextOptions`/`GenerateTextResult`, `StreamTextOptions`, `GenerateTextCallbacks`/`StreamTextCallbacks`, `GenerateObjectOptions`, `EmbedOptions`/`EmbedResult`, `OutputStrategy`, `LanguageModel` |
| `vercel-ai-openai` | OpenAI provider for Vercel AI SDK v4 | `OpenAIProvider`, `OpenAIProviderSettings`, `OpenAIChatLanguageModel`, `OpenAIResponsesLanguageModel`, `OpenAIEmbeddingModel`, `OpenAIImageModel`, `OpenAISpeechModel`, `OpenAITranscriptionModel` |
| `vercel-ai-openai-compatible` | Generic OpenAI-compatible provider (xAI, Groq, Together, etc.) | `OpenAICompatibleProvider`, `OpenAICompatibleProviderSettings`, `OpenAICompatibleChatLanguageModel`, `OpenAICompatibleEmbeddingModel`, `MetadataExtractor`, `StreamMetadataExtractor` |
| `vercel-ai-google` | Google Gemini provider for Vercel AI SDK v4 | `GoogleGenerativeAIProvider`, `GoogleGenerativeAIProviderSettings`, `GoogleGenerativeAILanguageModel`, `GoogleGenerativeAIEmbeddingModel`, `GoogleGenerativeAIImageModel`, `GoogleGenerativeAIVideoModel` |
| `vercel-ai-anthropic` | Anthropic Claude provider for Vercel AI SDK v4 | `AnthropicProvider`, `AnthropicProviderSettings`, `AnthropicMessagesLanguageModel`, `AnthropicConfig`, `CacheControlValidator` |
| `vercel-ai-bytedance` | ByteDance video provider (Seedance) via ModelArk API | `ByteDanceProvider`, `ByteDanceProviderSettings`, `ByteDanceVideoModel`, `ByteDanceVideoModelConfig`, `ByteDanceVideoSettings` |

### Utils (20)

| Crate | Purpose |
|-------|---------|
| `absolute-path` | Absolute path types with deserialization support |
| `apply-patch` | Unified diff/patch application with fuzzy matching |
| `async-utils` | Async runtime utilities and cancellation helpers |
| `cache` | LRU cache with Tokio mutex protection |
| `cargo-bin` | Cargo binary helpers for test harnesses |
| `common` | Shared cross-crate utility functions |
| `file-encoding` | File encoding and line-ending detection/preservation |
| `file-ignore` | .gitignore-aware file filtering |
| `file-search` | Fuzzy file search with nucleo and gitignore support |
| `file-watch` | Generic reusable file-watch infrastructure |
| `git` | Git operations wrapper |
| `image` | Image processing utilities |
| `json-to-toml` | JSON to TOML conversion |
| `keyring-store` | Secure credential storage using system keyring |
| `pty` | Pseudo-terminal handling |
| `readiness` | Readiness flag with token-based auth and async waiting |
| `shell-parser` | Shell command parsing and security analysis (24 analyzers: 16 Deny + 8 Ask) |
| `stdio-to-uds` | Bridge stdio streams to Unix domain sockets |
| `string` | String truncation and boundary utilities |
| `symbol-search` | Symbol search for code navigation |

## Key Design Patterns

| Pattern | Where | Details |
|---------|-------|---------|
| **Builder** | Most crates | `ExecutorBuilder`, `AgentLoop::new()`, `FacadeBuilder`, `SearchRequest` (fluent) |
| **Arc-heavy sharing** | core/, features/ | Registries, managers, trackers: `Arc<Mutex<T>>` or `Arc<RwLock<T>>` (e.g. `TeamStore`, `AutoMemoryState`) |
| **Event-driven** | loop, tui | `mpsc::Sender<LoopEvent>` for UI updates, `tokio::select!` multiplexing |
| **Cancellation** | All async | `CancellationToken` threaded through all layers |
| **Callback decoupling** | tools, subagent | `SpawnAgentFn` callback avoids tools→subagent circular dependency; `context` stores tool names as `Vec<String>` to avoid tools dep |
| **Meta messages** | system-reminder | `is_meta: true` hides reminders from UI while keeping them in model context |
| **Permission pipeline** | executor, tools | `Tool.check_permission()` → `PermissionRuleEvaluator` (sorted by `RuleSource` priority: Session > Command > Cli > Flag > Local > Project > Policy > User) → `ApprovalStore` |
| **Facade** | retrieval | Single `RetrievalFacade` entry point hides SearchService + IndexService + RecentFilesService |
| **Elm (TEA)** | tui | Model (`AppState`) + Message (`TuiEvent`) + Update (`handle_command`) + View (`render`) |
| **Middleware** | vercel-ai | `FnOnce` + `BoxFuture` callbacks for `do_generate`/`do_stream` delegation in language model middleware |

## Testing

### Test Assertions

- Use `pretty_assertions::assert_eq` for clearer diffs
- Prefer comparing entire objects over individual fields
  ```rust
  // Correct
  assert_eq!(actual, expected);
  // Avoid
  assert_eq!(actual.name, expected.name);
  assert_eq!(actual.value, expected.value);
  ```
- Avoid mutating process environment in tests; prefer passing flags or dependencies

### Test Organization

- **Separate test files** using `#[path]` attribute to keep source files focused:
  ```rust
  // At the end of implementation.rs
  #[cfg(test)]
  #[path = "implementation.test.rs"]
  mod tests;
  ```
  Tests go in the companion `implementation.test.rs` file in the same directory.
- Integration tests in `tests/` directory
- Use descriptive test names: `test_<function>_<scenario>_<expected>`

### Test & Lint Workflow

- Test the specific changed crate first: `just test-crate cocode-<name>`
- Only run full `just test` if changes affect shared crates (common/, core/, protocol/)
- Use `just fix -p cocode-<name>` for scoped clippy fixes; only run `just fix` without `-p` if you changed shared crates

### Snapshot Tests

This repo uses snapshot tests (via `insta`) to validate rendered output, especially in TUI.

- Any change that affects user-visible UI must include corresponding `insta` snapshot coverage
- Run tests to generate updated snapshots: `cargo test -p cocode-tui`
- Check pending: `cargo insta pending-snapshots -p cocode-tui`
- Review by reading `*.snap.new` files directly, or: `cargo insta show -p cocode-tui path/to/file.snap.new`
- Accept: `cargo insta accept -p cocode-tui`
- Install if missing: `cargo install cargo-insta`

## TUI Conventions

### Styling (ratatui)

- Use Stylize helpers: `"text".dim()`, `.bold()`, `.cyan()`, `.italic()`, `.underlined()` instead of manual `Style`
- Simple conversions: `"text".into()` for spans, `vec![...].into()` for lines
- Computed styles: `Span::styled` or `Span::from(text).set_style(style)` is OK when style is runtime-computed
- Avoid hardcoded `.white()` — prefer the default foreground (no color)
- Chain for readability: `url.cyan().underlined()`
- Use `Line::from(text)` or `Span::from(text)` only when the target type isn't obvious from context
- Avoid churn: don't refactor between equivalent forms without a clear readability gain
- Compactness: prefer the form that stays on one line after rustfmt

### Text Wrapping

- Use `textwrap::wrap` for plain strings
- For indentation, use `initial_indent` / `subsequent_indent` options rather than custom logic

## Async Conventions

### Tokio Runtime

- Use `tokio::task::spawn_blocking` for blocking operations
- Prefer `tokio::sync` primitives over `std::sync` in async contexts
- Add `Send + Sync` bounds to traits used with `Arc<dyn Trait>`

## Dependencies

### Adding Dependencies

- Prefer well-maintained crates with active development
- Check for security advisories before adding
- Use workspace dependencies when possible

### Common Dependencies

| Purpose | Crate |
|---------|-------|
| Async runtime | `tokio` |
| HTTP client | `reqwest` |
| JSON | `serde_json` |
| Error handling | `anyhow`, `snafu` |
| Logging | `tracing` |
| Testing | `pretty_assertions` |

## Error Handling

| Layer | Error Type |
|-------|------------|
| common/, core/ | `cocode-error` + snafu + snafu-virtstack (StatusCode `XX_YYY` classification, retryable flag) |
| features/ | snafu + `cocode-error` |
| utils/ | `anyhow::Result` |
| vercel-ai/ | `thiserror` (standalone, no cocode deps) |
| app/, exec/, mcp/, standalone | `anyhow::Result` |

StatusCode categories: General (00-05), Config (10), Provider (11), Resource (12). See [common/error/README.md](cocode-rs/common/error/README.md).

## Design Decisions

| Decision | Rationale |
|----------|-----------|
| **No Deprecated Code** | When refactoring or implementing features, remove obsolete code completely. Do not mark as deprecated or maintain backward compatibility - delete it outright to keep the codebase clean and avoid technical debt. |
| **No Inline Tests** | Never put `#[cfg(test)] mod tests { ... }` with test functions inline in source files. Always extract tests to a separate `<name>.test.rs` file and reference it with `#[cfg(test)] #[path = "<name>.test.rs"] mod tests;` at the end of the source file. |
| **No `unsafe` Code** | Never use `unsafe` blocks, `unsafe fn`, `unsafe impl`, or `unsafe trait` anywhere in the codebase. All code must be written in safe Rust. If a dependency requires `unsafe` at the boundary, wrap it in a safe abstraction within that dependency — never expose `unsafe` to cocode-rs crates. If you believe `unsafe` is truly unavoidable, stop and discuss the design with the team first. |
| **No Hardcoded Tool/Enum Names** | Never use string literals like `"Read"`, `"Task"`, `"shutdown_request"` for tool names or enum identifiers. Always use the canonical enum's `as_str()` method: `ToolName::Read.as_str()`, `MessageType::ShutdownRequest.as_str()`. For const arrays use `const X: &[&str] = &[ToolName::Xxx.as_str(), ...]`. When the canonical enum is in a crate you cannot depend on (e.g., cross-layer), define well-known constants in a shared module (see `generator::message_types`). This prevents silent breakage when enum variants are renamed. |
| **No Single-Use Helpers** | Do not create small helper methods that are referenced only once. Inline the logic at the call site. |
| **Python SDK Only** | `cocode-sdk/` currently targets Python only. Do not add TypeScript, Go, or other language SDKs at this stage. The codegen pipeline (`Rust → JSON Schema → datamodel-code-generator`) is designed for multi-language extension later, but the current priority is full Python SDK feature parity. |

## Specialized Documentation

| Component | Guide |
|-----------|-------|
| TUI | [app/tui/CLAUDE.md](cocode-rs/app/tui/CLAUDE.md) |
| Retrieval | [retrieval/CLAUDE.md](cocode-rs/retrieval/CLAUDE.md) |
| LSP | [lsp/CLAUDE.md](cocode-rs/lsp/CLAUDE.md) |
| Vercel AI SDK | [vercel-ai/ai/CLAUDE.md](cocode-rs/vercel-ai/ai/CLAUDE.md) |
| Vercel AI Provider | [vercel-ai/provider/CLAUDE.md](cocode-rs/vercel-ai/provider/CLAUDE.md) |
| Vercel AI Provider Utils | [vercel-ai/provider-utils/CLAUDE.md](cocode-rs/vercel-ai/provider-utils/CLAUDE.md) |
| File Ignore | [utils/file-ignore/CLAUDE.md](cocode-rs/utils/file-ignore/CLAUDE.md) |
| Error Codes | [common/error/README.md](cocode-rs/common/error/README.md) |
| User Docs | [docs/](docs/) (getting-started.md, config.md, sandbox.md) |
