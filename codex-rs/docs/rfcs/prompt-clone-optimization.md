# RFC: 消除 Prompt Clone - RequestContext 重构方案

**状态**: Draft
**创建时间**: 2025-11-21
**作者**: Claude (基于用户需求分析)
**影响范围**: core/client.rs, core/adapters/, codex.rs

---

## 📋 目录

1. [背景和问题](#背景和问题)
2. [优化目标](#优化目标)
3. [技术决策](#技术决策)
4. [数据流分析](#数据流分析)
5. [最终方案](#最终方案)
6. [详细实施步骤](#详细实施步骤)
7. [验证清单](#验证清单)
8. [性能对比](#性能对比)
9. [存在不足](#存在不足)
10. [未来优化](#未来优化)

---

## 背景和问题

### 当前架构问题

在当前的 LLM 调用流程中，存在一个性能瓶颈：

```rust
// http.rs:99-105 (当前实现)
let mut enhanced_prompt = prompt.clone();  // ❌ 克隆整个 Prompt
enhanced_prompt.reasoning_effort = effort;
enhanced_prompt.reasoning_summary = Some(summary);

adapter.transform_request(&enhanced_prompt, provider)
```

**问题分析：**

1. **大对象克隆**：`Prompt` 包含 `input: Vec<ResponseItem>`（完整消息历史）
2. **克隆成本**：
   - 小对话（10条消息）：~5 KB
   - 中等对话（50条消息）：~25 KB
   - 大对话（200条消息）：~100 KB
   - 超大对话（1000条+）：~500 KB
3. **克隆原因**：需要注入 `reasoning_effort` 和 `reasoning_summary`，但 `transform_request` 接口是只读的（`&Prompt`）
4. **年度成本**（假设 1M 对话，平均 100 KB）：
   - 修改前：`1M × 100 KB × 3 turns = 300 GB` 内存分配

### 根本原因

**架构缺陷：** Prompt 混合了两种不同生命周期的数据：

| 数据类型 | 生命周期 | 变化频率 | 大小 |
|---------|---------|---------|------|
| **消息历史** (input) | Per-turn 累积 | 每 turn 增长 | 5-500 KB |
| **配置参数** (4 字段) | Per-turn 变化 | 可能每 turn 变化 | ~100 bytes |

**4 个配置字段：**
1. `reasoning_effort: Option<ReasoningEffortConfig>` - 推理强度
2. `reasoning_summary: Option<ReasoningSummaryConfig>` - 推理摘要配置
3. `previous_response_id: Option<String>` - 增量对话 ID
4. `effective_parameters: ModelParameters` - 采样参数（temperature, top_p 等）

---

## 优化目标

### 主要目标

1. ✅ **消除大对象克隆** - 只复制小配置对象（~100 bytes）
2. ✅ **数据职责分离** - Prompt 专注消息，RequestContext 负责配置
3. ✅ **最小 API 变更** - 尽量保持向后兼容
4. ✅ **语义清晰化** - 统一"请求上下文"概念

### 性能指标

- **克隆开销降低**：98-99.9%
- **年度节省**（1M 对话）：~300 GB → ~300 MB
- **单次 turn 延迟**：减少 0.1-1 ms（内存分配开销）

---

## 技术决策

### 决策 1：复用现有 RequestContext

**选项 A**：新建 `TransformContext` 结构体
**选项 B**：扩展现有 `RequestContext`（✅ 选择）

**理由：**
- RequestContext 已存在，用于传递运行时上下文（conversation_id, session_source）
- 扩展后语义更统一："请求上下文" = 运行时上下文 + 模型配置参数
- 减少新类型，降低认知负担
- `build_request_metadata` 已使用 RequestContext，保持一致

### 决策 2：从 Prompt 完全移除 4 个字段

**选项 A**：Prompt 保留字段，用 `Arc<RequestContext>` 优化
**选项 B**：完全移除字段，分离 Prompt 和 RequestContext（✅ 选择）

**理由：**
- 清晰的职责分离：Prompt（不可变消息） vs RequestContext（可变配置）
- 避免数据重复：同一配置不应同时存在于 Prompt 和 RequestContext
- 更符合 Rust 惯用法：不可变数据 + 显式上下文
- 为未来扩展铺路（如添加新配置参数）

### 决策 3：client.stream() 增加参数

**选项 A**：保持签名不变，内部从 self + prompt 组装
**选项 B**：增加 `previous_response_id` 参数（✅ 选择）

**理由：**
- `previous_response_id` 是 per-turn 动态状态，不属于 ModelClient
- `effective_parameters` 可以从 `self.resolve_parameters()` 获取
- 只增加 1 个参数，API 变更最小
- 数据流更清晰：SessionState → codex.rs → client.stream()

### 决策 4：stream_with_adapter 参数简化

**当前签名**：8 个参数
**优化后**：5 个参数（✅ 合并为 RequestContext）

**参数变化：**
```rust
// 删除这 6 个独立参数：
conversation_id: ConversationId,
session_source: SessionSource,
effort: Option<ReasoningEffortConfig>,
summary: ReasoningSummaryConfig,
// 以及从 prompt 读取的：
// previous_response_id, effective_parameters

// 替换为 1 个参数：
context: RequestContext  // 包含全部 6 个字段
```

---

## 数据流分析

### 当前数据流（有问题）

```
┌──────────────────────────────────────────────────────────┐
│ codex.rs:1959 - 构造 Prompt（包含 4 字段）                │
└──────────────────────────────────────────────────────────┘
  ├─ previous_response_id = sess.state.get_last_response()
  ├─ effective_parameters = client.resolve_parameters()
  └─ prompt = Prompt {
        input,
        tools,
        previous_response_id,     // ❌ 大对象中嵌入小字段
        effective_parameters,     // ❌
        reasoning_effort: None,   // ❌ 默认值，后续克隆注入
        reasoning_summary: None,  // ❌
      }
  ↓
┌──────────────────────────────────────────────────────────┐
│ client.rs:180 - client.stream(&prompt)                   │
└──────────────────────────────────────────────────────────┘
  ├─ 从 self 读取：effort, summary
  └─ 从 prompt 读取：previous_response_id, effective_parameters
  ↓
┌──────────────────────────────────────────────────────────┐
│ http.rs:99 - stream_with_adapter(prompt, ..., effort, ...)│
└──────────────────────────────────────────────────────────┘
  ├─ let mut enhanced_prompt = prompt.clone();  // ❌ 克隆 Vec<ResponseItem>
  ├─ enhanced_prompt.reasoning_effort = effort;
  └─ enhanced_prompt.reasoning_summary = Some(summary);
  ↓
┌──────────────────────────────────────────────────────────┐
│ adapter.transform_request(&enhanced_prompt, provider)    │
└──────────────────────────────────────────────────────────┘
  读取所有 4 字段
```

**问题点：**
1. Prompt 包含不必要的字段（reasoning_effort/summary 默认为 None）
2. 需要 clone 整个 Prompt 才能注入这 2 个字段
3. 数据流混乱：4 字段来自不同来源，但全部塞进 Prompt

### 优化后数据流

```
┌──────────────────────────────────────────────────────────┐
│ codex.rs:1959 - 分离构造 Prompt 和 previous_response_id  │
└──────────────────────────────────────────────────────────┘
  ├─ previous_response_id = sess.state.get_last_response()
  └─ prompt = Prompt {
        input,
        tools,
        parallel_tool_calls,
        base_instructions_override,
        output_schema,
      }  // ✅ 不含 4 字段，专注消息
  ↓
┌──────────────────────────────────────────────────────────┐
│ client.rs:180 - client.stream(&prompt, previous_response_id)│
└──────────────────────────────────────────────────────────┘
  ✅ 构造完整 RequestContext（6 字段）：
  ├─ conversation_id: self.conversation_id
  ├─ session_source: self.session_source
  ├─ reasoning_effort: self.effort              // 从 ModelClient
  ├─ reasoning_summary: Some(self.summary)      // 从 ModelClient
  ├─ effective_parameters: self.resolve_parameters()  // 内部调用
  └─ previous_response_id                       // 从参数
  ↓
┌──────────────────────────────────────────────────────────┐
│ http.rs - stream_with_adapter(prompt, context, ...)     │
└──────────────────────────────────────────────────────────┘
  ✅ 直接传递，无需 clone：
  ├─ adapter.transform_request(prompt, &context, provider)
  └─ adapter.build_request_metadata(prompt, &context, provider)
  ↓
┌──────────────────────────────────────────────────────────┐
│ adapter - 从 context 读取所有配置                        │
└──────────────────────────────────────────────────────────┘
  context.reasoning_effort
  context.reasoning_summary
  context.previous_response_id
  context.effective_parameters
  context.conversation_id        // 运行时上下文
  context.session_source         // 运行时上下文
```

**改进点：**
1. ✅ Prompt 专注消息历史，体积不变但不含冗余字段
2. ✅ RequestContext 集中管理所有配置参数
3. ✅ 零 clone - 只复制 6 个小字段（~100 bytes）
4. ✅ 数据流清晰：每个字段来源明确

### 4 个字段的数据来源

| 字段 | 来源 | 获取位置 | 生命周期 |
|------|------|---------|---------|
| `reasoning_effort` | Config → ModelClient | client.rs (self.effort) | Per-session |
| `reasoning_summary` | Config → ModelClient | client.rs (self.summary) | Per-session |
| `previous_response_id` | SessionState | codex.rs → client.stream() | Per-turn |
| `effective_parameters` | Config + Provider | client.rs (self.resolve_parameters()) | Per-turn |
| `conversation_id` | ModelClient | client.rs (self.conversation_id) | Per-session |
| `session_source` | ModelClient | client.rs (self.session_source) | Per-session |

**关键发现：**
- `previous_response_id` 是唯一需要从外部传入的（每 turn 变化）
- 其他 5 个字段都可以从 `ModelClient` 内部获取
- 因此 `client.stream()` 只需增加 1 个参数

---

## 最终方案

### 架构概览

```
┌─────────────────────────────────────────────────────────┐
│ RequestContext（扩展后）                                 │
├─────────────────────────────────────────────────────────┤
│ 运行时上下文（现有）                                      │
│ ├─ conversation_id: String                              │
│ └─ session_source: String                               │
│                                                          │
│ 模型配置参数（新增）                                      │
│ ├─ reasoning_effort: Option<ReasoningEffortConfig>     │
│ ├─ reasoning_summary: Option<ReasoningSummaryConfig>   │
│ ├─ previous_response_id: Option<String>                │
│ └─ effective_parameters: ModelParameters               │
└─────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────┐
│ Prompt（精简后）                                         │
├─────────────────────────────────────────────────────────┤
│ 核心消息数据                                             │
│ ├─ input: Vec<ResponseItem>            // 消息历史      │
│ ├─ tools: Vec<ToolSpec>                // 可用工具      │
│ ├─ parallel_tool_calls: bool           // 并行调用      │
│ ├─ base_instructions_override: Option<String>          │
│ └─ output_schema: Option<Value>        // 结构化输出    │
│                                                          │
│ ❌ 删除的字段：                                          │
│ - reasoning_effort                                      │
│ - reasoning_summary                                     │
│ - previous_response_id                                  │
│ - effective_parameters                                  │
└─────────────────────────────────────────────────────────┘
```

### 接口变更

#### 1. RequestContext 扩展

```rust
#[derive(Debug, Clone)]
pub struct RequestContext {
    // ===== 现有字段 =====
    pub conversation_id: String,
    pub session_source: String,

    // ===== 新增字段 =====
    pub reasoning_effort: Option<ReasoningEffortConfig>,
    pub reasoning_summary: Option<ReasoningSummaryConfig>,
    pub previous_response_id: Option<String>,
    pub effective_parameters: ModelParameters,
}
```

#### 2. Prompt 精简

```rust
pub struct Prompt {
    pub input: Vec<ResponseItem>,
    pub tools: Vec<ToolSpec>,
    pub parallel_tool_calls: bool,
    pub base_instructions_override: Option<String>,
    pub output_schema: Option<Value>,

    // ❌ 删除：reasoning_effort, reasoning_summary,
    //         previous_response_id, effective_parameters
}
```

#### 3. client.stream() 签名

```rust
// 修改前
pub async fn stream(&self, prompt: &Prompt) -> Result<ResponseStream>

// 修改后（+1 参数）
pub async fn stream(
    &self,
    prompt: &Prompt,
    previous_response_id: Option<String>,
) -> Result<ResponseStream>
```

#### 4. stream_with_adapter 签名

```rust
// 修改前（8 参数）
pub async fn stream_with_adapter(
    &self,
    prompt: &Prompt,
    provider: &ModelProviderInfo,
    adapter_name: &str,
    conversation_id: ConversationId,
    session_source: SessionSource,
    effort: Option<ReasoningEffortConfig>,
    summary: ReasoningSummaryConfig,
    global_stream_idle_timeout: Option<u64>,
) -> Result<ResponseStream>

// 修改后（5 参数）
pub async fn stream_with_adapter(
    &self,
    prompt: &Prompt,
    context: RequestContext,  // 替代 6 个参数
    provider: &ModelProviderInfo,
    adapter_name: &str,
    global_stream_idle_timeout: Option<u64>,
) -> Result<ResponseStream>
```

#### 5. ProviderAdapter trait

```rust
pub trait ProviderAdapter: Send + Sync {
    // 修改前
    fn transform_request(
        &self,
        prompt: &Prompt,
        provider: &ModelProviderInfo,
    ) -> Result<JsonValue>;

    // 修改后（+1 参数）
    fn transform_request(
        &self,
        prompt: &Prompt,
        context: &RequestContext,
        provider: &ModelProviderInfo,
    ) -> Result<JsonValue>;

    // build_request_metadata 统一命名
    fn build_request_metadata(
        &self,
        prompt: &Prompt,
        context: &RequestContext,  // 之前叫 runtime_context
        provider: &ModelProviderInfo,
    ) -> Result<RequestMetadata> {
        Ok(RequestMetadata::default())
    }
}
```

---

## 详细实施步骤

### Phase 1: 基础结构修改

#### Step 1.1: 扩展 RequestContext

**文件**: `codex-rs/core/src/adapters/mod.rs`（或 `request_context.rs`）
**位置**: 如果是独立文件，在 `adapters/` 目录下

```rust
use codex_protocol::config_types::ModelParameters;
use codex_protocol::config_types::ReasoningEffort as ReasoningEffortConfig;
use codex_protocol::config_types::ReasoningSummary as ReasoningSummaryConfig;

#[derive(Debug, Clone)]
pub struct RequestContext {
    // ===== 现有字段（运行时上下文）=====
    /// Unique identifier for the current conversation
    ///
    /// Used for:
    /// - Request tracking across multiple API calls
    /// - Log correlation in enterprise LLM gateways
    /// - Session identification
    pub conversation_id: String,

    /// Source/origin of the session
    ///
    /// Possible values: "Cli", "VSCode", "Exec", "Mcp", "SubAgent", "Unknown"
    ///
    /// Used for:
    /// - Telemetry headers (x-openai-subagent)
    /// - Source-specific request handling
    /// - Debug/audit trails
    pub session_source: String,

    // ===== 新增字段（模型配置参数）=====
    /// Reasoning effort level for models that support extended thinking
    ///
    /// Source: ModelClient.effort (from Config.model_reasoning_effort)
    /// Lifecycle: Per-session (stable across turns)
    ///
    /// Values: None | Low | Medium | High
    pub reasoning_effort: Option<ReasoningEffortConfig>,

    /// Reasoning summary configuration
    ///
    /// Source: ModelClient.summary (from Config.model_reasoning_summary)
    /// Lifecycle: Per-session (stable across turns)
    ///
    /// Controls how reasoning content is presented (Detailed vs Concise)
    pub reasoning_summary: Option<ReasoningSummaryConfig>,

    /// Previous response ID for conversation continuity
    ///
    /// Source: SessionState.get_last_response() (updated each turn)
    /// Lifecycle: Per-turn (dynamic)
    ///
    /// Used for incremental conversation mode to reduce payload size
    pub previous_response_id: Option<String>,

    /// Resolved model sampling parameters
    ///
    /// Source: ModelClient.resolve_parameters()
    ///         (merged from Config + ModelProviderInfo overrides)
    /// Lifecycle: Per-turn (may change if provider config changes)
    ///
    /// Contains: temperature, top_p, frequency_penalty,
    ///           presence_penalty, max_tokens
    pub effective_parameters: ModelParameters,
}
```

**模块注册**（如果是独立文件）:
```rust
// In adapters/mod.rs
mod request_context;
pub use request_context::RequestContext;
```

#### Step 1.2: 精简 Prompt 结构

**文件**: `codex-rs/core/src/client_common.rs`
**位置**: line 31-66

**删除字段**:
```rust
pub struct Prompt {
    pub input: Vec<ResponseItem>,
    pub tools: Vec<ToolSpec>,
    pub parallel_tool_calls: bool,
    pub base_instructions_override: Option<String>,
    pub output_schema: Option<Value>,

    // ❌ 删除以下 4 个字段：
    // pub reasoning_effort: Option<ReasoningEffortConfig>,
    // pub reasoning_summary: Option<ReasoningSummaryConfig>,
    // pub previous_response_id: Option<String>,
    // pub effective_parameters: ModelParameters,
}
```

**更新 Default 实现**:
```rust
impl Default for Prompt {
    fn default() -> Self {
        Self {
            input: Vec::new(),
            tools: Vec::new(),
            parallel_tool_calls: false,
            base_instructions_override: None,
            output_schema: None,
            // ❌ 删除：
            // reasoning_effort: None,
            // reasoning_summary: None,
            // previous_response_id: None,
            // effective_parameters: Default::default(),
        }
    }
}
```

### Phase 2: 更新 Trait 和接口

#### Step 2.1: 更新 ProviderAdapter trait

**文件**: `codex-rs/core/src/adapters/mod.rs`
**位置**: line 431-460

```rust
pub trait ProviderAdapter: Send + Sync {
    /// Transform prompt and context into provider-specific request format
    ///
    /// # Parameters
    /// - `prompt`: Message history and tool specs
    /// - `context`: Request configuration (reasoning, parameters, etc.)
    /// - `provider`: Provider-specific settings
    fn transform_request(
        &self,
        prompt: &Prompt,
        context: &RequestContext,  // ✅ 新增参数
        provider: &ModelProviderInfo,
    ) -> Result<JsonValue>;

    /// Build dynamic request metadata (headers, query params)
    ///
    /// # Parameters
    /// - `prompt`: Message history (rarely used by this method)
    /// - `context`: Full request context (conversation_id, session_source, etc.)
    /// - `provider`: Provider-specific settings
    fn build_request_metadata(
        &self,
        prompt: &Prompt,
        context: &RequestContext,  // ✅ 统一命名（之前叫 runtime_context）
        provider: &ModelProviderInfo,
    ) -> Result<RequestMetadata> {
        // Default: no extra headers/params
        let _ = (prompt, context, provider);
        Ok(RequestMetadata::default())
    }

    // ... 其他方法保持不变
}
```

#### Step 2.2: 更新 client.stream()

**文件**: `codex-rs/core/src/client.rs`
**位置**: line 180-240

**修改签名**:
```rust
pub async fn stream(
    &self,
    prompt: &Prompt,
    previous_response_id: Option<String>,  // ✅ 新增参数
) -> Result<ResponseStream>
```

**修改函数体**（adapter 分支）:
```rust
pub async fn stream(
    &self,
    prompt: &Prompt,
    previous_response_id: Option<String>,
) -> Result<ResponseStream> {
    // Check if provider specifies a custom adapter
    if let Some(adapter_name) = &self.provider.adapter {
        // 🔑 核心改动：在这里构造完整 RequestContext
        let context = RequestContext {
            // 运行时上下文（从 self）
            conversation_id: self.conversation_id.to_string(),
            session_source: format!("{:?}", self.session_source),

            // 模型配置参数（从 self）
            reasoning_effort: self.effort,
            reasoning_summary: Some(self.summary),
            effective_parameters: self.resolve_parameters(),  // 内部调用

            // 会话状态（从参数）
            previous_response_id,
        };

        let adapter_client = AdapterHttpClient::new(
            self.client.clone(),
            self.otel_event_manager.clone(),
        );

        return adapter_client
            .stream_with_adapter(
                prompt,
                context,  // ✅ 单个参数替代 6 个独立参数
                &self.provider,
                adapter_name,
                self.config.stream_idle_timeout_ms,
            )
            .await;
    }

    // Fallback to existing wire_api routing (backward compatible)
    match self.provider.wire_api {
        WireApi::Responses => self.stream_responses(prompt, previous_response_id).await,
        WireApi::Chat => {
            // Chat completions API 可能也需要 previous_response_id
            // 当前实现中未使用，保持不变
            // ...
        }
    }
}
```

**修改 stream_responses**（fallback 路径）:
```rust
async fn stream_responses(
    &self,
    prompt: &Prompt,
    previous_response_id: Option<String>,  // ✅ 新增参数
) -> Result<ResponseStream> {
    // ... 现有逻辑

    // 注意：当前 ResponsesApiRequest 没有直接使用 previous_response_id
    // 如果未来需要支持，在这里添加

    // ... rest of the function
}
```

### Phase 3: 更新 Adapter 实现

#### Step 3.1: 更新 stream_with_adapter

**文件**: `codex-rs/core/src/adapters/http.rs`
**位置**: line 69-136

**修改签名**（参数从 8 个减少到 5 个）:
```rust
pub async fn stream_with_adapter(
    &self,
    prompt: &Prompt,
    context: RequestContext,  // ✅ 替代 6 个独立参数
    provider: &ModelProviderInfo,
    adapter_name: &str,
    global_stream_idle_timeout: Option<u64>,
) -> Result<ResponseStream>
```

**修改函数体**:

**删除这段 clone 逻辑**（line 98-101）:
```rust
// ❌ DELETE: Clone prompt and inject reasoning configuration
let mut enhanced_prompt = prompt.clone();
enhanced_prompt.reasoning_effort = effort;
enhanced_prompt.reasoning_summary = Some(summary);
```

**修改 transform_request 调用**（line 103-110）:
```rust
// ✅ NEW: Use context directly, no clone needed
let transformed_request = adapter
    .transform_request(prompt, &context, provider)  // 传递 context
    .map_err(|e| {
        CodexErr::Fatal(format!(
            "Adapter '{adapter_name}' failed to transform request: {e}"
        ))
    })?;
```

**删除独立构造 request_context**（line 112-116）:
```rust
// ❌ DELETE: Build runtime context for dynamic headers/params
let request_context = crate::adapters::RequestContext {
    conversation_id: conversation_id.to_string(),
    session_source: format!("{session_source:?}"),
};
```

**修改 build_request_metadata 调用**（line 119-125）:
```rust
// ✅ NEW: Reuse the same context
let request_metadata = adapter
    .build_request_metadata(prompt, &context, provider)  // 复用 context
    .map_err(|e| {
        CodexErr::Fatal(format!(
            "Adapter '{adapter_name}' failed to build request metadata: {e}"
        ))
    })?;
```

#### Step 3.2: 更新 GptOpenapiAdapter

**文件**: `codex-rs/core/src/adapters/gpt_openapi.rs`
**位置**: line 427-470

**修改 transform_request 签名**:
```rust
fn transform_request(
    &self,
    prompt: &Prompt,
    context: &RequestContext,  // ✅ 新增参数
    provider: &ModelProviderInfo,
) -> Result<JsonValue>
```

**全局替换字段读取**（在 transform_request 函数内）:

**Step 1**: 找到所有 `prompt.effective_parameters` 读取:
```rust
// ❌ 修改前
if let Some(temp) = prompt.effective_parameters.temperature {
    params.insert("temperature", json!(temp));
}
if let Some(top_p) = prompt.effective_parameters.top_p {
    params.insert("top_p", json!(top_p));
}
if let Some(freq) = prompt.effective_parameters.frequency_penalty {
    params.insert("frequency_penalty", json!(freq));
}
if let Some(pres) = prompt.effective_parameters.presence_penalty {
    params.insert("presence_penalty", json!(pres));
}
if let Some(max_tok) = prompt.effective_parameters.max_tokens {
    params.insert("max_tokens", json!(max_tok));
}

// ✅ 修改后
if let Some(temp) = context.effective_parameters.temperature {
    params.insert("temperature", json!(temp));
}
if let Some(top_p) = context.effective_parameters.top_p {
    params.insert("top_p", json!(top_p));
}
if let Some(freq) = context.effective_parameters.frequency_penalty {
    params.insert("frequency_penalty", json!(freq));
}
if let Some(pres) = context.effective_parameters.presence_penalty {
    params.insert("presence_penalty", json!(pres));
}
if let Some(max_tok) = context.effective_parameters.max_tokens {
    params.insert("max_tokens", json!(max_tok));
}
```

**Step 2**: 找到 `prompt.previous_response_id` 读取:
```rust
// ❌ 修改前（line 459-460）
if let Some(resp_id) = &prompt.previous_response_id {
    params.insert("previous_response_id", json!(resp_id));
}

// ✅ 修改后
if let Some(resp_id) = &context.previous_response_id {
    params.insert("previous_response_id", json!(resp_id));
}
```

**Step 3**: 找到 `prompt.reasoning_effort` 和 `prompt.reasoning_summary` 读取:
```rust
// ❌ 修改前（line 464-467）
if let Some(effort) = prompt.reasoning_effort {
    // ... reasoning 配置逻辑
}
if let Some(summary) = prompt.reasoning_summary {
    // ...
}

// ✅ 修改后
if let Some(effort) = context.reasoning_effort {
    // ... reasoning 配置逻辑
}
if let Some(summary) = context.reasoning_summary {
    // ...
}
```

**辅助：全局搜索命令**:
```bash
# 在 gpt_openapi.rs 中查找所有需要替换的位置
rg "prompt\.(effective_parameters|previous_response_id|reasoning_effort|reasoning_summary)" \
   codex-rs/core/src/adapters/gpt_openapi.rs --line-number
```

### Phase 4: 更新调用方

#### Step 4.1: 更新 codex.rs 构造 Prompt

**文件**: `codex-rs/core/src/codex.rs`
**位置**: line 1939-1968

**修改前**:
```rust
let previous_response_id = sess
    .state
    .lock()
    .await
    .get_last_response()
    .map(|id| id.to_string());

let effective_parameters = turn_context.client.resolve_parameters();

let prompt = Prompt {
    input,
    tools: router.specs(),
    parallel_tool_calls,
    base_instructions_override: turn_context.base_instructions.clone(),
    output_schema: turn_context.final_output_json_schema.clone(),
    effective_parameters,        // ❌ 删除
    previous_response_id,        // ❌ 删除
    reasoning_effort: None,      // ❌ 删除
    reasoning_summary: None,     // ❌ 删除
};
```

**修改后**:
```rust
// 提取 previous_response_id（需要传递给 client.stream）
let previous_response_id = sess
    .state
    .lock()
    .await
    .get_last_response()
    .map(|id| id.to_string());

// 构造 Prompt（精简版，不含 4 字段）
let prompt = Prompt {
    input,
    tools: router.specs(),
    parallel_tool_calls,
    base_instructions_override: turn_context.base_instructions.clone(),
    output_schema: turn_context.final_output_json_schema.clone(),
    // ✅ 不再包含 4 个字段
};
```

#### Step 4.2: 更新 client.stream() 调用

**搜索所有调用点**:
```bash
rg "\.stream\(&prompt\)" codex-rs/core/src --type rust --line-number
rg "client\.stream\(" codex-rs/core/src --type rust -A 2
```

**典型调用位置**（需要根据实际搜索结果确认）:

可能在 `try_run_turn` 或直接在 `run_turn` 函数中：

**修改前**:
```rust
let response_stream = turn_context.client.stream(&prompt).await?;
```

**修改后**:
```rust
let response_stream = turn_context
    .client
    .stream(&prompt, previous_response_id)
    .await?;
```

**注意事项**:
- `previous_response_id` 变量需要在调用前定义
- 如果在不同作用域，可能需要 clone 或重新获取
- 确保所有调用点都更新（包括测试文件）

### Phase 5: 更新测试代码

#### Step 5.1: 更新单元测试中的 Prompt 构造

**影响文件**: `codex-rs/core/tests/**/*.rs`

**搜索命令**:
```bash
rg "Prompt \{" codex-rs/core/tests --type rust -A 10
rg "Prompt::default" codex-rs/core/tests --type rust
```

**修改示例**:
```rust
// ❌ 修改前
let prompt = Prompt {
    input: vec![],
    tools: vec![],
    parallel_tool_calls: false,
    base_instructions_override: None,
    output_schema: None,
    reasoning_effort: None,
    reasoning_summary: None,
    previous_response_id: None,
    effective_parameters: Default::default(),
};

// ✅ 修改后
let prompt = Prompt {
    input: vec![],
    tools: vec![],
    parallel_tool_calls: false,
    base_instructions_override: None,
    output_schema: None,
    // 删除 4 个字段
};
```

#### Step 5.2: 更新测试中的 client.stream() 调用

```rust
// ❌ 修改前
let stream = client.stream(&prompt).await?;

// ✅ 修改后
let stream = client.stream(&prompt, None).await?;  // 测试中通常不需要 previous_response_id
```

#### Step 5.3: 更新 RequestContext mock

**如果测试直接构造 RequestContext**:
```rust
let context = RequestContext {
    conversation_id: "test-123".to_string(),
    session_source: "Cli".to_string(),
    // ✅ 新增字段
    reasoning_effort: None,
    reasoning_summary: None,
    previous_response_id: None,
    effective_parameters: Default::default(),
};
```

**或者提供 helper 函数**:
```rust
// In test utilities
impl RequestContext {
    pub fn test_default() -> Self {
        Self {
            conversation_id: "test-conv-id".to_string(),
            session_source: "Cli".to_string(),
            reasoning_effort: None,
            reasoning_summary: None,
            previous_response_id: None,
            effective_parameters: Default::default(),
        }
    }
}
```

---

## 验证清单

### 编译验证

```bash
# Step 1: 清理构建缓存
cargo clean -p codex-core

# Step 2: 检查编译错误
cargo check -p codex-core 2>&1 | tee /tmp/check-errors.txt

# Step 3: 完整构建（检查所有依赖）
cargo build 2>&1 | tee /tmp/build-errors.txt

# Step 4: 检查是否有遗漏的字段读取
rg "prompt\.(reasoning_effort|reasoning_summary|previous_response_id|effective_parameters)" \
   codex-rs/core/src --type rust
```

### 测试验证

```bash
# Step 1: 单元测试
cargo test -p codex-core --lib 2>&1 | tee /tmp/unit-tests.txt

# Step 2: 集成测试
cargo test -p codex-core --test '*' 2>&1 | tee /tmp/integration-tests.txt

# Step 3: Adapter 相关测试（重点）
cargo test -p codex-core adapter 2>&1 | tee /tmp/adapter-tests.txt

# Step 4: 所有测试
cargo test --all-features 2>&1 | tee /tmp/all-tests.txt
```

### 代码质量检查

```bash
# Step 1: Clippy 检查
just clippy 2>&1 | tee /tmp/clippy.txt

# Step 2: 格式检查
just fmt

# Step 3: 检查是否有 unwrap (可能新增)
rg "\.unwrap\(\)" codex-rs/core/src --type rust

# Step 4: 检查是否有未处理的 TODO
rg "TODO|FIXME" codex-rs/core/src --type rust
```

### 手动验证

#### 1. 检查所有 client.stream() 调用

```bash
# 查找所有调用点
rg "\.stream\(&prompt" codex-rs/core/src --type rust -B 2 -A 2

# 预期：所有调用都应该有 previous_response_id 参数
# stream(&prompt, previous_response_id)
```

#### 2. 检查所有 Prompt 构造

```bash
# 查找所有构造点
rg "Prompt \{" codex-rs/core/src --type rust -A 10

# 预期：不应该包含 4 个删除的字段
# 不应该有：reasoning_effort, reasoning_summary,
#          previous_response_id, effective_parameters
```

#### 3. 检查所有字段读取

```bash
# 在 adapter 实现中，应该从 context 读取
rg "context\.(reasoning_effort|reasoning_summary|previous_response_id|effective_parameters)" \
   codex-rs/core/src/adapters --type rust

# 不应该从 prompt 读取（应该为空）
rg "prompt\.(reasoning_effort|reasoning_summary|previous_response_id|effective_parameters)" \
   codex-rs/core/src --type rust
```

### 功能验证

#### 1. 运行 codex CLI

```bash
# 基本对话测试
just codex "echo hello"

# Adapter 路径测试（如果配置了自定义 adapter）
just codex "test adapter functionality"

# 长对话测试（验证性能改进）
# 创建一个包含多轮对话的测试脚本
```

#### 2. 检查日志

```bash
# 启用详细日志
RUST_LOG=debug just codex "test" 2>&1 | grep -i "clone\|prompt\|context"

# 预期：不应该看到 "cloning prompt" 相关日志
```

#### 3. 性能基准测试（可选）

```bash
# 对比修改前后的内存分配
# 使用工具如 valgrind, heaptrack 等
# 或者简单的时间测量

time just codex "run a long conversation"
```

---

## 性能对比

### 克隆成本分析

#### 修改前（Clone 整个 Prompt）

| 对话规模 | 消息数量 | Prompt 大小 | Clone 成本 |
|---------|---------|------------|-----------|
| 小对话 | 10 条 | ~5 KB | 5 KB |
| 中对话 | 50 条 | ~25 KB | 25 KB |
| 大对话 | 200 条 | ~100 KB | 100 KB |
| 超大对话 | 1000 条 | ~500 KB | 500 KB |

**每次 turn 的开销**: 5-500 KB
**典型对话 (3 turns, 50 条消息)**: 25 KB × 3 = 75 KB

#### 修改后（Clone 4 个字段）

| 字段 | 类型 | 大小估算 |
|------|------|---------|
| `previous_response_id` | `Option<String>` | ~50 bytes |
| `effective_parameters` | `ModelParameters` | ~40 bytes (5 个 Option) |
| `reasoning_effort` | `Option<Enum>` | ~2 bytes |
| `reasoning_summary` | `Option<Enum>` | ~2 bytes |
| **总计** | - | **~100 bytes** |

**每次 turn 的开销**: ~100 bytes（不随对话增长）
**典型对话 (3 turns)**: 100 bytes × 3 = 300 bytes

---

## 存在不足

### 1. API 破坏性变更

**问题**: `client.stream()` 签名变更

**影响范围**:
- 所有直接调用 `ModelClient::stream()` 的代码
- 测试代码中的 mock 调用
- 可能的外部依赖（如果 ModelClient 是公开 API）

**缓解措施**:
- 这是内部 API，无公开用户
- 编译期强制更新所有调用点
- 提供清晰的迁移文档（本 RFC）

### 2. 多处调用点需要更新

**问题**: `client.stream()` 可能在多处被调用

**搜索命令确认影响**:
```bash
rg "\.stream\(&prompt\)" codex-rs --type rust
```

**缓解措施**:
- 编译器会捕获所有遗漏的调用点
- 逐一检查每个调用，确保 `previous_response_id` 正确传递
- 自动化测试覆盖主要路径

### 3. 测试代码修改量大

**问题**: 所有测试中的 Prompt 构造需要更新

**影响**:
- 单元测试：~20-30 处
- 集成测试：~10-15 处
- Mock 代码：~5-10 处

**缓解措施**:
- 提供 `Prompt::default()` 简化测试构造
- 提供 `RequestContext::test_default()` helper
- 逐个文件修复，确保测试通过

### 4. Fallback 路径处理不完整

**问题**: `WireApi::Responses` 和 `WireApi::Chat` 路径可能需要 `previous_response_id`

**当前状态**:
- `stream_responses` 接收参数但未使用
- `stream_chat_completions` 不支持 incremental mode

**后续 TODO**:
- 评估 fallback 路径是否需要 `previous_response_id`
- 如果需要，更新 `ResponsesApiRequest` 结构
- 添加相应的测试覆盖

### 5. RequestContext 字段语义混合

**问题**: RequestContext 混合了两类数据

| 类别 | 字段 | 生命周期 |
|------|------|---------|
| 运行时上下文 | conversation_id, session_source | Per-session（相对静态） |
| 模型配置 | reasoning_effort, reasoning_summary | Per-session（相对静态） |
| 会话状态 | previous_response_id | Per-turn（动态） |
| 采样参数 | effective_parameters | Per-turn（可能变化） |

**潜在问题**:
- 语义不够清晰：什么是"请求上下文"？
- 未来扩展时可能混入更多不相关字段

**长期改进方向**:
- 可能进一步拆分为 `RuntimeContext` + `ModelConfig` + `TurnState`
- 但当前方案已足够清晰，暂不优化

### 6. 内存复制仍然存在

**问题**: 虽然不 clone Prompt，但仍需 clone 4 个字段

**当前成本**:
- `previous_response_id`: `Option<String>` clone (~50 bytes)
- `effective_parameters`: `ModelParameters` clone (~40 bytes)
- `reasoning_effort`: `Option<Enum>` copy (~2 bytes)
- `reasoning_summary`: `Option<Enum>` copy (~2 bytes)

**进一步优化方向** (Phase 2):
- 使用 `Arc<RequestContext>` 避免字段 clone
- 但需要评估 Arc 的开销（引用计数）
- 当前 ~100 bytes clone 可接受，暂不优化

---

## 未来优化

### Phase 2: Arc 优化（可选）

**如果 RequestContext clone 成为瓶颈**:

```rust
// 修改 stream_with_adapter 签名
pub async fn stream_with_adapter(
    &self,
    prompt: &Prompt,
    context: Arc<RequestContext>,  // ✅ 使用 Arc
    provider: &ModelProviderInfo,
    adapter_name: &str,
    global_stream_idle_timeout: Option<u64>,
) -> Result<ResponseStream>

// 在 client.stream() 中构造
let context = Arc::new(RequestContext {
    // ...
});
```

**优缺点**:
- ✅ 零 clone（只复制 Arc 指针）
- ✅ 多个 adapter 方法共享同一 context
- ❌ Arc 引用计数开销（atomic 操作）
- ❌ 所有字段变为只读（如果需要修改需要 clone）

**决策**: 当前 ~100 bytes clone 可接受，暂不引入 Arc

### Phase 3: 进一步分离上下文（可选）

**如果 RequestContext 语义混乱**:

```rust
#[derive(Debug, Clone)]
pub struct RuntimeContext {
    pub conversation_id: String,
    pub session_source: String,
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub reasoning_effort: Option<ReasoningEffortConfig>,
    pub reasoning_summary: Option<ReasoningSummaryConfig>,
}

#[derive(Debug, Clone)]
pub struct TurnState {
    pub previous_response_id: Option<String>,
    pub effective_parameters: ModelParameters,
}

// Adapter 接收 3 个独立上下文
fn transform_request(
    &self,
    prompt: &Prompt,
    runtime: &RuntimeContext,
    model_config: &ModelConfig,
    turn_state: &TurnState,
    provider: &ModelProviderInfo,
) -> Result<JsonValue>;
```

**优缺点**:
- ✅ 语义更清晰
- ✅ 每个上下文职责单一
- ❌ 参数数量增加（4 个上下文）
- ❌ 构造复杂度增加

**决策**: 当前 RequestContext 已足够清晰，暂不拆分

### Phase 4: 缓存 RequestContext（可选）

**如果发现重复构造 RequestContext**:

```rust
impl ModelClient {
    // 缓存 session-level 字段
    cached_context: Arc<Mutex<Option<RequestContext>>>,

    pub async fn stream(
        &self,
        prompt: &Prompt,
        previous_response_id: Option<String>,
    ) -> Result<ResponseStream> {
        // 复用缓存的 context，只更新 per-turn 字段
        let mut cached = self.cached_context.lock().await;
        if let Some(ref mut ctx) = *cached {
            ctx.previous_response_id = previous_response_id;
            ctx.effective_parameters = self.resolve_parameters();
            return self.stream_with_adapter_cached(prompt, ctx.clone()).await;
        }

        // 首次调用，构造并缓存
        // ...
    }
}
```

**优缺点**:
- ✅ 避免重复构造 session-level 字段
- ❌ 增加复杂度（缓存失效、线程安全）
- ❌ 当前构造成本低（~100 bytes），优化收益小

**决策**: 当前构造成本可接受，暂不缓存

---

## 附录

### A. 全局搜索命令

**查找所有需要修改的位置**:

```bash
# 1. 查找所有 Prompt 构造
rg "Prompt \{" codex-rs/core --type rust -A 10 > /tmp/prompt-constructions.txt

# 2. 查找所有 client.stream() 调用
rg "\.stream\(&prompt" codex-rs/core --type rust -B 2 -A 2 > /tmp/stream-calls.txt

# 3. 查找所有从 prompt 读取 4 字段的位置
rg "prompt\.(reasoning_effort|reasoning_summary|previous_response_id|effective_parameters)" \
   codex-rs/core --type rust > /tmp/prompt-field-reads.txt

# 4. 查找所有 RequestContext 构造
rg "RequestContext \{" codex-rs/core --type rust -A 10 > /tmp/context-constructions.txt

# 5. 查找所有 adapter.transform_request 调用
rg "\.transform_request\(" codex-rs/core --type rust -B 2 -A 2 > /tmp/transform-calls.txt
```

### B. 批量替换脚本（谨慎使用）

**仅作参考，建议手动逐个检查**:

```bash
# 在 gpt_openapi.rs 中批量替换
sed -i.bak 's/prompt\.effective_parameters/context.effective_parameters/g' \
    codex-rs/core/src/adapters/gpt_openapi.rs

sed -i.bak 's/prompt\.previous_response_id/context.previous_response_id/g' \
    codex-rs/core/src/adapters/gpt_openapi.rs

sed -i.bak 's/prompt\.reasoning_effort/context.reasoning_effort/g' \
    codex-rs/core/src/adapters/gpt_openapi.rs

sed -i.bak 's/prompt\.reasoning_summary/context.reasoning_summary/g' \
    codex-rs/core/src/adapters/gpt_openapi.rs
```

**注意**:
- 务必在执行前备份文件
- 执行后手动检查所有替换是否正确
- 可能会误替换其他 prompt 相关代码

### C. 影响范围总结表

| 文件 | 行数范围 | 改动类型 | 优先级 |
|------|---------|---------|-------|
| `client_common.rs` | 31-66 | 删除 Prompt 字段 | 🔴 高 |
| `adapters/mod.rs` | 20-50, 431-460 | 扩展 RequestContext + trait | 🔴 高 |
| `client.rs` | 180-240 | 修改 stream() 签名 | 🔴 高 |
| `adapters/http.rs` | 69-136 | 简化参数 + 删除 clone | 🔴 高 |
| `adapters/gpt_openapi.rs` | 427-470 | 更新字段读取 | 🔴 高 |
| `codex.rs` | 1939-1968, 调用点 | 更新 Prompt 构造 + stream 调用 | 🟡 中 |
| `tests/**/*.rs` | 多处 | 更新测试构造和调用 | 🟢 低 |

### D. 相关文档

- **Rust API Guidelines**: https://rust-lang.github.io/api-guidelines/
- **Performance Book**: https://nnethercote.github.io/perf-book/
- **codex-rs Architecture**: `codex-rs/CLAUDE.md`
- **Adapter System**: `codex-rs/docs/architecture/adapters.md`（如果存在）

---

## 总结

### 关键决策

1. ✅ **复用 RequestContext** - 统一"请求上下文"语义
2. ✅ **从 Prompt 完全移除 4 字段** - 清晰的职责分离
3. ✅ **client.stream() 增加参数** - 数据流更清晰
4. ✅ **零大对象 clone** - 只复制 ~100 bytes 小字段

### 预期收益

- **性能提升**: 98-99.9% clone 成本降低
- **年度节省**: ~300 GB → ~300 MB 内存分配
- **代码清晰度**: Prompt 专注消息，RequestContext 负责配置
- **可扩展性**: 新增配置参数只需修改 RequestContext

### 实施风险

- **API 变更**: `client.stream()` 签名变更（内部 API，可控）
- **修改量**: 6-7 个核心文件 + 测试文件（中等规模）
- **测试覆盖**: 需要全面测试 adapter 路径

### 下一步

1. 审查本 RFC，确认技术方案
2. 创建 tracking issue（如 GitHub Issue）
3. 按照实施步骤逐步修改
4. 每个 Phase 完成后运行验证清单
5. 所有测试通过后合并

---

**文档版本**: v1.0
**最后更新**: 2025-11-21
**状态**: 等待审查
