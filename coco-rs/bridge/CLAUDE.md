# coco-bridge

Standalone bridge for SDK / IDE / remote-control consumers. Covers two distinct
subsystems:

1. **REPL bridge** — NDJSON over stdio for programmatic SDK use (Python, CCR).
2. **IDE bridge** — permission relay + message pump for VS Code / JetBrains
   (detected via MCP lockfiles in TS; coco-rs ships the skeleton for parity).

## TS Source

- `bridge/replBridge.ts`, `replBridgeHandle.ts`, `replBridgeTransport.ts` — REPL bridge core
- `bridge/bridgeApi.ts`, `bridgeConfig.ts`, `bridgeMain.ts`, `bridgeMessaging.ts`, `bridgeUI.ts` — CCR daemon wiring
- `bridge/bridgePermissionCallbacks.ts`, `inboundMessages.ts`, `inboundAttachments.ts` — permission relay + attachment ingest
- `bridge/jwtUtils.ts`, `workSecret.ts`, `trustedDevice.ts` — session auth + device trust
- `bridge/sessionRunner.ts`, `createSession.ts`, `codeSessionApi.ts` — session lifecycle
- `bridge/capacityWake.ts`, `flushGate.ts`, `pollConfig.ts` — backpressure / polling
- `bridge/sessionIdCompat.ts`, `remoteBridgeCore.ts`, `envLessBridgeConfig.ts`, `bridgePointer.ts` — transport compat

## Key Types

| Module | Types |
|--------|-------|
| `protocol` | `BridgeInMessage` (Submit/Approve/Deny/Cancel/Ping), `BridgeOutMessage` (Text/ToolUse/ToolResult/PermissionRequest/Status/Error/Pong), `BridgeTransport` (WebSocket/Sse/Ndjson) |
| `repl` | `ReplBridge`, `BridgeState`, `ReplInMessage`, `ReplOutMessage`, `ControlRequest`, `ControlRequestHandler`, `RejectingControlHandler`, `dispatch_control`, `ControlError` |
| `server` | `BridgeServer` — NDJSON / WS listener skeleton |
| `permission_callbacks` | `BridgePermissionRequest`, `BridgePermissionResponse`, `BridgeDecision`, `BridgeRiskLevel` — IDE-side permission arbitration |
| `jwt_utils` | `Claims`, `JwtError` — session-ingress JWT |
| `work_secret` | `derive_secret_from_material`, `generate_fresh_secret`, `account_name_for_workspace` — keyring-backed per-workspace secret |
| `trusted_device` | `TrustedDevice`, `TrustedDeviceStore` — long-lived device trust |
| `ide` | IDE-specific adapters (detection, outbound RPC: `openDiff`, `openFile`, `getDiagnostics`) |

## Architecture

```
SDK / IDE ─► BridgeInMessage ─► dispatch_control / ControlRequestHandler
                                       │
                                       ├─ Submit → QueryEngine (via handle)
                                       ├─ Approve/Deny → PermissionCallbacks
                                       └─ Cancel → CancellationToken
                                       
QueryEngine ─► CoreEvent ─► BridgeOutMessage ─► transport (NDJSON/WS/SSE)
```

The IDE bridge in TS is NOT a direct IDE↔agent WebSocket relay. IDEs connect as
MCP servers (not through this crate). This crate owns the REPL bridge (stdio
NDJSON) and permission-relay primitives. The CCR daemon spawn modes
(SingleSession / Worktree / SameDir) are documented in
`docs/coco-rs/crate-coco-bridge.md` and wired from `coco-cli` (`Commands::RemoteControl`).
