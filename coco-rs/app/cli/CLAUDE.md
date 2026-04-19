# coco-cli

Top-level CLI: clap parser, binary entry, SDK (NDJSON over stdio), subcommand dispatch.
Depends on everything — wires registries, builds `ApiClient`, starts TUI or SDK server.

## TS Source

- `entrypoints/cli.tsx` — two-tier dispatch (fast-path branches before Commander)
- `entrypoints/sdk/{coreSchemas.ts,controlSchemas.ts,coreTypes.ts}` — SDK protocol
- `cli/structuredIO.ts` — NDJSON control loop (request/response subtypes)
- `cli/transports/{WebSocket,SSE,Hybrid}Transport.ts` + `ccrClient.ts`
- `cli/handlers/{agents.ts,auth.ts,autoMode.ts,mcp.tsx,plugins.ts,util.tsx}` — subcommand handlers
- `cli/{print,exit,update,remoteIO}.ts`
- `entrypoints/{init.ts,mcp.ts,agentSdkTypes.ts,sandboxTypes.ts}` — subcommand entries + SDK types
- `utils/releaseNotes.ts` — release-notes subcommand data source
- `server/{createDirectConnectSession,directConnectManager}.ts` — DirectConnect HTTP+WS
- `main.tsx` — Commander construction (preAction: init/migrations/policyLimits)

## Key Types

| Type | Purpose |
|------|---------|
| `Cli` (clap `Parser`) | Binary name `coco`; TS-parity flags (see `lib.rs`) |
| `Commands` | Subcommands: Chat, Config, Resume, Sessions, Status, Doctor, Login, Mcp, Plugin, Daemon, Ps/Logs/Attach/Kill, RemoteControl, Sdk, ReleaseNotes, Upgrade, Agents, AutoMode |
| `{Config,Mcp,Plugin}Action` | Subcommand action enums |
| `sdk_server::SdkServer` | NDJSON control server (Commands::Sdk) |
| `sdk_server::StdioTransport` | stdin/stdout NDJSON transport |
| `sdk_server::QueryEngineRunner` | Bridges `QueryEngine` to SDK control messages |
| `sdk_server::CliInitializeBootstrap` | Session bootstrap from `initialize` control request |
| `sdk::ModelUsage` + schemas | SDK wire types (mirrors `coreSchemas.ts`) |
| `tui_runner::*` | Launches `coco-tui` after bootstrap |
| `model_factory::*` | Builds `Arc<dyn LanguageModelV4>` from provider/model config |
| `output::*` | Non-interactive output formatters (text/json/stream-json) |

## Startup Flow

1. `Cli::parse()` — clap parses argv (subcommand or default chat)
2. Fast-path subcommands (Config, Doctor, ReleaseNotes, Sessions, Upgrade) bypass QueryEngine
3. Interactive/print/SDK paths: load config, build `ApiClient` via `model_factory`, register tools + commands
4. `Sdk` → `sdk_server` (NDJSON over stdio, `initialize`/`interrupt`/`can_use_tool`/`set_permission_mode`/...)
5. `--non-interactive` (print mode) → single `QueryEngine::run` + `output::*` formatter
6. Interactive → `tui_runner::run` (launches `coco-tui`)

## Flag Highlights (TS parity)

Session: `--prompt`, `--output-format`, `--input-format`, `--json-schema`, `--max-turns`, `--max-budget-usd`
Resume: `--continue`, `--resume`, `--fork-session`, `--session-id`, `--name`
Auth/Perms: `--dangerously-skip-permissions`, `--allow-dangerously-skip-permissions`, `--permission-mode`, `--permission-prompt-tool`
Tools: `--allowed-tools`, `--disallowed-tools`, `--add-dir`
Config: `--settings`, `--setting-sources`, `--system-prompt`, `--append-system-prompt(-file)`, `--mcp-config`, `--strict-mcp-config`
Model: `--model`, `--fallback-model`, `--betas`, `--agent`, `--thinking`, `--thinking-budget`, `--max-thinking-tokens`, `--effort`
Worktree/bg: `--worktree`, `--bg`
SDK: `--replay-user-messages`, `--include-hook-events`, `--include-partial-messages`
