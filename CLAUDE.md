# CLAUDE.md

Multi-provider LLM SDK and CLI. All development in `coco-rs/`.

> `cocode-rs/` and `codex-rs/` in this repo are reference implementations; active development is in `coco-rs/`.
>
> **Each crate has its own `CLAUDE.md`** with key types, invariants, and design notes — read it when working in that crate. This root file covers conventions and high-level structure only.

## Commands

Run from `coco-rs/` directory:

```bash
just fmt          # After Rust changes (auto-approve)
just pre-commit   # REQUIRED before commit (fmt + check + clippy + test via nextest)
just test         # If changed vercel-ai or core crates
just test-crate <name>   # Scope to a single crate
just check        # Type-check all crates
just clippy       # Run clippy on all crates
just fix -p <name>       # Auto-fix clippy warnings for a single crate
just help         # All commands
```

**Path conventions** (avoids the `coco-rs/coco-rs/...` mistake):
- `just` / `cargo` commands: run from `coco-rs/`; paths are workspace-relative (`app/query/src/...`).
- `git` commands: also fine to run from inside `coco-rs/` — paths stay workspace-relative (`app/query/src/...`), NOT prefixed with `coco-rs/`.
- The `coco-rs/` prefix only appears when viewing output from the repo root (e.g. session-start `gitStatus`, or `git` run from the repo root). Don't copy those paths verbatim into commands run from `coco-rs/`.

## Code Style

### Format and Lint

- When using `format!` and you can inline variables into `{}`, always do that: `format!("{name} is {age}")`, never `format!("{} is {}", name, age)`
- Collapse `if` per [collapsible_if](https://rust-lang.github.io/rust-clippy/master/index.html#collapsible_if)
- Prefer method references over closures per [redundant_closure_for_method_calls](https://rust-lang.github.io/rust-clippy/master/index.html#redundant_closure_for_method_calls)
- Make `match` exhaustive; avoid wildcard arms

### Integer Types

- Use `i32`/`i64` instead of `u32`/`u64` unless bit-pattern required — avoids overflow bugs, matches common APIs

### Error Handling

- Never `.unwrap()` in non-test code. Use `?` or `.expect("reason")`
- Avoid mixing `Result` and `Option` without clear conversion
- Per-layer error types: see [Error Handling](#error-handling) below

### Serde Conventions

- `#[serde(default)]` for optional config fields
- `#[derive(Default)]` for structs used with `..Default::default()`
- `#[serde(rename_all = "snake_case")]` for enums

### Parameter Design

- Avoid bool/ambiguous `Option` params that produce opaque callsites like `foo(false)` or `bar(None)`
- Prefer enums, named methods, newtypes

### Argument Comments

When a positional literal is unavoidable, add `/*param_name*/` matching the callee signature:

```rust
connect(/*timeout*/ None, /*retries*/ 3, /*verbose*/ false)
```

Add for `None` / booleans / numeric literals. Skip for string/char literals unless the comment adds real clarity.

### Module Size

- Target Rust modules under 500 LoC (excluding tests)
- Files > ~1400 LoC: create a new module instead of extending

### Comments

- Concise; describe purpose, not implementation
- Field docs: 1-2 lines, no example configs
- Code comments: only when intent is non-obvious

## Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│  App:         cli, tui, session, query, state                        │
│  Root:        commands, skills, hooks, tasks, memory, plugins,       │
│               keybindings                                             │
│  Core:        tool → tools, permissions, messages, context,         │
│               system-reminder                                         │
│  Services:    inference, compact, mcp, rmcp-client, mcp-types, lsp   │
│  Exec:        shell, sandbox, process-hardening, exec-server,        │
│               apply-patch                                             │
│  Standalone:  bridge, retrieval                                      │
│  Vercel AI:   ai → openai, openai-compatible, google, anthropic,     │
│               bytedance (on provider + provider-utils)                │
│  Common:      types, config, error, otel, stack-trace-macro          │
│  Utils:       see Utils table below                                  │
└──────────────────────────────────────────────────────────────────────┘
```

Lower layers depend on nothing above them. See each crate's `CLAUDE.md` for its key types and invariants.

**Before implementing any basic capability, scan the Utils table below** — if a crate already provides it (path handling, caching, git, encoding, ignore rules, fuzzy search, frontmatter, secret redaction, …), use it rather than rolling your own.

## Key Data Flows

### Agent Turn Lifecycle (driven by `app/query::QueryEngine`)

```
User input → ConversationContext → MessageHistory + attachments
  → ApiClient.query → vercel-ai stream
  → StreamAccumulator → StreamingToolExecutor (safe concurrent / unsafe queued)
  → Hook orchestration (Pre/PostToolUse)
  → Tool results → MessageHistory → emit CoreEvent (Protocol/Stream/Tui)
  → Check ContinueReason → maybe compact (micro/full/reactive)
  → Drain CommandQueue → loop if tool calls remain
```

### Configuration Resolution

```
~/.coco/{provider,model,config}.json + env + CLI overrides
  → Settings → EnvOnlyConfig → RuntimeOverrides → ResolvedConfig
  → GlobalConfig (hot-reload via SettingsWatcher)
  → ModelRoles + ModelAlias + FastModeState
  → BootstrapConfig → app/query → services/inference
```

### Provider Call Chain

```
QueryEngine → ApiClient [services/inference]
  → Arc<dyn LanguageModelV4> → vercel-ai (generate_text / stream_text)
  → provider impl [vercel-ai/{openai,anthropic,google,bytedance,openai-compatible}]
  → HTTP → typed stream → CacheBreakDetector + UsageAccumulator → QueryResult
```

For shell execution, MCP integration, background tasks: see the respective crate's CLAUDE.md (`exec/shell`, `services/mcp`, `tasks`).

## Crate Guide

One-line purposes. For key types and details, open each crate's own `CLAUDE.md`.

### Common

| Crate | Purpose |
|-------|---------|
| `types` | Foundation types (zero internal deps); re-exports vercel-ai aliases; wire-tagged unions |
| `config` | Layered config: JSON + env + runtime overrides + hot reload |
| `error` | Unified errors with `StatusCode` classification (snafu + virtstack) |
| `otel` | OpenTelemetry tracing and metrics |
| `stack-trace-macro` | `#[stack_trace_debug]` proc macro for snafu enums |

### Vercel AI

| Crate | Purpose |
|-------|---------|
| `vercel-ai-provider` | Standalone types matching `@ai-sdk/provider` v4 (no coco deps) |
| `vercel-ai-provider-utils` | Utilities for AI SDK v4 providers (Fetch, ResponseHandler, Schema) |
| `vercel-ai` | High-level SDK matching `@ai-sdk/ai` (generate_text, stream_text, …) |
| `vercel-ai-openai` | OpenAI provider (Chat + Responses + Embeddings + Image) |
| `vercel-ai-openai-compatible` | Generic OpenAI-compatible provider (xAI, Groq, Together) |
| `vercel-ai-google` | Google Gemini provider |
| `vercel-ai-anthropic` | Anthropic Claude provider |
| `vercel-ai-bytedance` | ByteDance Seedance video provider |

### Services

| Crate | Purpose |
|-------|---------|
| `inference` | Thin multi-provider wrapper: generic retry, usage aggregation, tool-schema, cache-break. Auth/caching/betas live in provider crates |
| `compact` | Context compaction: full / micro / reactive / session-memory + auto-trigger |
| `mcp` | MCP server lifecycle, OAuth (incl. xaa IDP), elicitation, channel permissions |
| `mcp-types` | Auto-generated MCP message types |
| `rmcp-client` | MCP client: stdio + HTTP/SSE transport, OAuth persistence |
| `lsp` | AI-friendly LSP (query by name+kind, not position); rust-analyzer/gopls/pyright/tsserver |

### Core

| Crate | Purpose |
|-------|---------|
| `tool` | `Tool` trait, streaming executor, registry, callback handles (interface layer) |
| `tools` | Built-in tool impls (File I/O, Web, Agent, Task, Plan, Shell, MCP mgmt, Scheduling) |
| `permissions` | Evaluator + 2-stage auto-mode/yolo XML-LLM classifier + bypass killswitch |
| `messages` | MessageHistory, normalization/filtering/predicates, cost tracking |
| `context` | System context assembly, CLAUDE.md discovery, attachments, plan-mode reminders |
| `system-reminder` | Dynamic `<system-reminder>` injection: trait-based generators + parallel orchestrator + throttle |

### Exec

| Crate | Purpose |
|-------|---------|
| `shell` | Shell execution with security analysis, destructive warnings, sandbox decisions |
| `sandbox` | Three modes: None/ReadOnly/Strict (disabled by default) |
| `process-hardening` | OS-level security (macOS PT_DENY_ATTACH, Linux prctl) |
| `exec-server` | Minimal `ExecutorFileSystem` trait ported from codex-rs |
| `apply-patch` | Unified diff/patch with fuzzy matching (ported from codex-rs) |

### Root Modules

| Crate | Purpose |
|-------|---------|
| `commands` | Slash command registry + built-in command impls (v1/v2/v3) |
| `skills` | Skill markdown workflows (bundled / project / user / plugin) |
| `hooks` | Pre/post event interception with scoped priority, async registry, SSRF guard |
| `tasks` | Three kinds: `running` (bg tasks), `task_list` (durable plan items), `todos` (per-agent) |
| `memory` | Persistent cross-session: CLAUDE.md mgmt, auto-extraction, session memory, KAIROS auto-dream, team sync |
| `plugins` | Plugin system via `PLUGIN.toml` (contributions, marketplace, hot-reload) |
| `keybindings` | Shortcuts with context-based resolution and chord support |

### App

| Crate | Purpose |
|-------|---------|
| `cli` | CLI entry (clap), transports (SSE / WS / NDJSON), server / daemon / SDK modes; binary `coco` |
| `tui` | Terminal UI with Elm architecture (TEA) + ratatui + rust-i18n |
| `session` | Session persistence, title generation, transcript recovery |
| `query` | Multi-turn agent loop driver (`QueryEngine`) + budget + command queue |
| `state` | Central `AppState` tree + swarm orchestration modules |

### Standalone

| Crate | Purpose |
|-------|---------|
| `bridge` | IDE bridge (VS Code/JetBrains), REPL bridge, JWT auth, trusted-device store |
| `retrieval` | BM25 + vector + AST + RepoMap (PageRank) via Facade; isolated `RetrievalEvent` stream |

### Utils

Reusable primitives. **Check here first** before implementing any basic utility.

| Crate | Purpose |
|-------|---------|
| `absolute-path` | Absolute path types with deserialization support |
| `async-utils` | Async runtime utilities and cancellation helpers |
| `cache` | LRU cache with Tokio mutex protection |
| `cargo-bin` | Cargo binary helpers for test harnesses |
| `common` | Shared cross-crate utility functions |
| `cursor` | Text cursor with kill ring (Ctrl+K/Y), word boundaries, UTF-8 safe |
| `file-encoding` | File encoding and line-ending detection/preservation |
| `file-ignore` | .gitignore-aware file filtering (unified ignore service) |
| `file-search` | Fuzzy file search with nucleo and gitignore support |
| `file-watch` | Generic reusable file-watch infrastructure |
| `frontmatter` | YAML frontmatter parser for skills, commands, agents, memory files |
| `git` | Git operations wrapper |
| `image` | Image processing utilities |
| `json-to-toml` | JSON to TOML conversion |
| `keyring-store` | Secure credential storage using system keyring |
| `pty` | Pseudo-terminal handling |
| `readiness` | Readiness flag with token-based auth and async waiting |
| `rustls-provider` | TLS provider init via rustls crypto ring |
| `secret-redact` | Secret redaction (OpenAI, Anthropic, GitHub, Slack, AWS, bearer tokens) |
| `shell-parser` | Shell command parsing and security analysis |
| `sleep-inhibitor` | Cross-platform sleep prevention (macOS/Linux/Windows) |
| `stdio-to-uds` | Bridge stdio streams to Unix domain sockets |
| `stream-parser` | Stream parsing (text, citation, inline hidden tag, proposed plan, UTF-8) |
| `string` | String truncation and boundary utilities |
| `symbol-search` | Symbol search for code navigation |
| `test-harness` | Test harness utilities for integration tests |

## Key Design Patterns

| Pattern | Where | Details |
|---------|-------|---------|
| **Builder** | Most crates | `QueryEngine::new()`, `SearchRequest` (fluent), `RetrievalFacade` |
| **Arc-heavy sharing** | core/, root, app/ | `Arc<Mutex<T>>` / `Arc<RwLock<T>>` for registries/managers |
| **Event-driven** | query, tui, tasks | `mpsc::Sender<CoreEvent>` sinks; `with_event_sink()` opt-in emitters |
| **3-layer event dispatch** | types, query | `CoreEvent::Protocol`/`Stream`/`Tui` — emit once, consumers pick layer |
| **Cancellation** | All async | `CancellationToken` threaded through all layers |
| **Registry** | tool, commands, skills, plugins, mcp | `ToolRegistry`, `CommandRegistry`, `SkillManager`, `PluginLoader`, `DiscoveryCache` |
| **State Machine** | query, permissions, mcp | `ContinueReason`, `AutoModeState`, `RmcpClient` |
| **Callback decoupling** | core/tool | `AgentHandle`, `HookHandle`, `MailboxHandle`, `McpHandle`, `ToolPermissionBridge` — avoid tool→subsystem cycles; `NoOp*` test doubles |
| **Permission pipeline** | permissions, tool | `check_permission()` → auto-mode/yolo XML classifier → `DenialTracker` + killswitch |
| **Facade** | retrieval | Single `RetrievalFacade` hides search + index + repomap + reranker |
| **Elm (TEA)** | tui | Model (`AppState`) + Message (`TuiEvent`) + Update + View |
| **Middleware** | vercel-ai | `FnOnce` + `BoxFuture` callbacks for `do_generate` / `do_stream` |
| **Typed extension slots** | vercel-ai providers | `ProviderOptions` / `ProviderMetadata` = `serde_json::Value` on purpose |
| **Isolated event stream** | retrieval | `RetrievalEvent` intentionally not bridged into `CoreEvent` |

## Error Handling

| Layer | Error Type |
|-------|------------|
| common/, core/, services/ | `coco-error` + snafu + snafu-virtstack (StatusCode `XX_YYY`, retryable flag) |
| root modules | snafu + `coco-error` |
| utils/ | `anyhow::Result` |
| vercel-ai/ | `thiserror` (standalone, no coco deps) |
| app/, exec/, standalone | `anyhow::Result` (retrieval uses `RetrievalErr`; apply-patch uses `thiserror`) |

StatusCode categories: General (00-05), Config (10), Provider (11), Resource (12). See [common/error/README.md](coco-rs/common/error/README.md).

## Testing

### Assertions

- Use `pretty_assertions::assert_eq` for clearer diffs
- Compare entire objects over individual fields
- Avoid mutating process env; pass flags/dependencies

### Organization — MANDATORY

Never inline `#[cfg(test)] mod tests { ... }`. Always companion file:

```rust
#[cfg(test)]
#[path = "implementation.test.rs"]
mod tests;
```

Tests go in `implementation.test.rs` alongside the source. Integration tests in `tests/`. Name descriptively: `test_<function>_<scenario>_<expected>`.

### Workflow

- Changed one crate: `just test-crate coco-<name>`
- Changed shared (common/, core/, services/): `just test`
- Clippy fix: `just fix -p coco-<name>`

### Snapshot Tests (insta)

- UI changes require `insta` snapshot coverage
- Generate: `cargo test -p coco-tui`; Pending: `cargo insta pending-snapshots -p coco-tui`
- Review `*.snap.new` directly or `cargo insta show`; Accept: `cargo insta accept -p coco-tui`

## TUI Conventions (ratatui)

- Stylize helpers: `"text".dim().bold().cyan()` — avoid manual `Style`
- Simple conversions: `"text".into()`, `vec![...].into()`
- Runtime-computed style: `Span::styled` or `Span::from(t).set_style(s)` is OK
- Avoid `.white()`; prefer default foreground
- Don't refactor between equivalent forms without a readability gain
- Prefer forms that stay on one line after rustfmt
- Text wrapping: `textwrap::wrap` with `initial_indent`/`subsequent_indent` — don't roll your own

## Async Conventions

- `tokio::task::spawn_blocking` for blocking ops
- Prefer `tokio::sync` primitives in async contexts
- `Send + Sync` bounds on traits used with `Arc<dyn Trait>`

## Dependencies

| Purpose | Crate |
|---------|-------|
| Async runtime | `tokio` |
| HTTP | `reqwest` |
| JSON | `serde_json` |
| Errors | `anyhow`, `snafu`, `thiserror` |
| Logging | `tracing` |
| Testing | `pretty_assertions`, `insta`, `wiremock` |
| TUI | `ratatui`, `crossterm` |
| MCP | `rmcp` |

Prefer well-maintained crates; check security advisories; use workspace deps.

## Design Decisions

### Code Hygiene

| Rule | Note |
|------|------|
| No deprecated code | Delete outright. No `#[deprecated]`, no backward-compat shims |
| No inline tests | Use `#[path = "<name>.test.rs"]` always |
| No `unsafe` | All safe Rust. Wrap unsafe deps in own crate. Truly unavoidable? Discuss first |
| No single-use helpers | Inline at the call site |

### Type Safety

**No hardcoded strings for closed sets** (tool names, event types, config keys, protocol discriminators). Preference order:

1. **Enum + `.as_str()`** — e.g. `CommandBase::Read.as_str()`, `HookEventType::PreToolUse.as_str()`
2. **Module constants** (`pub const X: &str = "..."`) when the canonical enum lives in an inaccessible crate
3. **Typed struct** instead of `serde_json::Value` map

Raw strings only for unconstrained input (user text, opaque external IDs, third-party wire formats).

**Typed structs over `serde_json::Value`** when the payload is both produced *and* consumed inside coco-rs. Use `Option<T>` + `#[serde(default, skip_serializing_if = "Option::is_none")]` for optional fields, `#[serde(tag = "type")]` for variants.

*Exception:* `vercel-ai-*` provider-extension slots (`ProviderOptions`, `ProviderMetadata`, raw provider responses, model-specific blobs) keep `Value` — deliberate pass-through. Unpack to typed structs at the coco-rs boundary; never let `Value` leak inward.

### Multi-Provider Boundaries

- **Provider concerns stay in provider crates.** OAuth, API-key helpers, cloud credentials (Bedrock/Vertex/Foundry), prompt-cache breakpoint detection, beta headers, 529-capacity retry, rate-limit messaging, Claude.ai/Anthropic policy limits live in `vercel-ai-<provider>` — **not** `services/inference`. When porting TS, skip `services/api/`, `services/oauth/`, `services/policyLimits/`, `services/claudeAiLimits*`, `services/rateLimitMessages*`, `utils/auth.ts`, `utils/betas.ts`. `services/inference` owns only generic concerns.

- **Models are `(provider, api, model_id)`, never a bare string.** Always go through `coco_config::ModelRoles::get(ModelRole::X)`. `ModelRole` variants (`Main`, `Fast`, `Compact`, `Plan`, `Explore`, `Review`, `HookAgent`, `Memory`, …) are the only way to address "which model runs this". Never add `title_model: String`; expose a `bool` flag and route via the appropriate role. Add a new `ModelRole` variant rather than a raw string.

- **Compaction — three generic strategies only:** micro-compact (clear old tool results), full LLM summarization, reactive (on `prompt_too_long`). Do **not** port TS `HISTORY_SNIP` or `CONTEXT_COLLAPSE` — cache-aware optimizations belong in that `vercel-ai-*` crate.

- **Plan Mode — skip Ultraplan only.** Port core lifecycle, Pewter-ledger (Phase-4 variants `null`/`trim`/`cut`/`cap`), Interview phase — gate on `settings.json` (`plan_mode.phase4_variant`, `plan_mode.workflow`), not GrowthBook or `USER_TYPE=ant`. Skip every `feature('ULTRAPLAN')` path (needs CCR backend coco-rs doesn't ship).

### Event System

- **Single `CoreEvent` enum, three dispatch layers:** `Protocol` (SDK NDJSON), `Stream` (agent content), `Tui` (terminal). Emit once; consumers pick a layer. `QueryEngine::emit_*` is the reference emitter.
- **Opt-in lifecycle emitters.** Background subsystems (`TaskManager`, future retrieval) expose `with_event_sink(mpsc::Sender<CoreEvent>)` — zero overhead when not subscribed.
- **Isolated event streams stay isolated.** `RetrievalEvent` and `vercel-ai` callbacks (`OnStartEvent` etc.) are **not** bridged into `CoreEvent`. Need cross-subsystem progress? Add a single aggregate variant through an opt-in sink — don't bridge the full taxonomy.

## Specialized Documentation

Every crate in `coco-rs/` has its own `CLAUDE.md` (path = `coco-rs/<layer>/<crate>/CLAUDE.md`).

- **Common**: types, config, error ([codes](coco-rs/common/error/README.md)), otel, stack-trace-macro
- **Vercel AI**: ai, provider, provider-utils, openai, openai-compatible, google, anthropic, bytedance
- **Services**: inference, compact, mcp, lsp
- **Core**: tool, tools, permissions, messages, context, system-reminder
- **Exec**: shell, sandbox, process-hardening, exec-server, apply-patch
- **Root**: commands, skills, hooks, tasks, memory, plugins, keybindings
- **App**: cli, tui, query, state, session
- **Standalone**: bridge, retrieval
- **Utils**: each of the 26 utils/ crates has one
- **User docs**: [docs/](docs/) — getting-started.md, config.md, sandbox.md
