# coco-otel — Crate Plan

TS source: `utils/telemetry/` (9 files, ~4K LOC), `services/analytics/` (8 files, ~4K LOC),
`utils/telemetryAttributes.ts`, `utils/stats.ts` (1061 LOC), `utils/statsCache.ts`,
`utils/log.ts`, `utils/errorLogSink.ts`, `utils/debug.ts`, `utils/sinks.ts`,
`utils/fileOperationAnalytics.ts`, `services/internalLogging.ts`,
`services/toolUseSummary/`, `services/api/logging.ts`

cocode-rs source: `common/otel/` (13 source files) — L0-L1 层直接复用

## Strategy: HYBRID (6 层模型)

cocode-rs 的 export 管道 (L0) 和基础事件 (L1) 质量优秀，直接复用。
TS 的 span 层级 (L2)、应用事件 (L3)、业务 metrics (L4)、自定义 exporter (L5) 需要从 TS 移植。
L6 运营控制 (sampling/killswitch/opt-out) 暂不实现。

| 层级 | 来源 | 状态 |
|------|------|------|
| L0: Export 管道 (OTLP gRPC/HTTP, TLS, Statsig, InMemory) | cocode-rs | **复用** |
| L1: 基础事件 (7 个核心事件: conversation_starts, user_prompt, tool_decision, tool_result, api_request, sse_event, sse_event_completed) | cocode-rs | **复用** |
| L2: Span 层级 (interaction→llm_request→tool→hook→user_input) | TS `sessionTracing.ts` | **新增** |
| L3: 应用事件 (~53 个事件类型: query/session/config/oauth/mcp) | TS `analytics/`, `api/logging.ts` | **新增** |
| L4: 业务 metrics (token/cost/LOC/session/active_time/PR/commit) | TS `instrumentation.ts` | **新增** |
| L5: 自定义 exporter (BigQuery, 1P Event Logging, Perfetto) | TS `bigqueryExporter.ts`, `firstPartyEventLogger.ts`, `perfettoTracing.ts` | **新增** |
| L6: 运营控制 (event sampling, killswitch, metrics opt-out, GrowthBook) | TS `sinkKillswitch.ts`, `growthbook.ts` | **暂不实现** |

## Dependencies

```
coco-otel depends on:
  - coco-error            (ErrorExt for telemetry_msg)
  - opentelemetry, opentelemetry_sdk, opentelemetry-otlp
  - tracing, tracing-subscriber, tracing-opentelemetry
  - opentelemetry-appender-tracing
  - reqwest (HTTP exporter + BigQuery)
  - serde, serde_json, chrono, tokio

coco-otel does NOT depend on:
  - coco-types            (no message/tool types — events use &str/Value)
  - coco-config           (settings passed in via OtelSettings struct)
  - coco-inference        (no LLM calls)
  - any core/app crate
```

## Data Definitions

### L0: Export 管道 (from cocode-rs, 直接复用)

```rust
/// 已有 — cocode-rs common/otel/src/config.rs
pub struct OtelSettings {
    pub environment: String,
    pub service_name: String,
    pub service_version: String,
    pub home_dir: PathBuf,
    pub exporter: OtelExporter,        // logs
    pub trace_exporter: OtelExporter,  // traces
    pub metrics_exporter: OtelExporter,// metrics
}

pub enum OtelExporter {
    None,
    Statsig,
    OtlpGrpc { endpoint: String, headers: HashMap<String, String>, tls: Option<OtelTlsConfig> },
    OtlpHttp { endpoint: String, headers: HashMap<String, String>, protocol: OtelHttpProtocol, tls: Option<OtelTlsConfig> },
}

pub enum OtelHttpProtocol { Binary, Json }

pub struct OtelTlsConfig {
    pub ca_certificate: Option<AbsolutePathBuf>,
    pub client_certificate: Option<AbsolutePathBuf>,
    pub client_private_key: Option<AbsolutePathBuf>,
}

/// 已有 — cocode-rs common/otel/src/otel_provider.rs
pub struct OtelProvider {
    pub logger: Option<SdkLoggerProvider>,
    pub tracer_provider: Option<SdkTracerProvider>,
    pub tracer: Option<Tracer>,
    pub metrics: Option<MetricsClient>,
}

/// 已有 — cocode-rs common/otel/src/metrics/client.rs
pub struct MetricsClient(Arc<MetricsClientInner>);  // Clone, thread-safe

/// 已有 — cocode-rs common/otel/src/metrics/timer.rs
pub struct Timer { ... }  // Drop impl auto-records duration
```

### L1: 基础事件 (from cocode-rs, 直接复用)

```rust
/// 已有 — cocode-rs common/otel/src/lib.rs
pub struct OtelEventMetadata {
    pub conversation_id: String,
    pub auth_mode: Option<String>,
    pub account_id: Option<String>,
    pub account_email: Option<String>,
    pub provider: String,
    pub model: String,
    pub log_user_prompts: bool,
    pub app_version: &'static str,
    pub terminal_type: String,
}

pub struct OtelManager {
    pub metadata: OtelEventMetadata,
    pub session_span: Span,
    pub metrics: Option<MetricsClient>,
    pub metrics_use_metadata_tags: bool,
}

pub enum ToolDecisionSource { Config, User }

/// 已有事件方法 (otel_manager.rs):
/// - conversation_starts()
/// - user_prompt()
/// - tool_decision()
/// - tool_result() + log_tool_result() + log_tool_failed()
/// - record_api_request() + log_request()
/// - sse_event() + sse_event_completed() + sse_event_failed()
/// - counter() + histogram() + record_duration() + start_timer()
```

### L2: Span 层级体系 (from TS `sessionTracing.ts`, 新增)

```rust
/// Span 管理器 — 维护 interaction→child 的嵌套关系
/// TS: startInteractionSpan/endInteractionSpan + start/endLLMRequestSpan + start/endToolSpan etc.
pub struct SpanManager {
    interaction_span: Option<Span>,  // 当前用户交互周期的 root span
}

impl SpanManager {
    /// 用户提交 prompt 时创建 interaction span
    pub fn start_interaction(&mut self, user_prompt: &str, sequence: i64) -> Span;
    pub fn end_interaction(&mut self, duration_ms: i64);

    /// LLM API 请求 span (interaction 的 child)
    pub fn start_llm_request(&self, model: &str, fast_mode: bool) -> Span;
    pub fn end_llm_request(
        span: Span,
        input_tokens: i64,
        output_tokens: i64,
        cache_read_tokens: i64,
        duration_ms: i64,
        ttft_ms: i64,
        has_tool_call: bool,
    );

    /// Tool 执行 span (interaction 的 child)
    pub fn start_tool(&self, tool_name: &str) -> Span;
    pub fn end_tool(span: Span, duration_ms: i64, success: bool, error: Option<&str>);

    /// 等待用户批准 span (tool 的 child)
    pub fn start_blocked_on_user(parent: &Span) -> Span;
    pub fn end_blocked_on_user(span: Span);

    /// Hook 执行 span (interaction 的 child)
    pub fn start_hook(&self, hook_name: &str, hook_type: &str) -> Span;
    pub fn end_hook(span: Span, duration_ms: i64);

    /// 等待用户输入 span
    pub fn start_user_input(&self, label: Option<&str>) -> Span;
    pub fn end_user_input(span: Span);
}
```

### L3: 应用事件 (from TS `analytics/`, 新增)

```rust
/// 事件记录器 — 统一的事件发送接口
/// TS: logEvent() / logEventAsync() / logOTelEvent()
impl OtelManager {
    /// 通用事件记录 (对应 TS logEvent)
    pub fn log_event(&self, event_name: &str, metadata: &Value);

    // --- Query 生命周期 ---
    pub fn query_before_attachments(&self, ...);
    pub fn query_after_attachments(&self, ...);
    pub fn query_error(&self, error: &str, ...);
    pub fn auto_compact_succeeded(&self, tokens_before: i64, tokens_after: i64);
    pub fn model_fallback_triggered(&self, from_model: &str, to_model: &str);
    pub fn streaming_tool_execution_used(&self, used: bool);

    // --- Session 生命周期 ---
    pub fn session_init(&self, ...);
    pub fn session_started(&self, startup_ms: i64, ...);
    pub fn session_exit(&self, total_cost_usd: f64, total_turns: i64, ...);

    // --- API 成功详情 (扩展现有 record_api_request) ---
    pub fn api_success(&self, metadata: ApiSuccessMetadata);
    pub fn api_error_detailed(&self, metadata: ApiErrorMetadata);

    // --- Config 变更 ---
    pub fn config_changed(&self, key: &str, ...);
    pub fn config_model_changed(&self, old_model: &str, new_model: &str);
    pub fn thinking_toggled(&self, enabled: bool);

    // --- OAuth 流程 ---
    pub fn oauth_flow_start(&self, provider: &str);
    pub fn oauth_success(&self, provider: &str);
    pub fn oauth_error(&self, provider: &str, error: &str);

    // --- MCP 事件 ---
    pub fn mcp_start(&self, server_name: &str);
    pub fn mcp_add(&self, server_name: &str);
    pub fn mcp_error(&self, server_name: &str, error: &str);

    // --- Tool 细粒度 (扩展现有 tool_decision/tool_result) ---
    pub fn tool_cancelled(&self, tool_name: &str, call_id: &str);
    pub fn tool_progress(&self, tool_name: &str, call_id: &str, ...);
    pub fn tool_permission_allowed(&self, tool_name: &str, source: ToolDecisionSource);
    pub fn tool_permission_rejected(&self, tool_name: &str, reason: &str);
}

/// API 成功事件的完整元数据
/// TS: tengu_api_success 有 30+ 字段
pub struct ApiSuccessMetadata {
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_input_tokens: i64,
    pub duration_ms: i64,
    pub duration_ms_including_retries: i64,
    pub ttft_ms: i64,
    pub attempt: i64,
    pub cost_usd: f64,
    pub stop_reason: String,
    pub provider: String,
    pub request_id: Option<String>,
    pub query_source: String,
    pub fast_mode: bool,
    pub text_content_length: i64,
    pub thinking_content_length: i64,
}

/// API 错误事件的完整元数据
pub struct ApiErrorMetadata {
    pub model: String,
    pub error: String,
    pub status: Option<i32>,
    pub error_type: String,
    pub duration_ms: i64,
    pub attempt: i64,
    pub provider: String,
    pub request_id: Option<String>,
    pub client_request_id: Option<String>,
}
```

### L4: 业务 Metrics (from TS `instrumentation.ts`, 新增)

```rust
/// 业务级 metric 名称常量
/// TS: meter.createCounter("claude_code.*")
pub mod metric_names {
    pub const SESSION_COUNT: &str = "coco.session.count";
    pub const TOKEN_USAGE: &str = "coco.token.usage";
    pub const COST_USAGE: &str = "coco.cost.usage";
    pub const LINES_OF_CODE: &str = "coco.lines_of_code.count";
    pub const ACTIVE_TIME: &str = "coco.active_time.total";
    pub const PULL_REQUEST_COUNT: &str = "coco.pull_request.count";
    pub const COMMIT_COUNT: &str = "coco.commit.count";
    pub const CODE_EDIT_DECISION: &str = "coco.code_edit_tool_decision.count";
}

/// TS metric tag 键常量
pub mod tag_keys {
    pub const MODEL: &str = "model";
    pub const TOKEN_TYPE: &str = "token_type";  // input, output, cached
    pub const TOOL_NAME: &str = "tool";
    pub const SUCCESS: &str = "success";
    pub const QUERY_SOURCE: &str = "query_source";
    pub const DECISION_TYPE: &str = "decision_type";
}

impl OtelManager {
    // --- Token & Cost ---
    pub fn record_token_usage(&self, model: &str, input: i64, output: i64, cached: i64);
    pub fn record_cost(&self, model: &str, cost_usd: f64, tool_name: Option<&str>);

    // --- Code changes ---
    pub fn record_lines_of_code(&self, lines_added: i64, decision: &str);
    pub fn record_commit(&self, source: &str);
    pub fn record_pull_request(&self, source: &str);

    // --- Session ---
    pub fn record_session_start(&self);
    pub fn record_active_time(&self, ms: i64);
}
```

### L5: 自定义 Exporter (from TS, 新增)

```rust
/// BigQuery metrics exporter — 企业级 metrics 导出
/// TS: bigqueryExporter.ts → https://api.anthropic.com/api/claude_code/metrics
pub struct BigQueryExporter {
    endpoint: String,
    api_key: String,
    export_interval: Duration,  // 5 minutes
}

impl PushMetricExporter for BigQueryExporter { ... }

/// 1P Event Logging exporter — 第一方事件日志
/// TS: firstPartyEventLogger.ts → https://api.anthropic.com/api/event_logging/batch
pub struct FirstPartyEventExporter {
    endpoint: String,
    chunk_size: i64,     // 200 events per batch
    retry_config: RetryConfig,  // quadratic backoff
    disk_fallback: PathBuf,     // failed events persisted to disk
}

/// Perfetto tracing — Chrome Trace Event 格式输出
/// TS: perfettoTracing.ts → ~/.coco/traces/trace-<session-id>.json
pub struct PerfettoTracer {
    output_path: PathBuf,
    write_interval: Duration,
}

impl PerfettoTracer {
    pub fn start_interaction(&mut self, prompt: &str);
    pub fn end_interaction(&mut self);
    pub fn start_llm_request(&mut self);
    pub fn end_llm_request(&mut self);
    pub fn start_tool(&mut self, tool_name: &str);
    pub fn end_tool(&mut self);
}
```

### Beta Tracing (L5, optional)

```rust
/// Beta 详细追踪 — 捕获完整 prompt/response 内容 (截断 60KB)
/// TS: betaSessionTracing.ts → ENABLE_BETA_TRACING_DETAILED + BETA_TRACING_ENDPOINT
/// 仅在企业环境启用，发送到 Honeycomb 等后端
pub struct BetaTracer {
    enabled: bool,
    endpoint: String,
    max_content_size: usize,  // 60KB, Honeycomb limit
}

impl BetaTracer {
    pub fn add_interaction_attributes(&self, span: &Span, prompt: &str);
    pub fn add_llm_request_attributes(&self, span: &Span, context: &Value, messages: &[Value]);
    pub fn add_llm_response_attributes(&self, span: &Span, metadata: &Value);
    pub fn add_tool_input_attributes(&self, span: &Span, tool_name: &str, input: &Value);
    pub fn add_tool_result_attributes(&self, span: &Span, tool_name: &str, result: &Value);
    fn truncate_content(content: &str, max_size: usize) -> &str;
}
```

## cocode-rs 优势保留

以下 cocode-rs 特性优于 TS，保留不改：

| 特性 | 说明 |
|------|------|
| Tag 验证 | `validate_tag_key/value()` 严格校验字符合法性，TS 无此校验 |
| Timer + Drop | scope 结束自动记录 duration，比 TS 手动 start/end 更安全 |
| Mutex poison recovery | `unwrap_or_else(PoisonError::into_inner)` 防 panic 传播 |
| 类型安全 | `ToolDecisionSource` 枚举 vs TS string literal |
| TLS 完整支持 | mTLS (CA cert + client cert + key)，TS 依赖 SDK 默认 |
| InMemory exporter | 测试专用 exporter，TS 需要 mock |
| Statsig 内置 | 直接内置 Statsig endpoint 配置 |

## Module Layout

```
common/otel/src/
  lib.rs                     # 复用 + 扩展 (add log_event, new metric methods)
  config.rs                  # 复用 (OtelSettings, OtelExporter)
  otel_provider.rs           # 复用 (OtelProvider, traceparent)
  otlp.rs                    # 复用 (OTLP exporter builders, TLS)
  metrics/
    mod.rs                   # 复用
    client.rs                # 复用 (MetricsClient)
    config.rs                # 复用 (MetricsConfig)
    error.rs                 # 复用 (MetricsError)
    timer.rs                 # 复用 (Timer)
    validation.rs            # 复用 (tag/metric name validation)
  traces/
    mod.rs                   # 复用
    otel_manager.rs          # 复用 + 扩展 (add L3 event methods)
  spans/                     # 新增: L2 span 层级
    mod.rs
    span_manager.rs          # SpanManager: interaction/llm/tool/hook/user_input spans
  events/                    # 新增: L3 应用事件
    mod.rs
    query_events.rs          # query lifecycle events
    session_events.rs        # session lifecycle events
    config_events.rs         # config change events
    oauth_events.rs          # OAuth flow events
    mcp_events.rs            # MCP events
  business_metrics.rs        # 新增: L4 业务 metrics
  exporters/                 # 新增: L5 自定义 exporter
    mod.rs
    bigquery.rs              # BigQuery exporter
    first_party.rs           # 1P event logging exporter
    perfetto.rs              # Perfetto tracing
  beta_tracing.rs            # 新增: L5 beta content tracing
```

## TS → Rust 文件映射

| TS 文件 | Rust 目标 | 层级 |
|---------|-----------|------|
| `utils/telemetry/sessionTracing.ts` | `spans/span_manager.rs` | L2 |
| `utils/telemetry/events.ts` | `events/mod.rs` | L3 |
| `utils/telemetry/instrumentation.ts` | `otel_provider.rs` (扩展) + `business_metrics.rs` | L0+L4 |
| `utils/telemetry/betaSessionTracing.ts` | `beta_tracing.rs` | L5 |
| `utils/telemetry/perfettoTracing.ts` | `exporters/perfetto.rs` | L5 |
| `utils/telemetry/bigqueryExporter.ts` | `exporters/bigquery.rs` | L5 |
| `utils/telemetry/logger.ts` | `otel_provider.rs` (已有 logger) | L0 |
| `services/analytics/index.ts` | `events/mod.rs` (logEvent API) | L3 |
| `services/analytics/firstPartyEventLogger.ts` | `exporters/first_party.rs` | L5 |
| `services/analytics/firstPartyEventLoggingExporter.ts` | `exporters/first_party.rs` | L5 |
| `services/analytics/sink.ts` | `events/mod.rs` (sink routing) | L3 |
| `services/analytics/datadog.ts` | 暂不实现 (L6 运营) | - |
| `services/analytics/metadata.ts` | `lib.rs` (OtelEventMetadata 已有) | L1 |
| `services/analytics/sinkKillswitch.ts` | 暂不实现 (L6 运营) | - |
| `services/analytics/config.ts` | `config.rs` (扩展) | L0 |
| `services/analytics/growthbook.ts` | 暂不实现 (L6 运营) | - |
| `services/api/logging.ts` | `events/query_events.rs` | L3 |
| `services/api/metricsOptOut.ts` | 暂不实现 (L6 运营) | - |
| `utils/telemetryAttributes.ts` | `lib.rs` (OtelEventMetadata 已有) | L1 |
| `utils/stats.ts` | `business_metrics.rs` | L4 |
| `utils/statsCache.ts` | `business_metrics.rs` | L4 |
| `utils/debug.ts` | `otel_provider.rs` (tracing 已覆盖) | L0 |
| `utils/sinks.ts` | `events/mod.rs` | L3 |
| `utils/fileOperationAnalytics.ts` | `events/mod.rs` | L3 |
| `services/internalLogging.ts` | `exporters/mod.rs` | L5 |
| `services/toolUseSummary/` | `events/mod.rs` | L3 |
