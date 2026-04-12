# vercel-ai-provider Development Guide

## Overview

This crate provides Rust type definitions matching `@ai-sdk/provider` v4 specification. It is a standalone types crate with no dependencies on other coco crates.

## Key Differences from hyper-sdk

1. **Method Naming**: Uses `do_generate`, `do_stream`, `do_embed` prefix (Vercel v4 convention)
2. **Prompt Types**: More granular `LanguageModelV4Prompt` with typed message variants
3. **Content Parts**: Separate `UserContentPart`, `AssistantContentPart`, `ToolContentPart` enums
4. **Stream Events**: Granular events with IDs (text-start, text-delta, text-end, etc.)
5. **Provider Extensibility**: `ProviderOptions`/`ProviderMetadata` for provider-specific data

## Module Structure

| Module | Purpose |
|--------|---------|
| `language_model` | `LanguageModelV4` trait + call options + results |
| `embedding_model` | `EmbeddingModelV4` trait + types |
| `image_model` | `ImageModelV4` trait + types |
| `provider` | `ProviderV4` trait |
| `prompt` | `LanguageModelV4Prompt`, message types |
| `content` | Content part enums (Text, File, Reasoning, ToolCall, etc.) |
| `tool` | Tool definitions, tool choice, tool call/result |
| `usage` | Token usage types |
| `stream` | Stream part types for streaming responses |
| `errors` | Error type hierarchy (AISdkError, etc.) |
| `shared` | ProviderOptions, ProviderMetadata, Warning |

## Type Mapping (TypeScript → Rust)

| TypeScript | Rust |
|------------|------|
| Union types | Enums with variants |
| Optional fields | `Option<T>` |
| Record<string, T> | `HashMap<String, T>` |
| AbortSignal | `CancellationToken` |
| ReadableStream | `futures::Stream` |
| JSONValue | `serde_json::Value` (type alias) |

## Design Principles

1. **Standalone**: No dependencies on other coco crates
2. **Type-safe**: Strongly typed with enums for discriminated unions
3. **Async**: Use `async-trait` for async trait methods
4. **Error chaining**: Proper error types with `thiserror`

## Testing

```bash
# From coco-rs directory
cargo test -p vercel-ai-provider
cargo check -p vercel-ai-provider
cargo clippy -p vercel-ai-provider
```