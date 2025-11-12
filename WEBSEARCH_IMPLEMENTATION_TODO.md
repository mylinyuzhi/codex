# WebSearch Implementation - Remaining Tasks

## ✅ Completed (85%)

All code has been written and compiles successfully:

- ✅ Provider architecture (DuckDuckGo, Tavily, OpenAI)
- ✅ WebSearchHandler implementation
- ✅ Configuration system (Config, Tools struct)
- ✅ Event system (WebSearchToolCallEvent)
- ✅ All compilation errors fixed
- ✅ Code formatted

## ❌ TODO: Critical Integration (15%)

### 1. Tool System Integration (CRITICAL)

**File**: `codex-rs/core/src/tools/spec.rs`

**Action**: Modify `build_specs()` function to add web search tool

**Location**: Around line 970-980 (after `view_image` tool registration)

**Code to add**:

```rust
// Add web_search tool if enabled
if config.tools_web_search_request {
    use crate::tools::web_search::{select_provider, WebSearchHandler};
    use std::sync::Arc;

    // Select provider based on config
    let provider = select_provider(
        config.web_search_config.provider,
        &config.model_family,
    ).await?;

    let max_results = config.web_search_config.max_results;

    // Generate tool spec based on provider type
    if provider.name() == "OpenAI" {
        // Use OpenAI native web_search tool type
        builder.push_spec(ToolSpec::WebSearch {});
    } else {
        // Use Function tool for custom providers
        let mut properties = BTreeMap::new();
        properties.insert(
            "query".to_string(),
            JsonSchema::String {
                description: Some("The search query to execute".to_string()),
            },
        );

        builder.push_spec(ToolSpec::Function(ResponsesApiTool {
            name: "web_search".to_string(),
            description: format!(
                "Search the web for current information using {}. \
                 Returns title, snippet, and URL for each result.",
                provider.name()
            ),
            strict: false,
            parameters: JsonSchema::Object {
                properties,
                required: Some(vec!["query".to_string()]),
                additional_properties: Some(false.into()),
            },
        }));

        // Register handler for custom providers
        let handler = Arc::new(WebSearchHandler::new(provider, max_results));
        registry.register("web_search", handler);
    }
}
```

**Important Notes**:
- OpenAI provider uses `ToolSpec::WebSearch {}` (no handler needed)
- Custom providers use `ToolSpec::Function` + handler registration
- The provider selection happens at startup, not per-request

### 2. Testing

**Basic Compilation Test**:
```bash
cd codex-rs
cargo check -p codex-core
cargo test -p codex-core --lib -- web_search
```

**Manual Testing**:
```bash
# 1. Update ~/.codex/config.toml
[tools]
web_search = true

[tools.web_search_config]
provider = "duckduckgo"  # or "tavily" or "openai"
max_results = 5

# 2. For Tavily provider:
export TAVILY_API_KEY="tvly-xxxxx"

# 3. Run codex
cargo run -p codex-cli
# Then test: "Search for Rust async programming best practices"
```

### 3. Documentation

**File**: `README.md` or `docs/tools/web-search.md`

**Content to add**:
```markdown
## Web Search Tool

The web search tool allows the agent to search the web for current information.

### Configuration

```toml
[tools]
web_search = true

[tools.web_search_config]
provider = "duckduckgo"  # "duckduckgo", "tavily", or "openai"
max_results = 5          # Number of results (1-20)
```

### Providers

- **DuckDuckGo** (default): Free, no API key required, uses HTML scraping
- **Tavily**: AI-optimized search, requires `TAVILY_API_KEY` environment variable
- **OpenAI**: Uses OpenAI's native web_search (GPT models only)

### Environment Variables

For Tavily provider:
```bash
export TAVILY_API_KEY="your-key-here"
```

Get free API key at: https://tavily.com/

### Examples

```bash
# Ask the agent to search
> Search for the latest Rust 1.75 release notes

# The agent will use web_search tool and provide current information
```
```

## Implementation Files Created

### Core Files (7 new files):
- `core/src/tools/web_search/mod.rs` - Module exports and provider selection
- `core/src/tools/web_search/provider.rs` - Provider trait
- `core/src/tools/web_search/duckduckgo_provider.rs` - DuckDuckGo implementation
- `core/src/tools/web_search/tavily_provider.rs` - Tavily implementation
- `core/src/tools/web_search/openai_provider.rs` - OpenAI marker
- `core/src/tools/handlers/web_search.rs` - Tool handler
- `WEBSEARCH_IMPLEMENTATION_TODO.md` - This file

### Modified Files (6 files):
- `protocol/src/config_types.rs` - Added WebSearchConfig types
- `protocol/src/protocol.rs` - Added WebSearchToolCallEvent
- `core/src/config/mod.rs` - Config parsing
- `core/src/tools/mod.rs` - Module export
- `core/src/tools/handlers/mod.rs` - Handler export
- `core/Cargo.toml` - Dependencies
- `app-server-protocol/src/protocol/v1.rs` - Tools struct
- `core/src/rollout/policy.rs` - Event handling

## Next Steps

1. **Complete integration** (step 1 above) - 30 minutes
2. **Test** (step 2) - 15 minutes
3. **Document** (step 3) - 15 minutes

**Total remaining time**: ~1 hour

## Design Decisions

### Why this architecture?

1. **Pluggable providers**: Easy to add new search backends
2. **Unified interface**: All providers use same WebSearchProvider trait
3. **Smart defaults**: Auto-selects best provider for each model
4. **Configuration flexibility**: Users can override defaults

### Provider Selection Logic

```
User sets provider in config:
  ├─ "duckduckgo" → Always use DuckDuckGo
  ├─ "tavily" → Use Tavily (requires API key)
  └─ "openai" → Try OpenAI, fallback to DuckDuckGo if incompatible
```

### Why separate OpenAI provider?

OpenAI's native `web_search` tool type is handled by OpenAI's API infrastructure:
- No local HTTP requests
- No result parsing
- Just a marker: `ToolSpec::WebSearch {}`

This is fundamentally different from custom providers that:
- Make HTTP requests locally
- Parse HTML/JSON responses
- Format results for LLM

## Troubleshooting

### Common Issues

**Issue**: `TAVILY_API_KEY` not found
```
Solution: export TAVILY_API_KEY="your-key"
```

**Issue**: OpenAI web_search not working
```
Check: Is this a GPT model? (gpt-4, gpt-3.5, o3, o4)
Fallback: Will automatically use DuckDuckGo
```

**Issue**: DuckDuckGo returns empty results
```
Possible causes:
- DuckDuckGo blocking (try different query)
- HTML structure changed (update selectors)
- Network issues
```

### Debug Mode

Add logging to see provider selection:
```rust
tracing::debug!("Selected web search provider: {}", provider.name());
```

## Future Enhancements

### Phase 2 (Not implemented yet):
- 🔜 Brave Search API provider
- 🔜 Result caching mechanism
- 🔜 Rate limiting protection
- 🔜 Quality filtering (remove spam/ads)
- 🔜 Multi-provider fallback chain
- 🔜 Result deduplication

### Architecture Benefits:
- ✅ New providers: Just implement WebSearchProvider trait
- ✅ No business logic changes needed
- ✅ Configuration-driven provider switching

## Questions?

Contact the implementer or refer to:
- Provider trait: `core/src/tools/web_search/provider.rs`
- Handler logic: `core/src/tools/handlers/web_search.rs`
- Configuration: `protocol/src/config_types.rs`
