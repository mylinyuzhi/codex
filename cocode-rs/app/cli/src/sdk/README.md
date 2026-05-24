# SDK Mode

Non-interactive JSON-RPC 2.0 interface for programmatic access to the cocode agent loop.

## Architecture

```
Python SDK (cocode-sdk)
       |
       v
  subprocess: cocode --sdk-mode
       |
  ┌────┴─────────────────────────────────┐
  │  stdio.rs   StdinReader / StdoutWriter│  NDJSON over stdin/stdout
  ├──────────────────────────────────────┤
  │  mod.rs     run_sdk_mode()           │  Session lifecycle + turn loop
  │             run_sdk_turn_loop()      │  tokio::select! event multiplexing
  ├──────────────────────────────────────┤
  │  session_builder.rs                  │  SessionStartRequestParams → SessionState
  │    apply_sdk_params()                │  Agents, hooks, MCP, tools, sandbox, thinking
  │    SdkHookBridge                     │  Hook callbacks via JSON-RPC request/response
  ├──────────────────────────────────────┤
  │  control.rs                          │  SdkPermissionBridge (tool approval flow)
  ├──────────────────────────────────────┤
  │  mcp_bridge.rs                       │  SdkMcpBridge (in-process @tool routing)
  └──────────────────────────────────────┘
       |
       v
  Core agent loop (cocode-core)
```

## Wire Protocol

All messages use JSON-RPC 2.0 over NDJSON (one JSON object per line).

**Client -> Server** (`ClientRequest`, routed by `method` field):
- `session/start` - Start new session with full configuration
- `session/resume` - Resume existing session by ID
- `turn/start` - Submit user message for a new turn
- `turn/interrupt` - Cancel the current turn
- `approval/resolve` - Respond to tool permission requests
- `input/resolveUserInput` - Respond to agent questions
- `hook/callbackResponse` - Respond to SDK hook callbacks
- `mcp/routeMessageResponse` - Respond to MCP tool routing
- `control/setModel`, `control/setPermissionMode`, `control/setThinking` - Runtime config
- `control/stopTask` - Cancel background agent
- `control/rewindFiles` - Rewind file state to a previous turn
- `control/updateEnv` - Update environment variables
- `control/keepAlive` - Heartbeat

**Server -> Client notifications** (`ServerNotification`, 56 typed variants):
- Session: `session/started`, `session/result`, `session/ended`
- Turn: `turn/started`, `turn/completed`, `turn/failed`, `turn/interrupted`, `turn/maxReached`
- Items: `item/started`, `item/updated`, `item/completed`
- Content: `agentMessage/delta`, `reasoning/delta`
- Subagent: `subagent/spawned`, `subagent/completed`, `subagent/backgrounded`
- Context: `context/compacted`, `context/usageWarning`
- Tasks: `task/started`, `task/completed`, `task/progress`
- System: `error`, `rateLimit`, `keepAlive`, `cost/warning`
- And more (MCP, model fallback, plan mode, sandbox, IDE, queue, rewind, hooks)

**Server -> Client requests** (`ServerRequest`, bidirectional with `id`):
- `approval/askForApproval` - Request tool permission decision
- `input/requestUserInput` - Request user input (questions, MCP elicitation)
- `hook/callback` - Invoke SDK-registered hook
- `mcp/routeMessage` - Route MCP call to SDK-managed server

## Key Features

- **Full agent capabilities**: All tools, subagents, background agents, worktree isolation
- **Bidirectional hooks**: SDK registers hook callbacks at session start; agent invokes them via JSON-RPC
- **MCP bridge**: SDK-managed MCP servers (`@tool()` decorator) routed through the control channel
- **Permission control**: Tool approvals via `AskForApproval` / `ApprovalResolve` request-response
- **MCP permission prompt tool**: Route permission decisions through an MCP tool for enterprise automation (fallback to SDK bridge)
- **Prompt suggestions**: Optional post-turn prompt suggestions (`prompt_suggestions: true`)
- **File rewind**: Rewind file and conversation state to a previous turn via `control/rewindFiles`
- **Structured output**: JSON schema validation on session result
- **Session management**: Start, resume, max turns, max budget enforcement
- **Streaming**: ThreadItem-based structured events (not raw SSE) for agent messages, reasoning, tool calls, file changes

## Transport: Stdio Only (Current)

The current implementation supports **local subprocess transport only** (stdin/stdout NDJSON).
This is the primary mode used by the Python SDK (`cocode-sdk/python`), which spawns the CLI
as a child process.

### Remote Transport (Future)

Remote transport (WebSocket, SSE, HTTP hybrid) is not yet implemented. When needed, this would
enable:
- Hosted/cloud SDK deployments
- Persistent connections with reconnection and message replay
- Batched outbound event streaming with backpressure
- Multi-client session sharing

The protocol layer (`app-server-protocol`) is transport-agnostic by design, so adding remote
transports requires no protocol changes — only new transport implementations alongside `StdinReader`/`StdoutWriter`.

## Python SDK

The Python SDK lives at `cocode-sdk/python/`. It spawns this CLI mode as a subprocess and
provides a Pythonic async API:

- `query()` - One-shot stateless queries
- `CocodeClient` - Bidirectional multi-turn sessions
- Custom tools via `@tool` decorator (routed through MCP bridge)
- Hook callbacks, permission control, structured output
- Auto-generated protocol types from JSON schema (`generated/protocol.py`)
