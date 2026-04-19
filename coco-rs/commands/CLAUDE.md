# coco-commands

Slash command registry and built-in implementations (help, config, clear, compact, model, session, login, mcp, plugin, diff, commit, pr, review, doctor, ...). ~96 commands across v1/v2/v3 in TS.

## TS Source
- `commands.ts` — top-level registry (loads + registers builtins)
- `commands/` — 88+ subdirs, one per command (each with its own `.ts` or `.tsx`)
- Top-level file commands: `commands/commit.ts`, `commit-push-pr.ts`, `bridge-kick.ts`, `brief.ts`, `advisor.ts`, `createMovedToPluginCommand.ts`

Paths relative to `/lyz/codespace/3rd/claude-code/src/`.

## Key Types
- `CommandHandler` trait — `execute(args: &str) -> Result<String>`
- `RegisteredCommand` — metadata (`CommandBase` from coco-types) + optional handler + `is_enabled` feature-flag gate
- `CommandRegistry` — name-keyed map with alias lookup; filter views: `visible()`, `sdk_safe()` (strips `is_sensitive`), `safe_for(CommandSafety)`
- `BuiltinCommand` / `AsyncBuiltinCommand` — sync and async built-in handler wrappers
- `builtin_base()`, `builtin_base_ext()` — construct default `CommandBase` with safety + argument-hint options
- `register_builtins()` — registers the starter ~25; `register_extended_builtins` in `implementations::`

## Modules
- `handlers/` — richer command handlers that need app state
- `implementations/` — extended builtin registrations and shared `names` constants
