# Multi-LLM Provider Plan

How coco-rs supports multiple LLM providers (Anthropic, OpenAI, Google, etc.) with per-model configuration, toolsets, and usage scenarios.

## Design Sources

- **TS (Claude Code)**: Model role mapping (main/fast/compact), provider detection (firstParty/bedrock/vertex/foundry), per-model capability checks, effort/thinking/fast mode
- **cocode-rs**: `LanguageModelV4` trait, `ModelRole` enum, `ModelInfo` per-model config, `ProviderFactory`, `ModelHub` caching, `RequestBuilder` pipeline, `apply_patch` tool type

Combined: TS defines **when/why** to use different models; cocode-rs defines **how** to abstract providers.

---

## 1. Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│  app/query (QueryEngine)                                │
│    uses ModelHub to get model for current role           │
├─────────────────────────────────────────────────────────┤
│  services/inference (coco-inference)                     │
│    ModelHub ── ProviderFactory ── RequestBuilder          │
│      │              │                  │                  │
│      │         ┌────┴────┐      5-step pipeline          │
│      │         │ Provider │     (normalize, cache,        │
│      │         │ Registry │      thinking, options,       │
│      │         └────┬────┘      interceptors)            │
│      │              │                                    │
│  ModelRoles    ┌────┴──────────────────────────┐         │
│  (per-role     │ vercel-ai LanguageModelV4     │         │
│   model        │  ├─ AnthropicProvider         │         │
│   selection)   │  ├─ OpenAIProvider            │         │
│                │  ├─ GoogleProvider             │         │
│                │  ├─ OpenAICompatibleProvider   │         │
│                │  └─ ByteDanceProvider          │         │
│                └───────────────────────────────┘         │
├─────────────────────────────────────────────────────────┤
│  common/config (coco-config)                             │
│    ModelInfo ── ModelRoles ── ProviderInfo                │
└─────────────────────────────────────────────────────────┘
```

---

## 2. Data Definitions

> **Source of truth**: Enum definitions (`ProviderApi`, `ModelRole`, `Capability`, `ApplyPatchToolType`, `WireApi`) are owned by `crate-coco-types.md`. Struct definitions (`ModelInfo`, `ProviderInfo`, `ModelRoles`) are owned by `crate-coco-config.md`. The code blocks below are for architectural context — if they conflict with the crate docs, the crate docs win.

### ModelRole (from cocode-rs, extended with TS patterns)

```rust
/// Which purpose a model serves. Each role can map to a different provider/model.
/// TS equivalent: getMainLoopModel(), getSmallFastModel(), etc. as implicit roles.
pub enum ModelRole {
    Main,       // Primary conversation (TS: getMainLoopModel())
    Fast,       // Quick/cheap operations (TS: getSmallFastModel() = Haiku)
    Compact,    // Context summarization (TS: uses main model)
    Plan,       // Planning/architecture (TS: opusplan alias)
    Explore,    // Codebase exploration (TS: subagent with inherit)
    Review,     // Code review (TS: subagent with inherit)
    HookAgent,  // Hook agent execution (TS: getSmallFastModel() default)
    Memory,     // Memory relevance ranking (TS: getDefaultSonnetModel())
}
```

### ModelSpec (resolved model identity)

```rust
/// A resolved model identity: which provider + which model ID.
pub struct ModelSpec {
    pub provider: ProviderApi,
    pub model_id: String,       // provider-specific model ID
    pub canonical_id: String,   // "claude-opus-4-6", "gpt-4o", etc.
}
```

### ProviderApi (from cocode-rs)

```rust
pub enum ProviderApi {
    Anthropic,        // Anthropic Claude (direct, Bedrock, Vertex, Foundry)
    Openai,           // OpenAI
    Gemini,           // Google Gemini
    Volcengine,       // Volcengine Ark
    Zai,              // Z.AI / ZhipuAI
    OpenaiCompat,     // Generic OpenAI-compatible endpoint
}
```

### ProviderInfo (runtime provider config)

```rust
pub struct ProviderInfo {
    pub name: String,
    pub api: ProviderApi,
    pub base_url: String,
    pub api_key: String,
    pub timeout_secs: i64,
    pub streaming: bool,
    pub wire_api: WireApi,      // Chat vs Responses (OpenAI)
    pub options: Option<Value>, // Provider-specific SDK settings
}

pub enum WireApi {
    Chat,       // Standard chat completions API
    Responses,  // OpenAI responses API (o1/o3, supports apply_patch)
}
```

### ModelInfo (per-model configuration, from cocode-rs)

```rust
/// Per-model configuration. Supports different capabilities per model.
/// TS equivalent: scattered across modelCapabilities.ts, thinking.ts, effort.ts
pub struct ModelInfo {
    // Identity
    pub slug: String,                          // "claude-opus-4-6"
    pub display_name: Option<String>,
    
    // Capacity (concrete types with defaults — see CLAUDE.md)
    pub context_window: i64,                   // default 200_000
    pub max_output_tokens: i64,                // default model-specific
    
    // Capabilities
    pub capabilities: HashSet<Capability>,
    
    // Thinking/Reasoning (multi-provider, from cocode-rs)
    pub default_thinking_level: Option<ThinkingLevel>,
    pub supported_thinking_levels: Option<Vec<ThinkingLevel>>,
    
    // Tools
    pub apply_patch_tool_type: ApplyPatchToolType,           // defaults to None variant
    pub excluded_tools: Vec<String>,                         // blacklist
    
    // Instructions (model-specific system prompt additions)
    pub base_instructions: Option<String>,
    pub base_instructions_file: Option<String>,
    
    // Sampling
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

pub enum Capability {
    ToolUse,
    Vision,
    Thinking,
    AdaptiveThinking,
    StructuredOutput,
    Effort,
    FastMode,
    PromptCaching,
    Streaming,
}

// ThinkingLevel struct and ReasoningEffort enum are defined in coco-types.
// See crate-coco-types.md for canonical definitions.
// See crate-coco-inference.md for thinking_convert (per-provider mapping).

pub enum ApplyPatchToolType {
    None,           // Anthropic: use FileEdit tool
    CustomToolCall, // OpenAI: apply_patch via custom tool_call
    BuiltIn,        // Future: native apply_patch support
}
```

### ModelRoles (role -> model mapping)

```rust
/// Maps each role to a specific model. Falls back to Main if role not configured.
/// TS equivalent: getMainLoopModel() for Main, getSmallFastModel() for Fast, etc.
pub struct ModelRoles {
    pub roles: HashMap<ModelRole, ModelSpec>,
}

impl ModelRoles {
    /// Get model for a role, falling back to Main.
    pub fn get(&self, role: ModelRole) -> &ModelSpec {
        self.roles.get(&role).unwrap_or_else(|| &self.roles[&ModelRole::Main])
    }
}
```

---

## 3. Model Usage Scenarios (TS patterns -> Rust abstraction)

### TS usage mapped to ModelRole

| TS usage | TS function | Rust ModelRole | Default model |
|----------|-------------|----------------|---------------|
| User conversation | `getMainLoopModel()` | `Main` | Opus 4.6 or Sonnet 4.6 |
| API key verify, title gen, shell prefix | `getSmallFastModel()` | `Fast` | Haiku 4.5 |
| Conversation compaction | uses main model | `Compact` | falls back to Main |
| Plan mode (opusplan) | alias resolution | `Plan` | Opus when in plan mode |
| Subagent (explore, review) | `getAgentModel()` | `Explore`/`Review` | inherit from Main |
| Hook agent | hook `model` field or small fast | `HookAgent` | Haiku 4.5 |
| Memory relevance ranking | `getDefaultSonnetModel()` | `Memory` | Sonnet |

### Subagent model inheritance

```rust
/// TS: getAgentModel(agentModel, parentModel, toolSpecifiedModel, permissionMode)
/// Priority: env override -> tool model -> agent model -> "inherit"
pub fn resolve_agent_model(
    agent_model: Option<&str>,
    parent_spec: &ModelSpec,
    tool_model: Option<&str>,
    roles: &ModelRoles,
) -> ModelSpec {
    // 1. CLAUDE_CODE_SUBAGENT_MODEL env override
    // 2. Tool-specified model (from skill frontmatter)
    // 3. Agent-specified model
    // 4. "inherit" -> clone parent_spec
}
```

---

## 4. Per-Model Tool Set

### TS finding: tools are NOT model-filtered

TS filters tools by **agent type**, not model:
- `ALL_AGENT_DISALLOWED_TOOLS` — never available to agents
- `ASYNC_AGENT_ALLOWED_TOOLS` — subset for background agents
- `IN_PROCESS_TEAMMATE_ALLOWED_TOOLS` — for teammates

### cocode-rs addition: per-model tool exclusion

cocode-rs `ModelInfo.excluded_tools` allows blacklisting tools per model. This is useful for:
- **OpenAI models**: exclude `FileEdit`, use `apply_patch` instead
- **Models without vision**: exclude image-related tool features
- **Minimal models**: exclude heavy tools (WebSearch, NotebookEdit)

### apply_patch — OpenAI-specific tool

```rust
/// apply_patch is OpenAI's custom tool for file editing.
/// It uses the Responses API (WireApi::Responses) with a special tool_call format.
/// Anthropic models use FileEdit (search-replace) instead.
pub struct ApplyPatchTool;

impl Tool for ApplyPatchTool {
    fn name(&self) -> &str { "apply_patch" }
    
    /// Only enabled for OpenAI providers
    fn is_enabled_for(&self, model_info: &ModelInfo) -> bool {
        model_info.apply_patch_tool_type == Some(ApplyPatchToolType::CustomToolCall)
    }
    
    async fn execute(&self, input: Value, ctx: &ToolUseContext, cancel: CancellationToken)
        -> Result<ToolResult<Value>, ToolError>
    {
        // Uses utils/apply-patch crate (from cocode-rs) to apply unified diff
        let patch = input["patch"].as_str().unwrap();
        coco_apply_patch::apply(patch, &ctx.cwd)?;
        Ok(ToolResult { data: json!({"success": true}), ..Default::default() })
    }
}
```

### Tool set assembly per model

```rust
/// Build tool definitions for a specific model.
/// TS: tools are agent-filtered, not model-filtered.
/// cocode-rs: adds model-level exclusion + provider-specific tools.
pub fn tools_for_model(
    registry: &ToolRegistry,
    model_info: &ModelInfo,
    agent_filter: Option<&AgentToolFilter>,
) -> Vec<ToolDefinition> {
    let mut tools: Vec<_> = registry.all()
        .filter(|t| !model_info.excluded_tools.contains(t.name()))
        .filter(|t| agent_filter.map_or(true, |f| f.allows(t.name())))
        .collect();
    
    // Add provider-specific tools
    if model_info.apply_patch_tool_type == Some(ApplyPatchToolType::CustomToolCall) {
        tools.push(ApplyPatchTool.definition());
    }
    
    tools.into_iter().map(|t| t.to_definition()).collect()
}
```

---

## 5. Per-Model System Prompt

### TS finding: system prompt is model-agnostic

TS uses the **same system prompt** for all models. No model-specific prompt branching.

### cocode-rs addition: `base_instructions`

cocode-rs `ModelInfo` has optional `base_instructions` — additional text prepended to the system prompt for specific models. Use cases:
- **OpenAI models**: instructions about apply_patch tool usage
- **Smaller models**: simplified instructions, fewer tool descriptions
- **Non-English models**: localized instructions

```rust
/// Build system prompt with optional model-specific additions.
pub fn build_system_prompt(
    context: &SystemContext,
    memory_files: &[MemoryFileInfo],
    model_info: &ModelInfo,
    tools: &[ToolDefinition],
) -> SystemPrompt {
    let mut blocks = vec![];
    
    // Model-specific base instructions (if configured)
    if let Some(ref instructions) = model_info.base_instructions {
        blocks.push(SystemPromptBlock::Text(instructions.clone()));
    }
    
    // Standard system prompt (same as TS, model-agnostic)
    blocks.extend(build_standard_prompt(context, memory_files, tools));
    
    SystemPrompt { blocks }
}
```

---

## 6. Provider-Specific Branching

### Beta Headers Matrix (from TS `utils/betas.ts` — 18 headers)

Source of truth for which beta features apply to which provider:

| Beta Header | firstParty | foundry | bedrock | vertex | Condition |
|-------------|-----------|---------|---------|--------|-----------|
| `claude-code-20250219` | Y | Y | Y | Y | Not Haiku |
| `context-1m` | Y | Y | Y | Y | 1M context models |
| `interleaved-thinking` | Y | Y | Opus4+/Sonnet4+ | Opus4+/Sonnet4+ | ISP support |
| `redact-thinking` | Y | Y | N | N | Thinking redaction |
| `context-management` | Y | Y | Y | Y | Tool clearing + thinking preservation |
| `structured-outputs` | Y | Y | N | N | Claude 4+ |
| `token-efficient-tools` | Y | Y | N | N | Ant-only |
| `prompt-caching-scope` | Y | N | N | N | Global cache scope (1P only) |
| `tool-search-1p` | Y | Y | N | N | Advanced tool-use |
| `tool-search-3p` | N | N | Y | Y | Tool search (3P variant) |
| `web-search` | Y | Y | N | Claude 4.0+ | Web search |
| `effort` | Y | Y | Y | Y | Effort parameter |
| `fast-mode` | Y | N | N | N | Fast mode (1P only) |

### Provider capability branching (from TS `utils/betas.ts` + `utils/thinking.ts`)

```rust
/// Provider-specific feature support.
/// From TS: modelSupportsISP(), modelSupportsStructuredOutputs(), etc.
pub fn provider_supports(provider: &ProviderApi, cap: Capability, model: &str) -> bool {
    match (provider, cap) {
        // Thinking: all providers for Claude 4+, but Bedrock/Vertex only Opus/Sonnet 4+
        (ProviderApi::Anthropic, Capability::Thinking) => is_claude_4_plus(model),
        
        // Structured outputs: firstParty/foundry only
        (ProviderApi::Openai, Capability::StructuredOutput) => true,  // OpenAI native
        (ProviderApi::Anthropic, Capability::StructuredOutput) => {
            // Only via firstParty/foundry, NOT bedrock/vertex
            !is_bedrock_or_vertex(provider)
        }
        
        // Prompt caching: Anthropic firstParty only (not foundry/bedrock/vertex)
        (ProviderApi::Anthropic, Capability::PromptCaching) => is_first_party(provider),
        
        // Fast mode: Anthropic firstParty only
        (ProviderApi::Anthropic, Capability::FastMode) => is_first_party(provider),
        
        // All providers support tool use and streaming
        (_, Capability::ToolUse) | (_, Capability::Streaming) => true,
        
        _ => false,
    }
}
```

### Capability checks at request time (TS pattern)

```rust
/// Check model capabilities at request time, not initialization.
/// TS: modelSupportsThinking(), modelSupportsEffort(), etc.
impl ModelInfo {
    pub fn supports(&self, cap: Capability) -> bool {
        self.capabilities.contains(&cap)
    }
    pub fn supports_thinking(&self) -> bool { self.supports(Capability::Thinking) }
    pub fn supports_effort(&self) -> bool { self.supports(Capability::Effort) }
    pub fn supports_fast_mode(&self) -> bool { self.supports(Capability::FastMode) }
    pub fn supports_vision(&self) -> bool { self.supports(Capability::Vision) }
    pub fn supports_structured_output(&self) -> bool { self.supports(Capability::StructuredOutput) }
}
```

### RequestBuilder pipeline (from cocode-rs, 5-step)

```rust
/// Build provider-specific request from model-agnostic InferenceContext.
/// From cocode-rs request_builder.rs — proven pattern.
pub fn build_request(ctx: &InferenceContext) -> LanguageModelV4CallOptions {
    let mut options = LanguageModelV4CallOptions::default();
    
    // Step 1: Normalize messages (empty content, tool ID sanitization)
    options.prompt = normalize_messages(&ctx.messages, &ctx.model_info);
    
    // Step 2: Prompt cache breakpoints (Anthropic-only)
    if ctx.provider_info.api == ProviderApi::Anthropic {
        add_cache_breakpoints(&mut options.prompt);
    }
    
    // Step 3: Provider base options
    options.max_output_tokens = ctx.model_info.max_output_tokens;
    options.temperature = ctx.model_info.temperature;
    
    // Step 4: Reasoning/thinking config -> provider options
    if ctx.model_info.supports_thinking() {
        options.reasoning = Some(ctx.thinking_level.into());
    }
    
    // Step 5: Provider-specific options (via provider_options pass-through)
    options.provider_options = build_provider_options(&ctx.provider_info, &ctx.model_info);
    
    options
}
```

---

## 7. ModelHub (from cocode-rs — caching + resolution)

```rust
/// Central model management. Caches providers and models.
/// From cocode-rs model_hub.rs.
pub struct ModelHub {
    config: Arc<Config>,
    providers: RwLock<HashMap<String, Arc<dyn ProviderV4>>>,
    models: RwLock<HashMap<ModelSpec, Arc<dyn LanguageModelV4>>>,
    factory: ProviderFactory,
}

impl ModelHub {
    /// Get or create a LanguageModelV4 for a spec.
    pub async fn get_model(&self, spec: &ModelSpec) -> Result<Arc<dyn LanguageModelV4>> {
        if let Some(model) = self.models.read().get(spec) {
            return Ok(model.clone());
        }
        let provider = self.get_or_create_provider(&spec.provider).await?;
        let model = provider.language_model(&spec.model_id)?;
        self.models.write().insert(spec.clone(), model.clone());
        Ok(model)
    }
    
    /// Resolve role to model spec using ModelRoles config.
    pub fn resolve_role(&self, role: ModelRole) -> ModelSpec {
        self.config.model_roles().get(role).clone()
    }
}
```

---

## 8. Configuration Example

```jsonc
// ~/.coco/config.json
{
  "providers": {
    "anthropic": {
      "api": "anthropic",
      "base_url": "https://api.anthropic.com",
      "api_key_env": "ANTHROPIC_API_KEY"
    },
    "openai": {
      "api": "openai",
      "base_url": "https://api.openai.com/v1",
      "api_key_env": "OPENAI_API_KEY",
      "wire_api": "responses"
    },
    "bedrock": {
      "api": "anthropic",
      "base_url": "https://bedrock-runtime.us-east-1.amazonaws.com"
    }
  },
  "models": {
    "claude-opus-4-6": {
      "provider": "anthropic",
      "context_window": 200000,
      "max_output_tokens": 128000,
      "capabilities": ["tool_use", "vision", "thinking", "adaptive_thinking", "effort", "fast_mode", "prompt_caching"],
      "default_thinking_level": "medium"
    },
    "claude-haiku-4-5": {
      "provider": "anthropic",
      "context_window": 200000,
      "max_output_tokens": 64000,
      "capabilities": ["tool_use", "vision", "thinking", "prompt_caching"]
    },
    "gpt-4o": {
      "provider": "openai",
      "context_window": 128000,
      "max_output_tokens": 16384,
      "capabilities": ["tool_use", "vision", "structured_output"],
      "apply_patch_tool_type": "custom_tool_call",
      "excluded_tools": ["FileEdit"],
      "base_instructions": "Use the apply_patch tool instead of FileEdit for file modifications."
    }
  },
  "model_roles": {
    "main": "claude-opus-4-6",
    "fast": "claude-haiku-4-5",
    "compact": null,
    "plan": "claude-opus-4-6",
    "hook_agent": "claude-haiku-4-5",
    "memory": "claude-sonnet-4-6"
  }
}
```

---

## 9. Crate Responsibility

| Component | Crate | What it does |
|-----------|-------|-------------|
| `LanguageModelV4` trait | `vercel-ai-provider` | Provider-agnostic model interface (cp from cocode-rs) |
| Provider implementations | `vercel-ai-anthropic`, `vercel-ai-openai`, `vercel-ai-google`, etc. | Per-provider SDK (cp from cocode-rs) |
| `ProviderApi`, `ModelInfo`, `ModelRole`, `Capability` | `common/types` (`coco-types`) | Shared type definitions |
| `ProviderInfo`, provider/model config loading | `common/config` (`coco-config`) | Config loading + model selection |
| `ModelHub`, `ProviderFactory`, `RequestBuilder` | `services/inference` (`coco-inference`) | Runtime model management + request building |
| `ApplyPatchTool` | `core/tools` (`coco-tools`) | OpenAI-specific tool (conditionally loaded) |
| `tools_for_model()` | `core/tool` (`coco-tool`) | Model-aware tool filtering |
| `ModelRoles` resolution | `app/query` (`coco-query`) | Role-based model selection at query time |

---

## 10. Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| **ModelRole enum, not ad-hoc functions** | TS uses scattered functions (getSmallFastModel, getMainLoopModel). Rust unifies into enum for type safety. |
| **ModelInfo per-model config** | TS checks capabilities at call time with `modelSupportsX()` functions. Rust pre-loads into ModelInfo struct. |
| **apply_patch as conditional tool** | OpenAI uses apply_patch instead of FileEdit. Loaded only when `apply_patch_tool_type == CustomToolCall`. |
| **base_instructions per model** | TS has model-agnostic prompts. Rust adds optional per-model instructions for provider-specific tools. |
| **ProviderFactory from cocode-rs** | Proven pattern for routing ProviderApi to implementation. |
| **ModelHub caching from cocode-rs** | Avoids re-creating providers/models per request. |
| **RequestBuilder 5-step pipeline** | Handles provider-specific quirks (cache breakpoints for Anthropic, reasoning_effort for OpenAI). |
| **Capability enum, not booleans** | Extensible set vs fixed fields. New capabilities don't require struct changes. |
| **WireApi for OpenAI** | OpenAI has two APIs (Chat Completions vs Responses). Responses supports apply_patch. |
