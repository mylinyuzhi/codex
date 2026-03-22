# vercel-ai-provider-utils Development Guide

## Overview

This crate provides utility functions for implementing AI providers that follow the Vercel AI SDK v4 specification.

## Key Utilities

| Module | Purpose |
|--------|---------|
| `load_api_key` | API key loading from environment variables |
| `load_setting` | Generic setting loading utilities |
| `headers` | Header manipulation utilities |
| `json` | JSON parsing utilities |
| `generate_id` | ID generation utilities |
| `api` | API posting/getting utilities |
| `response_handler` | Response handling traits and implementations |
| `schema` | Schema utilities for structured output |

## Design Principles

1. **Standalone**: Only depends on `vercel-ai-provider` for types
2. **Async-first**: All I/O operations are async
3. **Error propagation**: Uses `AISdkError` from provider crate
4. **Cancellation**: All async operations support `CancellationToken`

## Testing

```bash
# From cocode-rs directory
cargo test -p vercel-ai-provider-utils
cargo check -p vercel-ai-provider-utils
cargo clippy -p vercel-ai-provider-utils
```