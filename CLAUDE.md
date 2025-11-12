# Codex Development Guide

Fast, LLM-optimized guide for developing **codex-rs** - a Rust CLI with 42+ crates.

**Workspace:** `codex-rs/` | **Main entry:** `codex-rs/cli/` | **Core logic:** `codex-rs/core/`

---

## <critical_rules>

### Rule 1: Error Handling - CodexErr vs anyhow

**Decision Tree:**
```
Is this core business logic or public API?
├─ YES → Use CodexErr (codex-rs/core/src/error.rs)
│   └─ Pattern: use crate::error::Result as CodexResult
└─ NO → Is this MCP/adapters/tests/utilities?
    └─ YES → Use anyhow::Result
```

<error_contexts>
- **CodexErr files:** core/, cli/, exec/, app-server/, tui/
- **anyhow files:** mcp/, adapters/, config/edit.rs, all test files
- **Convert anyhow→CodexErr:** `.map_err(|e| CodexErr::Fatal(e.to_string()))?`
</error_contexts>

### Rule 2: Arc<dyn Trait> Requirements

**MUST have `Send + Sync`:**
```rust
pub trait ToolHandler: Send + Sync {  // ✅ Required by compiler
    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput>;
}
```
**Files using this pattern:** `tools/registry.rs`, `adapters/registry.rs`, `auth/storage.rs`

### Rule 3: Struct Field Addition

**Two different purposes - often need BOTH:**
- `#[serde(default)]` → Missing fields in JSON/TOML files
- `#[derive(Default)]` → `..Default::default()` in code

```rust
#[derive(Default, Debug, Deserialize)]
pub struct Config {
    pub name: String,
    #[serde(default)]  // For TOML: fills if missing
    pub new_field: Option<u16>,  // For code: Default trait fills
}
```

### Rule 4: Rust/Cargo Conventions

**Crate Naming:**
- All crates prefixed with `codex-` (e.g., `core/` folder → `codex-core` crate)

**Required Clippy Fixes:**
- Collapse nested if statements (`collapsible_if`)
- Inline format args: `format!("{var}")` not `format!("{}", var)`
- Method refs over closures: `items.map(String::from)` not `items.map(|x| String::from(x))`

**Never Use:**
- Unsigned integers (u32/u64) - always use i32/i64
- `.unwrap()` in non-test code

</critical_rules>

---


## Architecture Map

<crate_hierarchy>
```
codex-rs/
├─ core/           → Business logic, conversations, tools
├─ protocol/       → Message types (SQ/EQ pattern)
├─ cli/           → Binary entry point
├─ tui/           → Ratatui interface (codex-rs/tui/styles.md)
├─ exec/          → Headless automation
├─ tools/         → Tool registry & handlers
├─ codex-hooks/   → Hook system (event interception, Bash/native actions)
├─ mcp*/          → MCP integration (uses anyhow)
└─ utils/         → git, cache, pty, tokenizer
```
</crate_hierarchy>

## Module Detailed Functions

### **core/** - Core Business Logic ⭐
**Responsibility:** Heart of codex - conversation management, LLM interactions, tool execution
- `conversation.rs` → Main `CodexConversation` struct, manages message flow and state
- `error.rs` → `CodexErr` type (must use for core logic)
- Tool handling and invocation logic
- **When to add code:** New conversation behavior, tool interaction patterns, state management
- **Public API:** `CodexConversation`, `CodexErr`, conversation operations

### **protocol/** - Message Type Definitions
**Responsibility:** Defines SQ (Streaming Query) and EQ (Execution Query) patterns
- Input/output message structures
- Serialization/deserialization contracts
- **When to add code:** New message types needed across crates
- **Note:** Changes here may require updates in multiple crates

### **cli/** - Binary Entry Point
**Responsibility:** Command-line interface and application startup
- Argument parsing, configuration loading
- Conversation initialization
- **When to add code:** New CLI commands, startup logic
- **Uses:** CodexErr for error handling

### **tui/** - Ratatui Terminal UI
**Responsibility:** Interactive terminal interface with styling
- Widget rendering, input handling, state management
- View/theme system (see `codex-rs/tui/styles.md`)
- **When to add code:** New UI components, interaction patterns
- **Uses:** CodexErr for error handling
- **Rule:** Never use `.white()` directly, use theme system

### **exec/** - Headless Automation
**Responsibility:** Non-interactive execution mode for scripts/automation
- Handles prompt execution without user input
- State persistence and result formatting
- **When to add code:** New automation patterns, execution modes
- **Uses:** CodexErr for error handling

### **tools/** - Tool Registry & Handlers
**Responsibility:** Tool registration, execution, and integration
- `registry.rs` → Central tool registry (`Arc<dyn ToolHandler>`)
- Individual tool implementations
- **Key Pattern:** All handlers must be `Send + Sync`
- **When to add code:** New tools, tool middleware, caching layer
- **Architecture:** `Arc<RwLock<HashMap<ToolId, Arc<dyn ToolHandler>>>>`

### **mcp-*** - MCP Server Integration
**Responsibility:** Model Context Protocol implementations
- MCP server adapters for different LLM providers
- Tool translation (MCP → codex format)
- **Error Handling:** Uses `anyhow::Result` (NOT CodexErr)
- **When to add code:** New MCP providers, protocol features
- **Tool Naming:** `format!("{server}__{tool}")` (double underscore)

### **utils/** - Utilities & Helpers
**Responsibility:** Shared utilities across crates
- `git/` → Git operations (repo detection, status)
- `cache/` → Caching mechanisms
- `pty/` → Pseudo-terminal handling
- `tokenizer/` → Token counting for LLM usage
- **Error Handling:** Uses `anyhow::Result`
- **When to add code:** Shared functionality, infrastructure

### **core_test_support/** - Test Utilities
**Responsibility:** Testing helpers and mock utilities
- `responses.rs` → Mock response builders
- Integration test utilities
- **Error Handling:** Uses `anyhow::Result`
- **When to add code:** New test helpers, mock responses

### **codex-hooks/** - Hook System
**Responsibility:** Event-driven interception system for tool execution lifecycle
- `action/` → Bash and native action executors, global function registry
- `executor.rs` → Sequential/parallel execution with effect application
- `manager.rs` → Global singleton HookManager, registration and triggering
- `config.rs` → TOML configuration loading and manager building
- `context.rs` → Shared state management (approval, sandbox, mutations)
- `decision.rs` → HookDecision types and effect definitions
- **Error Handling:** Uses `anyhow::Result` (utility crate)
- **When to add code:** New action types, hook phases, effect types, protocol extensions

### **Custom Commands (Custom Prompts)**
**Responsibility:** User-defined slash commands via Markdown files in `~/.codex/prompts/`
- Enables `/prompts:name KEY=value` invocation with named (`$KEY`) and positional (`$1-$9`) parameters
- **Discovery:** `core/src/custom_prompts.rs` → Loads `.md` files, parses YAML frontmatter
- **Protocol:** `protocol/src/custom_prompts.rs` → `CustomPrompt` struct and validation
- **Expansion:** `tui/src/bottom_pane/prompt_args.rs` → Argument parsing and template substitution
- **Pattern:** Markdown with optional `description` and `argument_hint` frontmatter
- **When to add code:** New parameter types, validation logic, discovery enhancements

### **Subagent Support**
**Responsibility:** Nested Codex conversations that delegate approvals to parent sessions
- Enables hierarchical workflows with isolated conversation context
- **Built-in types:** `Review`, `Compact` with default configurations
- **Custom agents:** TOML configs in `~/.codex/agents/` (user) or `.codex/agents/` (project)
  - Priority: built-in < user < project
  - Schema: name, description, system_prompt, model, tools, max_turns, thinking_budget
- **Invocation:** `Op::CustomAgent { agent_name, prompt }` dispatches to CustomAgentTask
- **Registry:** `core/src/agent_registry.rs` → File discovery, validation, LazyLock caching
- **Protocol:** `protocol/src/agent_definition.rs` → AgentDefinition, AgentLoadStatus
- **Task:** `core/src/tasks/custom_agent.rs` → CustomAgentTask spawns subagent with config
- **HTTP:** `x-openai-subagent` header includes subagent type/name
- **When to add code:** New agent types, config fields, delegation patterns
- **Tests:** `core/tests/suite/codex_delegate.rs`, `core/tests/suite/agent_registry.rs`

### **Compact Strategies**
**Responsibility:** Pluggable conversation compaction strategies for flexible context management
- **Core abstraction:** `core/src/compact_strategy.rs` → `CompactStrategy` trait, registry with `LazyLock`
- **Built-in strategies:**
  - `simple` (default) → Handoff-focused prompt, preserves recent user messages (~20k tokens)
  - `file-recovery` → Kode-cli inspired, 8-section structured prompt + automatic file recovery
- **Strategy selection:** Via `compact_prompt` config field with `strategy:` prefix
  - Example: `compact_prompt = "strategy:file-recovery"` in `~/.codex/config.toml`
  - Defaults to `simple` if no prefix or invalid strategy name
- **File recovery mechanism:**
  - Parses `read_file` function calls from conversation history (stateless)
  - Filters: excludes `node_modules`, `.git`, `dist/`, `build/`, `.cache/`, `/tmp`
  - Limits: max 5 files, 10k tokens/file, 50k total tokens
  - Reads current file state (not historical tool output)
  - Appends to compacted history with line numbers
- **Architecture:**
  - `core/src/compact_strategies/` → Strategy implementations (`simple.rs`, `file_recovery.rs`)
  - `core/templates/compact/` → Prompt templates (`prompt.md`, `file_recovery.md`)
  - Strategy dispatch in `core/src/compact.rs` (~15 lines modified)
- **When to add code:** New compaction strategies, custom prompt templates, file filtering rules
- **Tests:** `core/src/compact_strategies/file_recovery.rs` (unit tests), `core/tests/suite/compact.rs` (integration)

### **Hook System**
**Responsibility:** Event-driven lifecycle interception system allowing external scripts and native functions to block, validate, or augment tool execution at various lifecycle points
- **Claude Code compatible:** Full protocol compatibility with 9 event types (PreToolUse, PostToolUse, UserPromptSubmit, Stop, SubagentStop, Notification, PreCompact, SessionStart, SessionEnd)
- **Dual action types:**
  - **Bash actions:** Execute external scripts with JSON I/O protocol, support exit code convention (0=continue, 2=block)
  - **Native actions:** Call registered Rust functions directly via global registry
- **Configuration:** TOML files in `~/.codex/hooks.toml` or `.codex/hooks.toml`
  - Structure: `[[PreToolUse]]` → `matcher` pattern → `[[PreToolUse.hooks]]` action list
  - Sequential vs parallel execution mode per hook definition
  - Priority-based ordering: hooks execute in (Phase, Priority) tuple order
- **Architecture:**
  - **Protocol:** `protocol/src/hooks.rs` → Claude Code protocol types (HookEventContext, HookOutput, HookDecision)
  - **Core crate:** `codex-hooks/` → Complete hook system implementation
    - `action/` → Bash and native action executors, global registry
    - `executor.rs` → Sequential/parallel execution coordinator
    - `manager.rs` → Global singleton with trigger API, disabled by default
    - `config.rs` → TOML loading and HookManager builder
  - **Core integration:** `core/src/hooks/integration.rs` → Convenience wrappers for triggering hooks
  - **Error handling:** `core/src/error.rs` → `CodexErr::HookBlocked` variant
- **State sharing:** Hooks can modify shared state (approval, sandbox, command mutations, metadata) via effect system
- **Examples:** `.codex/hooks/` → validate-shell.sh, audit-log.sh, session lifecycle hooks
- **When to add code:** New hook events → `protocol/src/hooks.rs`, new action types → `codex-hooks/src/action/`, integration points → `core/src/hooks/`
- **Tests:** `codex-hooks/src/*` (24 unit tests), `codex-hooks/tests/integration_tests.rs` (9 integration tests)

---

**Key Files:**
- Error definitions: `core/src/error.rs`
- Tool registry: `tools/registry.rs`
- Conversation logic: `core/src/conversation.rs`
- Test utilities: `core_test_support/src/responses.rs`

---

## Function Call Architecture

**Multiple tool calls:** ✅ Supported in single LLM response (via `parallel_tool_calls` flag)

**Execution model:** Per-tool intelligent parallelization
- `supports_parallel_tool_calls = true` → Concurrent execution (read lock)
- `supports_parallel_tool_calls = false` → Serial execution (write lock)
- Mixed calls → Safe serial fallback

**Code locations:** `tools/parallel.rs` (RwLock control) | `tools/router.rs` (per-tool config) | `protocol/src/models.rs` (ResponseItem::FunctionCall)

---

## Sandbox & Approval System

**Responsibility:** Two independent systems providing user permission control (approval) and OS-level enforcement (sandbox)

- **Approval:** User consent gate via policies (`UnlessTrusted` | `OnFailure` | `OnRequest` | `Never`)
  - **Core:** `core/src/tools/orchestrator.rs` → Approval orchestration, `core/src/tools/sandboxing.rs` → `Approvable` trait, approval cache
  - **Protocol:** `protocol/src/approvals.rs` → Event definitions, `protocol/src/protocol.rs` → `AskForApproval` enum
  - **Cache:** Session-scoped HashMap, only `ApprovedForSession` cached
- **Sandbox:** OS-level restrictions via policies (`ReadOnly` | `WorkspaceWrite` | `DangerFullAccess`)
  - **Core:** `core/src/sandboxing/mod.rs` → `SandboxManager`, `core/src/seatbelt.rs` (macOS), `core/src/landlock.rs` (Linux)
  - **Protocol:** `protocol/src/protocol.rs` → `SandboxPolicy`, `WritableRoot` structs
  - **Built-in:** `.git/` always read-only, network disabled by default in WorkspaceWrite
- **Integration:** `core/src/tools/orchestrator.rs` coordinates both systems (approval check → sandbox selection → transform → execute → retry on denial)
- **Subagent delegation:** `core/src/codex_delegate.rs` → `forward_events()` routes approval requests to parent session
- **When to add code:** New approval/sandbox policies → `protocol/src/protocol.rs`, new platform support → `core/src/sandboxing/`, new tool needs approval → implement `Approvable` trait
- **Tool pattern:** Must declare `sandbox_preference()` and optionally implement `Approvable`, always invoke via `ToolOrchestrator::run()`

**Detailed docs:** `docs/architecture/sandbox-approval-integration.md`

---

## Common Mistakes & Solutions

<avoid_these>

| Mistake | Solution | Check Command |
|---------|----------|---------------|
| Wrong Result type | Use `CodexResult` alias | `rg "use.*Result" <file>` |
| Missing Send+Sync | Add bounds to trait | Compiler will error |
| No #[serde(default)] | Add for optional fields | Test TOML loading |
| unsigned integers | Always use i32/i64 | `rg "u32\|u64" --type rust` |
| format!("{}", var) | Use format!("{var}") | `just fmt` auto-fixes |
| Only `cargo check -p` | Always run `cargo build` before commit | `cargo build` |
| Adding tool without full build | Run `cargo build` after tool changes | Checks EventMsg matches |
| `.white()` in TUI | Never use - use default foreground | `rg "\.white\(\)" codex-rs/tui/` |
| Manual Style in TUI | Use `.dim()`, `.bold()`, `.cyan()` helpers | See `tui/styles.md` |
| Plain string wrapping | Use `textwrap::wrap` for strings | Check imports |
| Line wrapping in TUI | Use `word_wrap_lines` from `tui/src/wrapping.rs` | For ratatui Lines |
| Nested if statements | Collapse per clippy::collapsible_if | `just fix -p <crate>` |
| Closure over method ref | Use `items.map(String::from)` | `just fix -p <crate>` |
| Field-by-field comparison | Compare entire objects in tests | In test assertions |

</avoid_these>

---

## Testing Patterns

**Test Conventions:**
- Use `pretty_assertions::assert_eq` for clearer diffs (import in test module)
- Prefer comparing entire objects over field-by-field assertions
- Prefer `wait_for_event` over `wait_for_event_with_timeout`

**ResponseMock Helpers:**
```rust
let request = mock.single_request();  // For one POST
let requests = mock.requests();       // For multiple POSTs

// Available helpers on ResponsesRequest:
request.body_json()                   // Full JSON body
request.input()                       // Input field
request.function_call_output(call_id) // Function call output
request.custom_tool_call_output(call_id) // Custom tool output
request.call_output(call_id)          // Generic call output
request.header("name")                // HTTP header
request.path()                        // Request path
request.query_param("key")            // Query parameter
```

**Integration Test Template:**
```rust
// In tests/ directory - uses anyhow
use core_test_support::responses;

let mock = responses::mount_sse_once(&server, responses::sse(vec![
    responses::ev_response_created("resp-1"),
    responses::ev_function_call(call_id, "tool", &args),
])).await;

// Assert outbound requests
let request = mock.single_request();
assert_eq!(request.function_call_output(call_id), expected);
```

**Snapshot Testing (TUI):**
```bash
# Run tests and generate snapshots
cargo test -p codex-tui

# Check pending changes
cargo insta pending-snapshots -p codex-tui

# Review specific snapshot (read .snap.new files directly)
cargo insta show -p codex-tui path/to/file.snap.new

# Accept all (only if you intend to accept ALL)
cargo insta accept -p codex-tui
```
---

## Adding New Tools

**Implementation Flow:**
1. `protocol/src/config_types.rs` → Config structs (#[derive(Default)] + #[serde(default)])
2. `core/src/tools/my_tool.rs` → Handler (impl ToolHandler, must be Send + Sync)
3. `core/src/tools/spec.rs` → Register in build_specs() (push_spec + register_handler)
4. `core/src/config/mod.rs` → Add config field to Config struct
5. Tests → Use anyhow, validate data exists before using

**Optional: Add EventMsg for User Notifications** 📢

If tool needs to notify user (like web_search, web_fetch):
1. `protocol/src/protocol.rs` → Add event variant + struct:
   ```rust
   // In EventMsg enum
   MyToolCall(MyToolCallEvent),

   // Event struct definition
   #[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, TS)]
   pub struct MyToolCallEvent {
       pub call_id: String,
       pub info: String,  // Tool-specific info to display
   }
   ```
2. Update ALL EventMsg matches (3 files):
   - `mcp-server/src/codex_tool_runner.rs`
   - `exec/src/event_processor_with_human_output.rs`
   - `tui/src/chatwidget.rs`
   - Add `| EventMsg::MyToolCall(_)` to wildcard match arms
3. In handler, send event before execution:
   ```rust
   session.send_event(
       turn.as_ref(),
       EventMsg::MyToolCall(MyToolCallEvent { call_id, info })
   ).await;
   ```
4. **Run `cargo build`** to catch all missing match arms

**Critical: Batch Error Discovery** ⚠️

```bash
# Step 4 trigger: Adding field to Config breaks ALL test initializations
cargo check 2>&1 | tee errors.txt        # Discover all at once
rg "Config \{" core/src --type rust      # Find every initialization
# Fix all simultaneously, not one-by-one (saves 70% iterations)
```

**Common Pitfalls:**

| Mistake | Fix Command | Why It Matters |
|---------|-------------|----------------|
| Using invalid test data (model slugs, IDs) | `rg 'starts_with\("' core/src/model_family.rs` | Prevents .unwrap() panics |
| Incremental error fixing | `cargo check 2>&1 \| grep "error\[E"` | Batch discovery saves time |
| Implicit test configs | Explicitly construct test configs | Default may not match test expectations |

**Verification:**
```bash
just fmt                           # Format code
cargo check                        # Quick check
cargo build                        # ⭐ REQUIRED: Verify all 42+ crates
cargo test -p codex-core --lib     # Unit tests
just clippy                        # Lint check
```

---

## Development Workflow

**Core workflow:**
1. `just fmt` → `cargo check` → `cargo test -p <crate>` → `just fix -p <crate>` (⚠️ ask first)
2. **CRITICAL for protocol/core changes:** `cargo build` (verifies all 42+ crates)
3. `cargo test --all-features` (if core/protocol changed, ⚠️ ask first)
4. `just clippy` + `rg "unwrap\(\)" --type rust` + `rg "white\(\)" codex-rs/tui/`

**Quality Checks Levels:**
- **Level 1 (iteration):** `cargo check -p <crate>` - fast feedback during development
- **Level 2 (pre-commit):** `cargo build` - **REQUIRED before any commit**
- **Level 3 (core changes):** `cargo test --all-features` - comprehensive validation

**When `cargo build` is REQUIRED:**
- After modifying `protocol/src/protocol.rs` (especially `EventMsg` enum)
- After adding new tools in `core/src/tools/` (may affect EventMsg matches downstream)
- After changing public APIs in core/protocol packages
- **Always as final check before committing**

**Why:**
- `cargo check -p` only validates one crate, misses downstream issues (exec, tui, mcp-server)
- Adding tools can break EventMsg pattern matches in other packages
- Codex-rs has 42+ crates with complex dependencies

**Other commands:** `just codex` (run), `just tui` (TUI), `just exec "prompt"` (headless), `cargo insta review` (snapshots)

---

## Key Patterns

<implementation_patterns>

**Tool Registration:**
```rust
// codex-rs/tools/registry.rs
registry.register("tool_name", Arc::new(handler));
```

**MCP Tool Names:**
```rust
format!("{server}__{tool}")  // Double underscore delimiter
```

**Conversation Flow:**
```
User → Op → SQ → CodexConversation → Tool → EQ → Response
```

**State Management:**
```rust
Arc<RwLock<HashMap<ConversationId, Arc<CodexConversation>>>>
```

</implementation_patterns>

---

## Environment Notes

- **Sandbox vars** (CODEX_SANDBOX_*): Read-only, NEVER modify
  - `CODEX_SANDBOX_NETWORK_DISABLED=1` set when using shell tool
  - `CODEX_SANDBOX=seatbelt` set when spawning Seatbelt processes
  - Tests check these to early-exit when sandboxed
- **Required tools**: Install `just`, `rg`, `cargo-insta` before running commands
- **Rust version**: See `rust-toolchain.toml`
- **Dependencies**: Prefer stdlib over external crates (LazyLock > once_cell)

---

## Git Workflow

**ONLY commit when user explicitly requests.** See `.claude/docs/git-workflow.md`