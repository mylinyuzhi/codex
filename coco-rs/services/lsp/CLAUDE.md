# coco-lsp

AI-friendly LSP client — queries by symbol name + kind instead of exact line/column. Rust-native LSP core (server manager, lifecycle, JSON-RPC, symbol cache, incremental sync) + TS-ported `services/lsp/` diagnostic store + plugin extensions layered on top.

## TS Source
- `services/lsp/LSPServerManager.ts` — multi-server lifecycle (Rust: `server.rs`)
- `services/lsp/LSPServerInstance.ts` — per-server instance + health (Rust: `lifecycle.rs`)
- `services/lsp/LSPClient.ts` — LSP operations (Rust: `client.rs`)
- `services/lsp/LSPDiagnosticRegistry.ts` — diagnostic store + debounce (Rust: `diagnostics.rs`)
- `services/lsp/config.ts` — config loading (Rust: `config.rs`)
- `services/lsp/manager.ts` — manager coordination
- `services/lsp/passiveFeedback.ts` — passive feedback

## Key Types

- `LspServerManager`, `ServerConfigInfo`, `ServerStatus`, `ServerStatusInfo` — top-level manager
- `LspClient` — per-connection AI-friendly operations + caching
- `LspServersConfig`, `LspServerConfig`, `BuiltinServer`, `BUILTIN_SERVERS`, `ConfigLevel`, `LifecycleConfig`, `LSP_SERVERS_CONFIG_FILE`, `command_exists`
- `SymbolKind`, `ResolvedSymbol`, `SymbolMatch`, `find_matching_symbols`, `flatten_symbols`
- `DiagnosticsStore`, `DiagnosticEntry`, `DiagnosticSeverityLevel`
- `ServerLifecycle`, `ServerHealth`, `ServerStats`
- `LspInstaller`, `InstallEvent`, `InstallerType`
- `TimeoutConfig`, `LspErr`
- `create_manager()` — convenience constructor

## AI-Friendly Symbol Resolution

Query by name+kind instead of position:
```rust
client.definition(path, "Config", Some(SymbolKind::Struct)).await?;
client.references(path, "process", Some(SymbolKind::Function), true).await?;
```
Position-based variants available with `_at_position` suffix. `SymbolKind::from_str_loose()` accepts `fn`/`func`/`function`, `trait`/`interface`, `var`/`let`/`variable`, etc.

## Built-in Language Servers

| Server | Extensions | Install |
|--------|------------|---------|
| rust-analyzer | `.rs` | `rustup component add rust-analyzer` |
| gopls | `.go` | `go install golang.org/x/tools/gopls@latest` |
| pyright | `.py`, `.pyi` | `npm install -g pyright` |
| typescript-language-server | `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs` | `npm install -g typescript-language-server typescript` |

## Configuration

Config files: `~/.coco/lsp_servers.json` (user) → `.coco/lsp_servers.json` (project overrides). Adding a built-in: extend `BUILTIN_SERVERS` in `config.rs` with `id`, `extensions`, `commands`, `install_hint`, `languages`.

## Constants

| Constant | Value | Purpose |
|----------|-------|---------|
| `MAX_OPENED_FILES` | 500 | File tracking limit |
| `MAX_SYMBOL_CACHE_SIZE` | 100 | Symbol cache entries per file |
| `MAX_INCREMENTAL_CONTENT_SIZE` | 1MB | Incremental sync threshold (Myers diff; falls back to full sync above) |
| `LRU_EVICTION_PERCENT` | 25% | Cache eviction batch size |
| `HEALTH_CHECK_TIMEOUT_SECS` | 5 | Health probe timeout |

## Lifecycle Management

`ServerLifecycle` tracks crashes with max restarts + exponential backoff. Health check tries `workspace/symbol`, falls back to `hover` on any open file.

## Notes

- **Does NOT follow the `*_ext.rs` extension pattern** — direct file modifications preferred.
- Symbol cache invalidation is version-tracked per file.
- Re-exports `lsp_types::{CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Location, SymbolInformation}`.
