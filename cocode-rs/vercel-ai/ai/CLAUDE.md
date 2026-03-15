# vercel-ai Development Guide

## Overview

This crate provides high-level API functions for LLM interactions, matching `@ai-sdk/ai` TypeScript package. It builds on top of `vercel-ai-provider` types and `vercel-ai-provider-utils` utilities.

## Core Functions

| Function | Description |
|----------|-------------|
| `generate_text` | Generate text from a prompt (non-streaming) |
| `stream_text` | Stream text generation |
| `generate_object` | Generate structured output matching a JSON schema |
| `stream_object` | Stream structured output generation |
| `embed` | Generate embeddings for text |
| `embed_many` | Generate embeddings for multiple texts |

## Module Structure

| Module | Purpose |
|--------|---------|
| `generate_text` | `generate_text`, `stream_text`, result types |
| `generate_object` | `generate_object`, `stream_object`, result types |
| `embed` | `embed`, `embed_many`, result types |
| `prompt` | `Prompt` type, `CallSettings` |
| `model` | Model resolution functions |
| `provider` | Global default provider pattern |
| `types` | Re-exports from provider crate |
| `error` | Error types specific to high-level API |

## Global Provider Pattern

The crate supports a global default provider that can be set once and used for all model resolution:

```rust,ignore
use vercel_ai::{set_default_provider, generate_text};
use std::sync::Arc;

// Set a default provider
set_default_provider(Arc::new(my_provider));

// Now generate_text can use string model IDs
let result = generate_text(GenerateTextOptions {
    model: "claude-3-sonnet".into(), // Resolved via default provider
    prompt: Prompt::user("Hello"),
    ..Default::default()
}).await?;
```

## Testing

```bash
# From cocode-rs directory
cargo test -p vercel-ai
cargo check -p vercel-ai
cargo clippy -p vercel-ai
```

## Type Mappings from TypeScript

| TypeScript | Rust |
|------------|------|
| `Promise<T>` | `impl Future<Output = T>` |
| `ReadableStream<T>` | `Pin<Box<dyn Stream<Item = T>>>` |
| `TOOLS extends ToolSet` | `TOOLS: ToolSet` trait bound |
| Union types | Enums |
| `Record<string, T>` | `HashMap<String, T>` |