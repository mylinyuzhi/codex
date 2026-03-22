# OpenAI SDK for Rust

Rust SDK for the OpenAI Responses API. This crate provides a full-featured client for interacting with OpenAI models, including streaming support.

Reference: [openai-python](https://github.com/openai/openai-python) @ `722d3fffb82e9150a16da01e432b70d126ca5254`

## Features

- **Response API** - Create, retrieve, cancel, and stream responses
- **Streaming** - Full SSE streaming with 53 event types
- **Embeddings API** - Generate text embeddings
- **Multi-turn Conversations** - Continue conversations with `previous_response_id`
- **12 Built-in Tool Types** - Web search, file search, code interpreter, computer use, custom tools, and more
- **16 Output Item Types** - Full coverage of response output variants
- **Extended Thinking** - Reasoning mode with configurable token budgets
- **Prompt Caching** - Cache system prompts for improved latency
- **Logprobs** - Token probability information

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
openai-sdk = { path = "../openai" }
```

## Quick Start

```rust
use openai_sdk::{Client, ResponseCreateParams, ResponseInputItem};

#[tokio::main]
async fn main() -> openai_sdk::Result<()> {
    // Create client from OPENAI_API_KEY environment variable
    let client = Client::from_env()?;

    // Build request
    let params = ResponseCreateParams::new("gpt-4o", vec![
        ResponseInputItem::user_text("What is the capital of France?")
    ]);

    // Make API call
    let response = client.responses().create(params).await?;

    // Get text response
    println!("{}", response.text());

    Ok(())
}
```

## API Reference

### Client

```rust
// From environment variable (OPENAI_API_KEY)
let client = Client::from_env()?;

// With explicit API key
let client = Client::new("sk-...");

// With custom configuration
let config = ClientConfig::new("sk-...")
    .base_url("https://api.openai.com/v1")
    .organization("org-...")
    .project("proj-...");
let client = Client::with_config(config)?;
```

### Response API

#### Create Response

```rust
let params = ResponseCreateParams::new("gpt-4o", vec![
    ResponseInputItem::user_text("Hello!")
])
.max_output_tokens(1024)
.temperature(0.7);

let response = client.responses().create(params).await?;
```

#### Retrieve Response

```rust
let response = client.responses().retrieve("resp_abc123").await?;
```

#### Cancel Response

```rust
let response = client.responses().cancel("resp_abc123").await?;
```

### Streaming API

Stream responses in real-time with Server-Sent Events (SSE):

#### Basic Streaming

```rust
use openai_sdk::{Client, ResponseCreateParams, ResponseInputItem, ResponseStreamEvent};

let client = Client::from_env()?;
let params = ResponseCreateParams::new("gpt-4o", vec![
    ResponseInputItem::user_text("Write a short story about a robot.")
]);

let mut stream = client.responses().stream(params).await?;

while let Some(event) = stream.next().await {
    match event? {
        ResponseStreamEvent::OutputTextDelta { delta, .. } => {
            print!("{}", delta);
        }
        ResponseStreamEvent::ResponseCompleted { response, .. } => {
            println!("\n\nDone! Response ID: {}", response.id);
        }
        ResponseStreamEvent::Error { message, .. } => {
            eprintln!("Error: {}", message);
        }
        _ => {}
    }
}
```

#### Collect Full Response

```rust
let mut stream = client.responses().stream(params).await?;
let response = stream.collect_response().await?;
println!("{}", response.text());
```

#### Stream Text Only

```rust
let mut stream = client.responses().stream(params).await?;
let text = stream.text_deltas().await?;
println!("{}", text);
```

#### Resume Interrupted Stream

```rust
// Resume from sequence number 10
let mut stream = client.responses()
    .stream_from("resp_abc123", Some(10))
    .await?;

while let Some(event) = stream.next().await {
    // Process events starting after sequence 10
}
```

#### Use with futures Stream

```rust
use futures::StreamExt;

let stream = client.responses().stream(params).await?;
let mut event_stream = stream.into_stream();

while let Some(event) = event_stream.next().await {
    // Process events using futures combinators
}
```

### Multi-turn Conversations

```rust
// First turn
let response1 = client.responses().create(
    ResponseCreateParams::new("gpt-4o", vec![
        ResponseInputItem::user_text("Remember the number 42")
    ])
).await?;

// Continue conversation
let response2 = client.responses().create(
    ResponseCreateParams::new("gpt-4o", vec![
        ResponseInputItem::user_text("What number did I mention?")
    ])
    .previous_response_id(&response1.id)
).await?;
```

### Function Calling

```rust
use openai_sdk::{Tool, FunctionDefinition, InputContentBlock};

// Define function tool
let weather_tool = Tool::function(FunctionDefinition {
    name: "get_weather".into(),
    description: Some("Get current weather".into()),
    parameters: Some(serde_json::json!({
        "type": "object",
        "properties": {
            "location": { "type": "string" }
        },
        "required": ["location"]
    })),
    strict: Some(true),
});

let params = ResponseCreateParams::new("gpt-4o", vec![
    ResponseInputItem::user_text("What's the weather in Tokyo?")
])
.tools(vec![weather_tool]);

let response = client.responses().create(params).await?;

// Check for function calls
if response.has_function_calls() {
    for (call_id, name, args) in response.function_calls() {
        println!("Call {}: {}({})", call_id, name, args);

        // Provide function output for next turn
        let result = r#"{"temp": "22C", "condition": "sunny"}"#;
        let output = InputContentBlock::FunctionCallOutput {
            call_id: call_id.to_string(),
            output: result.to_string(),
        };
        // Include in next request...
    }
}
```

### Built-in Tools

The SDK supports 12 built-in tool types with fluent builder APIs:

```rust
use openai_sdk::{Tool, UserLocation, RankingOptions};

// Web Search
let web_search = Tool::web_search()
    .with_search_context_size("medium")
    .with_user_location(UserLocation {
        city: Some("San Francisco".into()),
        country: Some("US".into()),
        ..Default::default()
    });

// File Search
let file_search = Tool::file_search(vec!["vs_abc123".into()])
    .with_max_results(10)
    .with_ranking_options(RankingOptions {
        ranker: Some("auto".into()),
        score_threshold: Some(0.5),
    });

// Code Interpreter
let code_interpreter = Tool::code_interpreter()
    .with_container("container_abc")
    .with_environment("python:3.11");

// Computer Use
let computer = Tool::computer_use(1920, 1080)
    .with_model("computer-use-preview");

// Image Generation
let image_gen = Tool::image_generation()
    .with_size("1024x1024")
    .with_quality("hd");

// Local Shell
let shell = Tool::local_shell()
    .with_allowed_commands(vec!["ls".into(), "cat".into()]);

// MCP (Model Context Protocol)
let mcp = Tool::mcp("npx -y @anthropic-ai/mcp-server-fetch")
    .with_server_url("http://localhost:3000")
    .with_allowed_tools(vec!["fetch".into()])
    .with_require_approval("never");

// Text Editor
let editor = Tool::text_editor();

// Apply Patch
let patch = Tool::apply_patch();

// Custom Tool (with Lark grammar)
let custom = Tool::custom_with_grammar(
    "apply_patch",
    "Apply file patches",
    "lark",
    include_str!("grammar.lark"),
);

// Custom Tool (unconstrained text)
let custom_text = Tool::custom_text("my_tool", "Process free-form input");
```

### Tool Choice

Control how the model selects tools:

```rust
use openai_sdk::ToolChoice;

// Auto (default) - model decides
.tool_choice(ToolChoice::Auto)

// None - disable tool use
.tool_choice(ToolChoice::None)

// Required - must use a tool
.tool_choice(ToolChoice::Required)

// Force specific function
.tool_choice(ToolChoice::function("get_weather"))

// Force specific tool type
.tool_choice(ToolChoice::hosted_tool("web_search_preview"))
.tool_choice(ToolChoice::file_search())
.tool_choice(ToolChoice::code_interpreter())
.tool_choice(ToolChoice::web_search())
.tool_choice(ToolChoice::computer_use())
.tool_choice(ToolChoice::mcp("server_name"))
```

### Extended Thinking

Enable reasoning mode for complex tasks:

```rust
use openai_sdk::{ThinkingConfig, ReasoningSummary};

let params = ResponseCreateParams::new("o1", vec![
    ResponseInputItem::user_text("Solve this complex math problem...")
])
.thinking(ThinkingConfig::enabled(8192))  // Budget in tokens
.reasoning(ReasoningConfig {
    effort: Some(ReasoningEffort::High),
    summary: Some(ReasoningSummary::Auto),
});

let response = client.responses().create(params).await?;

// Get reasoning content
if let Some(reasoning) = response.reasoning() {
    println!("Reasoning: {}", reasoning);
}
```

### Prompt Caching

Cache system prompts for improved latency:

```rust
use openai_sdk::{PromptCachingConfig, PromptCacheRetention};

let params = ResponseCreateParams::new("gpt-4o", messages)
    .prompt_caching(PromptCachingConfig {
        retention: Some(PromptCacheRetention::Auto),
    });

let response = client.responses().create(params).await?;

// Check cached tokens
let cached = response.cached_tokens();
println!("Used {} cached tokens", cached);
```

### Image Input

```rust
use openai_sdk::{ResponseInputItem, InputContentBlock, ImageSource, ImageDetail};

// From URL
let message = ResponseInputItem::user_message(vec![
    InputContentBlock::text("What's in this image?"),
    InputContentBlock::image_url("https://example.com/image.jpg", ImageDetail::Auto),
]);

// From base64
let message = ResponseInputItem::user_message(vec![
    InputContentBlock::text("Describe this:"),
    InputContentBlock::image_base64(base64_data, ImageMediaType::Png, ImageDetail::High),
]);

// From file ID
let message = ResponseInputItem::user_message(vec![
    InputContentBlock::text("Analyze this:"),
    InputContentBlock::image_file("file-abc123", ImageDetail::Low),
]);
```

### Response Helper Methods

Extract specific output types from responses:

```rust
// Text content
let text = response.text();

// Function calls
for (call_id, name, args) in response.function_calls() { ... }
let has_functions = response.has_function_calls();

// All tool calls (any type)
let has_tools = response.has_tool_calls();

// Web search results
for (call_id, status, results) in response.web_search_calls() { ... }

// File search results
for (call_id, queries, results) in response.file_search_calls() { ... }

// Computer use actions
for (call_id, action) in response.computer_calls() { ... }

// Code interpreter outputs
for (call_id, code, outputs) in response.code_interpreter_calls() { ... }

// MCP tool calls
for mcp_ref in response.mcp_calls() { ... }

// Image generation results
for (call_id, revised_prompt, result) in response.image_generation_calls() { ... }

// Local shell executions
for (call_id, action, result) in response.local_shell_calls() { ... }

// Reasoning/thinking content
let reasoning = response.reasoning();
```

### Multi-turn with Tool Outputs

Provide tool outputs to continue conversations:

```rust
use openai_sdk::InputContentBlock;

// Function call output
InputContentBlock::FunctionCallOutput {
    call_id: "call_123".into(),
    output: r#"{"result": "success"}"#.into(),
}

// Computer use output (screenshot)
InputContentBlock::ComputerCallOutput {
    call_id: "call_456".into(),
    output: ComputerCallOutputData::Screenshot { image_url: "data:...".into() },
    acknowledged_safety_checks: vec![],
}

// Web search output
InputContentBlock::WebSearchCallOutput {
    call_id: "call_789".into(),
    output: Some("Search results...".into()),
}

// Code interpreter output
InputContentBlock::CodeInterpreterCallOutput {
    call_id: "call_abc".into(),
    output: Some("Execution output...".into()),
}

// MCP tool output
InputContentBlock::McpCallOutput {
    call_id: "call_def".into(),
    output: Some("MCP result...".into()),
    error: None,
}

// Custom tool call output
InputContentBlock::custom_tool_call_output("call_xyz", "Tool result...")
```

### Advanced: Reusing Output Items as Input (ResponseInputItem parity)

The Python SDK exposes a `ResponseInputItem` type that lets you feed whole
output items (messages, tool calls, reasoning, etc.) back into the model as
input on subsequent turns.

The Rust SDK provides the same capability while staying close to the wire
format:

- Every `Response` contains an `output: Vec<OutputItem>` list.
- Each `OutputItem` can be serialized to JSON using
  `OutputItem::to_input_item_value()`.
- You can build a `ConversationParam` directly from a list of `OutputItem`s
  using `ConversationParam::from_output_items` and pass it to
  `ResponseCreateParams::conversation(...)`.

This lets you use the model's own previous outputs (including intermediate
tool calls and reasoning) as structured context for the next request.

```rust
use openai_sdk::{Client, ResponseCreateParams, ResponseInputItem, ConversationParam};

#[tokio::main]
async fn main() -> openai_sdk::Result<()> {
    let client = Client::from_env()?;

    // First turn: let the model think and potentially call tools
    let first = client.responses().create(
        ResponseCreateParams::new("gpt-4o", vec![
            ResponseInputItem::user_text("Plan a weekend trip and think step by step."),
        ]),
    ).await?;

    // Build a conversation that reuses all output items from the first turn
    let conversation = ConversationParam::from_output_items(&first.output);

    // Second turn: ask the model to continue from its previous outputs
    let params = ResponseCreateParams::new("gpt-4o", vec![
        ResponseInputItem::user_text("Continue from where you left off and finalize the plan."),
    ])
    .conversation(conversation);

    let second = client.responses().create(params).await?;
    println!("{}", second.text());

    Ok(())
}
```

For more than two turns, you generally want to accumulate all prior
`OutputItem`s (from turns 1..=n-1) and feed them back as conversation
context on turn n. One straightforward pattern is to maintain a list of
JSON items and extend it after each response:

```rust
use openai_sdk::{Client, ResponseCreateParams, ResponseInputItem, ConversationParam};
use serde_json::Value;

#[tokio::main]
async fn main() -> openai_sdk::Result<()> {
    let client = Client::from_env()?;

    // Collect all prior output items as JSON for conversation context.
    let mut conversation_items: Vec<Value> = Vec::new();

    let turns = vec![
        "First, brainstorm 3 possible weekend trips.",
        "Now pick one option and expand it into a day-by-day plan.",
        "Finally, summarize the plan in 3 bullet points.",
    ];

    for (i, prompt) in turns.iter().enumerate() {
        let mut params = ResponseCreateParams::new(
            "gpt-4o",
            vec![ResponseInputItem::user_text(prompt)],
        );

        // For turn > 0, attach all previous output items as conversation
        // context (this mirrors Python's ResponseInputItem behavior).
        if !conversation_items.is_empty() {
            let conv = ConversationParam::Items {
                items: conversation_items.clone(),
            };
            params = params.conversation(conv);
        }

        let response = client.responses().create(params).await?;
        println!("Turn {}: {}", i + 1, response.text());

        // Append this turn's output items so that future turns see them.
        conversation_items.extend(
            response
                .output
                .iter()
                .map(|item| item.to_input_item_value()),
        );
    }

    Ok(())
}
```

If you need even more control (for example, mixing previous `OutputItem`s
with custom input items), you can:

- Call `OutputItem::to_input_item_value()` to get `serde_json::Value` for
  each output item.
- Construct your own `ConversationParam::Items { items: Vec<Value> }` and
  include both previous outputs and custom JSON items.

#### Mixed context example: tool call + tool output

The following example shows how to:

- Keep the previous tool call `OutputItem` in the conversation context, and
- Provide the corresponding tool output as a `FunctionCallOutput` content
  block in the next user message.

```rust
use openai_sdk::{
    Client,
    ConversationParam,
    InputContentBlock,
    ResponseInputItem,
    ResponseCreateParams,
};
use serde_json::Value;

#[tokio::main]
async fn main() -> openai_sdk::Result<()> {
    let client = Client::from_env()?;

    // First turn: the model decides to call a function
    let first = client.responses().create(
        ResponseCreateParams::new("gpt-4o", vec![
            ResponseInputItem::user_text("Use a tool to fetch the current weather in Tokyo."),
        ]),
    ).await?;

    // Collect all output items (including the function_call) as JSON
    let mut conversation_items: Vec<Value> = first
        .output
        .iter()
        .map(|item| item.to_input_item_value())
        .collect();

    // Assume there was at least one function call and we executed it client-side
    let (call_id, tool_result) = (
        "call_123",
        r#"{"temp": "22C", "condition": "sunny"}"#,
    );

    // Second turn: send both the prior tool call (via conversation) and
    // the tool output (via FunctionCallOutput) back to the model.
    let mut params = ResponseCreateParams::new("gpt-4o", vec![
        ResponseInputItem::Message {
            role: openai_sdk::Role::User,
            content: vec![
                InputContentBlock::text(
                    "Here is the result of your tool call, please explain it.",
                ),
                InputContentBlock::FunctionCallOutput {
                    call_id: call_id.to_string(),
                    output: tool_result.to_string(),
                    is_error: None,
                },
            ],
            id: None,
            status: None,
        },
    ]);

    // Attach previous output items as conversation context (ResponseInputItem parity)
    if !conversation_items.is_empty() {
        let conv = ConversationParam::Items {
            items: conversation_items.clone(),
        };
        params = params.conversation(conv);
    }

    let second = client.responses().create(params).await?;
    println!("Second turn: {}", second.text());

    Ok(())
}
```

### Embeddings API

```rust
use openai_sdk::{EmbeddingCreateParams, EncodingFormat};

let params = EmbeddingCreateParams::new(
    "text-embedding-3-small",
    "Hello, world!"
)
.dimensions(256)
.encoding_format(EncodingFormat::Float);

let response = client.embeddings().create(params).await?;

// Single embedding
if let Some(embedding) = response.embedding() {
    println!("Embedding: {:?}", embedding);
}

// Multiple embeddings
for emb in response.data {
    println!("Index {}: {} dimensions", emb.index, emb.embedding.len());
}
```

### Response Includables

Request additional data in responses:

```rust
use openai_sdk::ResponseIncludable;

let params = ResponseCreateParams::new("gpt-4o", messages)
    .include(vec![
        ResponseIncludable::FileSearchResults,
        ResponseIncludable::MessageInputImageUrls,
        ResponseIncludable::ComputerCallOutputImageUrls,
        ResponseIncludable::ReasoningEncryptedContent,
    ]);
```

### Error Handling

```rust
use openai_sdk::{OpenAIError, Result};

match client.responses().create(params).await {
    Ok(response) => println!("{}", response.text()),
    Err(OpenAIError::RateLimited { retry_after }) => {
        println!("Rate limited, retry after {:?}", retry_after);
    }
    Err(OpenAIError::ContextWindowExceeded) => {
        println!("Context window exceeded, reduce input size");
    }
    Err(e) => eprintln!("Error: {}", e),
}
```

### Request Hooks

Intercept and modify HTTP requests before they are sent:

```rust
use openai_sdk::{Client, ClientConfig, HttpRequest, RequestHook};
use std::sync::Arc;

#[derive(Debug)]
struct CustomRequestHook;

impl RequestHook for CustomRequestHook {
    fn on_request(&self, request: &mut HttpRequest) {
        // Modify URL, headers, or body
        request.headers.insert(
            "X-Custom-Header".into(),
            "custom-value".into()
        );
    }
}

let config = ClientConfig::new("sk-...")
    .request_hook(Arc::new(CustomRequestHook));
let client = Client::new(config)?;
```

## Output Item Types

The SDK supports 16 output item types:

| Type | Description |
|------|-------------|
| `Message` | Text message response |
| `FunctionCall` | Function tool invocation |
| `Reasoning` | Extended thinking content |
| `FileSearchCall` | File search tool results |
| `WebSearchCall` | Web search tool results |
| `ComputerCall` | Computer use actions |
| `CodeInterpreterCall` | Code execution results |
| `ImageGenerationCall` | Generated images |
| `LocalShellCall` | Shell command execution |
| `McpCall` | MCP tool invocation |
| `McpListTools` | MCP tool listing |
| `McpApprovalRequest` | MCP approval requests |
| `ApplyPatchCall` | Code patch applications |
| `FunctionShellCall` | Shell function calls |
| `CustomToolCall` | Custom tool invocations |
| `Compaction` | Conversation compaction |

## Response Status

| Status | Description |
|--------|-------------|
| `Completed` | Successfully completed |
| `Failed` | Failed with error |
| `InProgress` | Currently processing |
| `Incomplete` | Stopped early (length, etc.) |
| `Cancelled` | Cancelled by user |
| `Queued` | Waiting in queue |

## Stream Events (53 Types)

All events include a `sequence_number` field for ordering.

### Lifecycle Events

| Event | Description |
|-------|-------------|
| `ResponseCreated` | Response object created |
| `ResponseInProgress` | Processing started |
| `ResponseCompleted` | Successfully completed |
| `ResponseFailed` | Failed with error |
| `ResponseIncomplete` | Stopped early |
| `ResponseQueued` | Waiting in queue |

### Text Output Events

| Event | Description |
|-------|-------------|
| `OutputTextDelta` | Incremental text content |
| `OutputTextDone` | Text content complete |
| `RefusalDelta` | Incremental refusal text |
| `RefusalDone` | Refusal complete |

### Function Call Events

| Event | Description |
|-------|-------------|
| `FunctionCallArgumentsDelta` | Incremental function arguments |
| `FunctionCallArgumentsDone` | Function arguments complete |

### Output Item Events

| Event | Description |
|-------|-------------|
| `OutputItemAdded` | New output item started |
| `OutputItemDone` | Output item complete |
| `ContentPartAdded` | New content part started |
| `ContentPartDone` | Content part complete |

### Reasoning Events

| Event | Description |
|-------|-------------|
| `ReasoningTextDelta` | Incremental reasoning text |
| `ReasoningTextDone` | Reasoning text complete |
| `ReasoningSummaryPartAdded` | Summary part started |
| `ReasoningSummaryPartDone` | Summary part complete |
| `ReasoningSummaryTextDelta` | Incremental summary text |
| `ReasoningSummaryTextDone` | Summary text complete |

### Audio Events

| Event | Description |
|-------|-------------|
| `AudioDelta` | Incremental audio data |
| `AudioDone` | Audio complete |
| `AudioTranscriptDelta` | Incremental transcript |
| `AudioTranscriptDone` | Transcript complete |

### MCP Events

| Event | Description |
|-------|-------------|
| `McpCallInProgress` | MCP call started |
| `McpCallCompleted` | MCP call complete |
| `McpCallFailed` | MCP call failed |
| `McpCallArgumentsDelta` | Incremental MCP arguments |
| `McpCallArgumentsDone` | MCP arguments complete |
| `McpListToolsInProgress` | Tool listing started |
| `McpListToolsCompleted` | Tool listing complete |
| `McpListToolsFailed` | Tool listing failed |

### Tool Call Events

| Event | Description |
|-------|-------------|
| `FileSearchCallInProgress` | File search started |
| `FileSearchCallSearching` | File search in progress |
| `FileSearchCallCompleted` | File search complete |
| `WebSearchCallInProgress` | Web search started |
| `WebSearchCallSearching` | Web search in progress |
| `WebSearchCallCompleted` | Web search complete |
| `CodeInterpreterCallInProgress` | Code interpreter started |
| `CodeInterpreterCallInterpreting` | Code executing |
| `CodeInterpreterCallCompleted` | Code interpreter complete |
| `CodeInterpreterCallCodeDelta` | Incremental code |
| `CodeInterpreterCallCodeDone` | Code complete |
| `ImageGenCallInProgress` | Image generation started |
| `ImageGenCallGenerating` | Image generating |
| `ImageGenCallPartialImage` | Partial image available |
| `ImageGenCallCompleted` | Image generation complete |
| `CustomToolCallInputDelta` | Incremental custom tool input |
| `CustomToolCallInputDone` | Custom tool input complete |

### Annotation Events

| Event | Description |
|-------|-------------|
| `OutputTextAnnotationAdded` | Text annotation added |

### Error Events

| Event | Description |
|-------|-------------|
| `Error` | Stream error occurred |

## Configuration Options

### ResponseCreateParams

| Parameter | Type | Description |
|-----------|------|-------------|
| `model` | `String` | Model ID (required) |
| `input` | `Vec<ResponseInputItem>` | Input messages (required) |
| `max_output_tokens` | `i32` | Maximum response tokens |
| `temperature` | `f64` | Sampling temperature (0-2) |
| `top_p` | `f64` | Nucleus sampling |
| `presence_penalty` | `f64` | Presence penalty |
| `frequency_penalty` | `f64` | Frequency penalty |
| `stop` | `Vec<String>` | Stop sequences |
| `tools` | `Vec<Tool>` | Available tools |
| `tool_choice` | `ToolChoice` | Tool selection mode |
| `previous_response_id` | `String` | Continue conversation |
| `instructions` | `String` | System instructions |
| `thinking` | `ThinkingConfig` | Extended thinking |
| `reasoning` | `ReasoningConfig` | Reasoning options |
| `prompt_caching` | `PromptCachingConfig` | Caching config |
| `service_tier` | `ServiceTier` | Service tier |
| `truncation` | `Truncation` | Input truncation |
| `include` | `Vec<ResponseIncludable>` | Extra response data |
| `background` | `bool` | Background processing |
| `conversation` | `ConversationParam` | Conversation settings |

## License

MIT
