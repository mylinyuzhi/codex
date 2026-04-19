# vercel-ai-provider

Standalone type definitions matching `@ai-sdk/provider` v4. Zero dependencies on other coco crates.

## TS Source

Ports `@ai-sdk/provider` v4 spec (not from `claude-code/src/`).

## Key Types

Model traits: `LanguageModelV4`, `EmbeddingModelV4`, `ImageModelV4`, `SpeechModelV4`, `TranscriptionModelV4`, `RerankingModelV4`, `VideoModelV4`, `ProviderV4`, `SimpleProvider`.

Language model API: `LanguageModelV4CallOptions`, `LanguageModelV4GenerateResult`, `LanguageModelV4StreamResult`, `LanguageModelV4StreamResponse`, `LanguageModelV4StreamPart`, `LanguageModelV4Request`, `LanguageModelV4Response`, `LanguageModelV4Tool`, `LanguageModelV4ToolChoice`, `LanguageModelV4ProviderTool`, `ReasoningLevel`, `ResponseFormat`, `Source`, `SourceType`, `StreamError`, `ToolApprovalRequest`, `UnifiedFinishReason`, `FinishReason`, `Usage`, `InputTokens`, `OutputTokens`.

Prompt / message: `LanguageModelV4Prompt` (= `Vec<LanguageModelV4Message>`), `LanguageModelV4Message`.

Content parts: `UserContentPart`, `AssistantContentPart`, `ToolContentPart`, `TextPart`, `FilePart`, `ReasoningPart`, `ReasoningFilePart`, `ToolCallPart`, `ToolResultPart`, `ToolResultContent`, `ToolResultContentPart`, `CustomPart`, `FileIdReference`, `DataContent`.

Embedding / image / speech / transcription / reranking / video: `EmbeddingModelV4CallOptions`, `EmbeddingModelV4EmbedResult`, `EmbeddingType`, `EmbeddingUsage`, `EmbeddingValue`, `ImageModelV4CallOptions`, `ImageModelV4GenerateResult`, `ImageModelV4File`, `ImageModelV4Response`, `ImageModelV4Usage`, `GeneratedImage`, `ImageData`, `ImageFileData`, `ImageQuality`, `ImageResponseFormat`, `ImageSize`, `ImageStyle`, `SpeechModelV4CallOptions`, `SpeechModelV4Result`, `TranscriptionModelV4CallOptions`, `TranscriptionModelV4Result`, `TranscriptionSegmentV4`, `RerankingModelV4CallOptions`, `RerankingModelV4Result`, `RankedItem`, `RerankDocuments`, `VideoModelV4CallOptions`, `VideoModelV4Result`, `VideoData`, `VideoDuration`, `VideoSize`.

Middleware: `LanguageModelV4Middleware`, `EmbeddingModelV4Middleware`, `ImageModelV4Middleware`.

Shared / JSON / metadata: `ProviderOptions`, `ProviderMetadata`, `Warning`, `JSONSchema`, `JSONValue`, `JSONArray`, `JSONObject`, `ResponseMetadata`, `ToolInvocation`, `ToolInputExample`, `ToolDefinitionV4` (alias), `ToolChoice` (alias).

Errors: `AISdkError`, `APICallError`, `EmptyResponseBodyError`, `InvalidArgumentError`, `InvalidPromptError`, `InvalidResponseDataError`, `JSONParseError`, `LoadAPIKeyError`, `LoadSettingError`, `NoContentGeneratedError`, `NoSuchModelError`, `ProviderError`, `TooManyEmbeddingValuesForCallError`, `TypeValidationContext`, `TypeValidationError`, `UnsupportedFunctionalityError`.

## v4 Conventions

- Method naming: `do_generate`, `do_stream`, `do_embed` (v4 prefix).
- Prompt: `LanguageModelV4Prompt` = `Vec<LanguageModelV4Message>` with typed `User` / `Assistant` / `Tool` / `System` variants.
- Streaming: granular events with IDs — `TextStart` / `TextDelta` / `TextEnd`, `ReasoningStart` / `ReasoningDelta` / `ReasoningEnd`, `ToolInputStart` / `ToolInputDelta` / `ToolInputEnd`, `ToolCall`, `ToolResult`.
- Provider extensibility: `ProviderOptions` / `ProviderMetadata` carry `serde_json::Value` — intentional extension point for unknown provider fields (the one `Value` use that does NOT violate the "typed structs over JSON values" rule).
- Errors use `thiserror` (standalone — this crate has no `coco-error` dep).
