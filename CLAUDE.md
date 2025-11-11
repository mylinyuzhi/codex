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

## Development Workflow

**Core workflow:**
1. `just fmt` → `cargo test -p <crate>` → `just fix -p <crate>` (⚠️ ask first)
2. `cargo test --all-features` (if core/protocol changed, ⚠️ ask first)
3. `just clippy` + `rg "unwrap\(\)" --type rust` + `rg "white\(\)" codex-rs/tui/`

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