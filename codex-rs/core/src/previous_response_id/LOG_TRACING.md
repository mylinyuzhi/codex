# Incremental Input Log Tracing Guide

This document describes the debug logging added to track the incremental message building process using `previous_response_id`.

## Overview

When `RUST_LOG=debug` is set, the implementation outputs detailed logs showing:
- State tracking (set/get/clear response_id)
- Filtering decisions (incremental vs full history)
- Item type classification (LLM vs user inputs)
- Final input composition

## Log Flow Example

### Scenario: Full Conversation with Tool Call

```
Turn 1: User sends first message
════════════════════════════════════════════════════════════════

DEBUG codex::previous_response_id: Input mode decision: adapter_supports=true, pending_items=0, mode=incremental
DEBUG codex::previous_response_id: Building incremental input from history (total_items=1)
DEBUG codex::previous_response_id: No LLM-generated items in history - first turn, returning full history (total_items=1)
DEBUG codex::previous_response_id: Incremental filtering complete: 1 user input items (from 1 total history items, last_llm_idx=None)
DEBUG codex::previous_response_id: Built incremental input: 1 total items (1 filtered + 0 pending)

DEBUG codex: Building prompt without previous_response_id (full history mode)
→ Server receives: Full history (1 user message)


Turn 1: Server responds with FunctionCall
════════════════════════════════════════════════════════════════

DEBUG codex_core::state::session: Setting last_response_id for incremental mode: response_id=resp-1
→ State updated: last_response_id = Some("resp-1")


Turn 2: Tool execution completes, sending output
════════════════════════════════════════════════════════════════

DEBUG codex::previous_response_id: Input mode decision: adapter_supports=true, pending_items=0, mode=incremental
DEBUG codex::previous_response_id: Building incremental input from history (total_items=3)
DEBUG codex::previous_response_id: Found last LLM-generated item at index 2 (type=FunctionCall, total_items=3)
DEBUG codex::previous_response_id: Filtering 0 items after last LLM output (skip 3 items)
DEBUG codex::previous_response_id: Incremental filtering complete: 0 user input items (from 3 total history items, last_llm_idx=Some(2))

→ Filtered result: []  (no items after last FunctionCall yet)
→ Pending input: [FunctionCallOutput]
→ Final input: [FunctionCallOutput]

DEBUG codex::previous_response_id: Built incremental input: 1 total items (0 filtered + 1 pending)

DEBUG codex: Building prompt with previous_response_id for incremental mode: response_id=resp-1
→ Server receives: previous_response_id="resp-1", input=[FunctionCallOutput]


Turn 2: Server responds with AssistantMessage
════════════════════════════════════════════════════════════════

DEBUG codex_core::state::session: Setting last_response_id for incremental mode: response_id=resp-2
→ State updated: last_response_id = Some("resp-2")


Turn 3: User sends new message
════════════════════════════════════════════════════════════════

DEBUG codex::previous_response_id: Input mode decision: adapter_supports=true, pending_items=1, mode=incremental
DEBUG codex::previous_response_id: Building incremental input from history (total_items=5)
DEBUG codex::previous_response_id: Found last LLM-generated item at index 4 (type=Message(assistant), total_items=5)
DEBUG codex::previous_response_id: Filtering 0 items after last LLM output (skip 5 items)
DEBUG codex::previous_response_id: Incremental filtering complete: 0 user input items (from 5 total history items, last_llm_idx=Some(4))

→ Filtered result: []  (no items after last AssistantMessage yet)
→ Pending input: [Message(user)]
→ Final input: [Message(user)]

DEBUG codex::previous_response_id: Built incremental input: 1 total items (0 filtered + 1 pending)

DEBUG codex: Building prompt with previous_response_id for incremental mode: response_id=resp-2
→ Server receives: previous_response_id="resp-2", input=[Message(user)]


Compact Operation Triggered
════════════════════════════════════════════════════════════════

DEBUG codex_core::state::session: Replacing history (2 items) and clearing response_id (was: resp-3)
DEBUG codex_core::state::session: History replacement complete: 2 items, tracking_cleared=true
→ State updated: last_response_id = None
→ History replaced with compacted summary


Turn 4: After compact
════════════════════════════════════════════════════════════════

DEBUG codex::previous_response_id: Input mode decision: adapter_supports=true, pending_items=1, mode=incremental
DEBUG codex::previous_response_id: Building incremental input from history (total_items=2)
DEBUG codex::previous_response_id: Found last LLM-generated item at index 0 (type=Message(assistant), total_items=2)
DEBUG codex::previous_response_id: Filtering 1 items after last LLM output (skip 1 items)
DEBUG codex::previous_response_id: Incremental filtering complete: 1 user input items (from 2 total history items, last_llm_idx=Some(0))

→ Filtered result: [Message(user)]
→ Pending input: []
→ Final input: [Message(user)]

DEBUG codex: Building prompt without previous_response_id (full history mode)
→ Server receives: Full history (compacted summary + new user message)


Error Recovery: previous_response_not_found
════════════════════════════════════════════════════════════════

DEBUG codex_core::state::session: Clearing last_response_id (was: resp-stale)
WARN  codex: Previous response ID not found on server - cleared tracking, retrying with full history
→ State updated: last_response_id = None
→ Retry with full history


Defensive Filtering Warning (should be rare)
════════════════════════════════════════════════════════════════

WARN  codex::previous_response_id: Filtering out LLM item at index 5 (type=Reasoning) - unexpected after last LLM output
→ This indicates history is in an unexpected state
→ Defensive filter prevents sending duplicate LLM items to server
```

## Log Levels

- **DEBUG**: Normal operation tracking
  - State changes (set/clear response_id)
  - Filtering decisions and results
  - Input composition breakdown

- **WARN**: Unexpected conditions (non-critical)
  - Defensive filtering of LLM items
  - Empty input fallback to full history

## Key Log Locations

### State Management (`core/src/state/session.rs`)

```rust
set_last_response()                    // Line ~86
clear_last_response()                  // Line ~102
replace_history_and_clear_tracking()   // Line ~51
```

### Filtering Logic (`core/src/previous_response_id/input_builder.rs`)

```rust
build_turn_input()                     // Line ~83 (decision log)
build_incremental_input_filtered()     // Line ~159 (filtering logs)
get_item_type_name()                   // Line ~248 (helper for logging)
```

### Integration Points (`core/src/codex.rs`)

```rust
Building prompt with/without previous_response_id  // Line ~1947
Error recovery clearing                            // Line ~2000
```

## Interpreting Logs

### Successful Incremental Flow

```
adapter_supports=true
→ Building incremental input
→ Found last LLM item
→ Filtered X user input items
→ Building prompt WITH previous_response_id
```

### Full History Fallback (First Turn)

```
adapter_supports=true
→ No LLM-generated items in history - first turn
→ Building prompt WITHOUT previous_response_id
```

### Full History Fallback (After Compact)

```
Replacing history and clearing response_id
→ Building prompt WITHOUT previous_response_id
→ Sends compacted summary + new messages
```

### Error Recovery

```
Clearing last_response_id (was: resp-XXX)
→ Previous response ID not found
→ Retry with full history
```

## Performance Impact

All logs use `tracing::debug!()` which:
- Zero cost when not enabled
- Minimal overhead when enabled (structured logging)
- Can be filtered by module: `RUST_LOG=codex::previous_response_id=debug`

## Testing Log Output

```bash
# Enable debug logs for specific module
RUST_LOG=codex::previous_response_id=debug cargo test

# Enable all debug logs
RUST_LOG=debug cargo run

# Enable specific log level
RUST_LOG=codex_core::state::session=trace cargo run
```

## Troubleshooting with Logs

### Issue: Tool output not sent

**Expected logs**:
```
Found last LLM-generated item at index X (type=FunctionCall)
Built incremental input: N total items (M filtered + P pending)
```

**Check**:
- Is `P pending` > 0? (Should include FunctionCallOutput)
- Is `adapter_supports=true`?
- Is `previous_response_id` set?

### Issue: Duplicate items sent

**Expected logs**:
```
WARN: Filtering out LLM item at index X (type=Y)
```

**Action**: This is defensive filtering catching an edge case. File a bug report with full logs.

### Issue: Full history sent when incremental expected

**Expected logs**:
```
Building prompt without previous_response_id
```

**Check**:
- Was history recently replaced? (Look for "Replacing history")
- Was there an error recovery? (Look for "Clearing last_response_id")
- Is this the first turn? (Look for "No LLM-generated items")

## Log Examples by Scenario

See integration tests in `core/tests/suite/incremental_input.rs` for scenarios that exercise these logs.
