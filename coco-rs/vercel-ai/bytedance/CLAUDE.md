# vercel-ai-bytedance

ByteDance Seedance video provider for Vercel AI SDK v4 (via ModelArk API).

## TS Source

Not a port of anything in `@ai-sdk/*` or `claude-code/src/` — coco-rs addition covering ByteDance's ModelArk video API (Seedance model family).

## Key Types

- `ByteDanceProvider`, `ByteDanceProviderSettings`, `bytedance()` (default), `create_bytedance()`
- `ByteDanceVideoModel`, `ByteDanceVideoModelConfig`
- `ByteDanceVideoProviderOptions`, video settings module
- `ByteDanceErrorData`, `ByteDanceFailedResponseHandler`
- `map_resolution` — maps generic video resolutions to Seedance-specific strings

## Conventions

- Reads `ARK_API_KEY` by default.
- Only exposes `provider.video_model(id)` — no language / embedding / image models.
- Supported model: `seedance-1-5-pro-251215` (and successors).
