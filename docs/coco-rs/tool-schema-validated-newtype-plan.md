# Plan: Tool Input Schema — Validated Newtype + Single Parse Seam

## Context

`coco-rs` 的 `Tool` trait 把三件**本质独立**的事捆在一个 associated type 上：

```rust
// core/tool-runtime/src/traits.rs:382-391
pub trait Tool: Send + Sync + 'static {
    type Input: for<'de> Deserialize<'de> + JsonSchema + Send + Sync + 'static;
    // ...
}
```

- **反序列化**（`Deserialize`）—— Tool executor 边界的 wire→typed 转换
- **schema 派生**（`JsonSchema`）—— 给 LLM / strict provider 看的 JSON Schema 来源
- **执行 payload 类型**（`execute(input: Self::Input, ...)`）—— 业务代码的 typed seam

静态 Tool（41 个内置）三件事来自同一个 `#[derive(Deserialize, JsonSchema)] struct`
—— 不变量自动同步。动态 Tool 把 `type Input = Value`：
- `Deserialize` 走 `Value`（passthrough），OK
- `JsonSchema` 走 schemars 给 `Value` 派生的 garbage（`type: null` 或 `anyOf: [...]`）
- 严格 OpenAI-compatible provider（DeepSeek、xAI Grok strict、Together strict 等）
  拒绝这个 schema，wire-level 400

### 当前生产环境实际状态

| 路径 | 实现 | `type Input` | `input_json_schema()` 状态 |
|------|------|--------------|------------------------|
| External MCP server | `McpTool` | `Value` | ✅ X3 fix (`0303dc3ef2`) 已 override |
| SDK custom tool（in-process MCP） | `McpTool` | `Value` | ✅ 同 McpTool |
| `--json-schema` 结构化输出 | `StructuredOutputTool` | `Value` | ✅ P0 fix（本文档触发）已 override |
| 41 个内置静态 Tool | typed struct | typed | N/A，blanket default 安全 |

X3 fix 修了 `McpTool` 一家，但 `StructuredOutputTool`（架构等价的第二个动态 Tool）
**直到本文档触发前都带着同样的 bug 在生产里**。这暴露的不是某个具体 Tool 的疏忽，
而是**架构层缺少结构性强制**：

- `Tool::input_json_schema()` 的 blanket default 在 release build **无任何保护**
  （X2 follow-up 加的 `debug_assert!` 只在 dev / test 触发）
- doc 警告"MUST override"是文化约定，不是 type system 约束
- `core/tools/CLAUDE.md` 写"MCPTool is the only dynamic tool" —— **文档自己漂移了**
- Plugin Tool / Custom Tool / HTTP Tool 等未来动态 Tool 会继承同一 footgun

### 历史背景

| Commit / 节点 | 含义 |
|---|---|
| `0303dc3ef2` | X3 fix — McpTool 显式 override `input_json_schema()`，stash wire schema |
| `9ab148b28a` | X2 follow-up — `Tool::input_json_schema()` blanket default 加 `debug_assert` + 4 个 McpTool regression test |
| 本文档触发 commit | P0 — `StructuredOutputTool::input_json_schema()` override + 3 个 regression test + CLAUDE.md 更正 |

P0 把已存在的第 2 个 bug 灭了，但**根因仍未消除**：第 3 个动态 Tool（Plugin Tool
是首要候选）只要不抄 doc，就会重蹈覆辙。

---

## Problem Statement

**根因**：`Tool` trait 让 `type Input: JsonSchema` 同时承担 schema 派生职责，
而 `serde_json::Value` 实现了 `JsonSchema`（permissively），让"派生路径"在
`Value` Input 下 silent fail。

**症状**：

1. **Release build 无保护** —— `debug_assert!` 不在 release 路径
2. **文档漂移** —— `core/tools/CLAUDE.md` 错记"only one dynamic tool"
3. **Override 漏检** —— `StructuredOutputTool` 漏掉 `input_json_schema()` override，
   生产带 bug
4. **Schema 双源** —— `input_schema()` (parsed view) 和 `input_json_schema()` (full
   envelope) 各自有 default，drift 风险只能靠测试覆盖发现
5. **未来扩展不安全** —— Plugin / Custom / HTTP Tool 框架时还会重新踩同一个洞

---

## Options Considered

完整对抗性评审保留在 session memory，这里只列结论性分歧。

| # | 方案 | 评价 |
|---|------|------|
| 0 | 维持现状（debug_assert + 测试 + doc） | release 无保护；P0 已证不够 |
| A | 删 `JsonSchema` bound + helper 函数 | drift 风险；`Value` 仍可 sneak through |
| B | sealed `HasStaticInputSchema` marker | marker trait = 用 trait 模拟 enum；sealed 在 Rust 是文化共识不是机制 |
| D | proc macro 注解 | 加 proc-macro 基础设施 + IDE/diag 退化；不解决架构问题 |
| E | 只删 `input_json_schema` default | trait surface 不对称；不挡 `Value` sneak through |
| F | 拆 Tool / StaticTool sub-trait | coherence 冲突；over-engineer |
| G | schema source 抽 associated type | 架构最正确；Tool struct 加字段；改动量大 |
| H | 杀 `type Input`，全 `Value`（codex-rs 形态） | 失去 typed seam，Rust 优势倒退 |
| **I** | **validated newtype + 单一 parse seam** | **此文档选项** |

方案 I 是 G 的工程化收紧 + codex-rs `parse_tool_input_schema` 防线的移植，
保留 coco-rs 的 typed Tool 优势。

---

## Architecture Choice — Option I

### 核心设计

1. **`ToolInputSchema` 升级为 validated newtype**

   现在是 `{ properties: HashMap, required: Vec<String> }` 的 parsed 视图。
   重构为持有完整 type-narrowed schema map 的 newtype，**所有公开构造器自带校验**：

   ```rust
   // core/tool-runtime/src/schema.rs (重写)
   pub struct ToolInputSchema {
       inner: serde_json::Map<String, Value>,  // 保证 root = {type:"object", ...}
   }

   impl ToolInputSchema {
       /// 静态 Tool：从 `#[derive(JsonSchema)]` 类型派生 + 校验。
       /// 启动期 panic（不是 wire-time 400）。
       pub fn from_typed<T: schemars::JsonSchema>() -> Self;

       /// 动态 Tool：从 wire-provided JSON Schema 解析 + 校验。
       pub fn from_wire(raw: Value) -> Result<Self, SchemaError>;

       /// 已 type-narrowed 的运行时构造（如 Plugin manifest 直接给 fields）。
       pub fn from_parts(
           properties: Map<String, Value>,
           required: Vec<String>,
       ) -> Result<Self, SchemaError>;

       // 视图方法（无构造器，不可绕过校验）
       pub fn as_value(&self) -> Value;
       pub fn properties(&self) -> &Map<String, Value>;
       pub fn required(&self) -> &[String];
       pub fn omit_property(&self, field: &str) -> Self;  // session-aware mutator
   }

   // 唯一 fallible 错误类型
   #[derive(Debug, thiserror::Error)]
   pub enum SchemaError {
       #[error("schema root is not a JSON object")]
       NotObject,
       #[error("schema `type` field is not the string \"object\": {0:?}")]
       BadType(Value),
       #[error("schema `properties` is not a JSON object")]
       PropertiesNotMap,
       #[error("schema `required` is not an array of strings")]
       RequiredNotStringArray,
   }
   ```

   `inner: Map<String, Value>` 是 private。任何持有 `ToolInputSchema` 的代码
   **不可能**绕过校验拿到原始 garbage —— Rust privacy 给的硬约束。

2. **`Tool` trait 形态收紧**

   ```rust
   // core/tool-runtime/src/traits.rs
   pub trait Tool: Send + Sync + 'static {
       /// 反序列化职责保留，但 schema 派生职责剥离。
       type Input: for<'de> Deserialize<'de> + Send + Sync + 'static;
       //         ↑ 删 JsonSchema bound —— Value 不再"伪合规"
       type Output: Serialize + for<'de> Deserialize<'de> + JsonSchema + ...;

       /// 返回 stored schema 的引用 —— 无 default、无 drift 可能。
       fn input_schema(&self) -> &ToolInputSchema;

       /// Session-aware 变体。默认 borrow；override 时 own 一个 transformed 版本。
       fn input_schema_for_session(&self, _: &SchemaContext) -> Cow<'_, ToolInputSchema> {
           Cow::Borrowed(self.input_schema())
       }

       // execute / render_for_model 等保持 typed，维护性优势保留
       async fn execute(&self, input: Self::Input, ctx: &ToolUseContext) -> Result<...>;
       fn render_for_model(&self, output: &Self::Output) -> Vec<...>;
       // ...
   }
   ```

   - **删除**：`input_json_schema()` / `input_json_schema_for_session()` —— 由
     `as_value()` / `as_value_for_session()` 视图取代（或 caller 直接 `.as_value()`）
   - **删除**：`input_schema()` 和 `input_json_schema()` 的 blanket default
   - **删除**：`derive_input_schema_value::<Self::Input>()` 调用链 —— 仅留作
     `ToolInputSchema::from_typed` 内部 helper

3. **Tool struct 存储 schema 作为字段**

   静态 Tool：
   ```rust
   pub struct ReadTool {
       schema: ToolInputSchema,  // 启动期一次 derive
   }
   impl ReadTool {
       pub fn new() -> Self {
           Self { schema: ToolInputSchema::from_typed::<ReadInput>() }
       }
   }
   impl Tool for ReadTool {
       type Input = ReadInput;
       fn input_schema(&self) -> &ToolInputSchema { &self.schema }
       // ...
   }
   ```

   动态 Tool（McpTool / StructuredOutputTool / 未来 PluginTool）：
   ```rust
   pub struct McpTool {
       schema: ToolInputSchema,  // wire 解析 + 校验一次，构造期错误立即返
       // ...
   }
   impl McpTool {
       pub fn new(wire_schema: Value, ...) -> Result<Self, SchemaError> {
           Ok(Self {
               schema: ToolInputSchema::from_wire(wire_schema)?,
               // ...
           })
       }
   }
   ```

4. **`from_wire` 是动态 schema 进入系统的唯一入口**

   - `McpTool::new` —— 已经走（X3 fix 之后是 inline 解析，本次重构改用 `from_wire`）
   - `StructuredOutputTool::new` —— 本次新增
   - `PluginTool::new`（未来）—— 框架立项时 by-construction 走
   - `CustomTool::new`（未来）—— 同上

   Plugin 作者即便在外部 crate，**只能调 `from_wire`** —— `inner` 是 private，
   没有其它构造路径。这是 Rust privacy + newtype 给的硬约束。

### 设计不变量

| 不变量 | 强制机制 |
|--------|---------|
| 任何 `ToolInputSchema` 实例的 root.type == "object" | `from_typed` 派生后 validate；`from_wire` 拒绝其它 type；`from_parts` 自动 fold in |
| 没有 `type: null` 字段在 wire 上 | `from_wire` 递归校验时拒绝（与 codex-rs `parse_tool_input_schema` 等价） |
| Tool 永远不会"忘 override" schema | `input_schema()` 无 default —— 漏写是 `error[E0046]` |
| 动态 Tool 不能绕过 wire 校验 | `inner` 是 private，构造器仅有 `from_typed` / `from_wire` / `from_parts`，三者都校验 |
| Schema 与 Input 不会 drift | 静态 Tool 用 `from_typed::<Self::Input>()` 一行；偏离需要显式硬编码不同类型，code review 即可捕捉 |
| Output schema 不退化 | `Self::Output: JsonSchema` bound 保留；blanket default 仍可用（Output 历史上无 `Value` 问题） |

---

## Migration Plan

按依赖顺序分阶段，每阶段独立可 commit、独立 CI 绿。

### Phase 0 — 准备（已完成）

- ✅ `0303dc3ef2` X3 fix（McpTool override）
- ✅ `9ab148b28a` X2 follow-up（debug_assert + 测试基础设施）
- ✅ 本文档触发 commit（StructuredOutputTool P0 + CLAUDE.md 更正）

### Phase 1 — 新 `ToolInputSchema` newtype 并行存在

**目标**：在不删除现有 trait 方法的前提下，让新类型可用。

- 新增 `core/tool-runtime/src/schema.rs::ToolInputSchemaV2`（暂用 V2 后缀
  避免命名冲突；Phase 4 rename）
- 实现 `from_typed` / `from_wire` / `from_parts` / 视图方法
- 错误类型 `SchemaError` 落在 `coco-error` 体系下，挂 `StatusCode::Resource`
- 单元测试：每个构造器 happy path + 各 `SchemaError` 变体

**改动文件**：
- `core/tool-runtime/src/schema.rs`（新增 ~250 行）
- `core/tool-runtime/src/schema.test.rs`（新增 ~150 行）
- `core/tool-runtime/src/lib.rs`（export 新类型）

**验证**：`just test-crate coco-tool-runtime`

### Phase 2 — 动态 Tool 迁移到 newtype

**目标**：McpTool 和 StructuredOutputTool 改用新类型作为字段。

- `core/tools/src/tools/mcp_tools.rs`
  - `McpTool` 字段 `raw_schema: Map<String, Value>` → `schema: ToolInputSchemaV2`
  - `McpTool::new` 改成 `Result<Self, SchemaError>`，内部走 `from_wire`
  - `Tool::input_json_schema` 改 `Some(self.schema.as_value())`
  - `Tool::input_schema` 改 derive from `self.schema.properties()` / `.required()`
- `core/tools/src/tools/structured_output.rs`
  - 同样模式
  - `StructuredOutputTool::new` 现有签名已经是 `Result<Self, String>`，错误类型升级
- `services/mcp/src/discovery.rs::convert_server_tools`
  - 调用 `McpTool::new` 处理 `Result`；invalid schema 的 server tool 跳过 + warn log
- `app/cli/src/headless.rs:412`
  - `register_structured_output_tool` 处理 `Result`

**改动文件**：~6 个文件，每个 5-20 行
**验证**：`just quick-check && cargo test -p coco-tools && cargo test -p coco-mcp`

### Phase 3 — 静态 Tool 批量迁移

**目标**：41 个静态 Tool 改成 schema-as-field 形态。

- 每个 `<X>Tool` struct 加 `schema: ToolInputSchemaV2` 字段
- 每个 `<X>Tool::new()` 内 `Self { schema: ToolInputSchemaV2::from_typed::<<X>Input>(), ... }`
- 每个 `impl Tool for <X>Tool` 的 `input_schema()` 改 `&self.schema`
- 删除每个 Tool 对 `input_json_schema()` 的（如有的）显式 override —— 在 Phase 4
  之前可以同时保留 V1（`ToolInputSchema`）和 V2 两条路径

**改动模式**（机械化，可脚本批处理）：

```diff
 pub struct ReadTool {
+    schema: ToolInputSchemaV2,
     // ... 现有字段
 }

 impl ReadTool {
     pub fn new(...) -> Self {
-        Self { /* fields */ }
+        Self {
+            schema: ToolInputSchemaV2::from_typed::<ReadInput>(),
+            /* fields */
+        }
     }
 }

 impl Tool for ReadTool {
     // ...
-    fn input_schema(&self) -> ToolInputSchema {
-        coco_tool_runtime::derive::derive_input_schema::<ReadInput>()
-    }
+    fn input_schema_v2(&self) -> &ToolInputSchemaV2 { &self.schema }
 }
```

**改动文件**：41 个 Tool 文件，每个 5-8 行
**估算行数**：~250 行净增（含 schema 字段 + 构造器赋值 + trait 方法替换）
**验证**：每改 5-10 个 Tool 跑一次 `just quick-check`；全完后 `just test`

### Phase 4 — Trait surface 收紧（破坏性变更）

**目标**：删除老 `ToolInputSchema` struct、删除 blanket default、删除
`JsonSchema` bound on `Self::Input`。

- `core/tool-runtime/src/traits.rs`
  - 删除 `Tool::input_schema()` 老签名（返回 `ToolInputSchema` 老类型）
  - 删除 `Tool::input_json_schema()`
  - 删除 `Tool::input_schema_for_session()` / `input_json_schema_for_session()`
  - 改名 `input_schema_v2` → `input_schema`，签名 `-> &ToolInputSchema`（新类型借旧名）
  - 删除 `Self::Input: JsonSchema` bound
- `core/tool-runtime/src/derive.rs`
  - `derive_input_schema_value::<T>` 改为 crate-private（仅 `ToolInputSchemaV2::from_typed` 内部用）
- `common/types/src/schema.rs` 老 `ToolInputSchema` struct
  - 删除（或保留为 `coco_tool_runtime::ToolInputSchema` 的别名以避免一次性大改）
- 消费侧（`app/query/src/engine_prompt.rs::build_language_model_tools` 等）
  - 调用 `tool.input_schema().as_value()` 而不是 `.input_json_schema()`
  - validator (`effective_tool_schema`) 入参类型对齐

**改动文件**：~15 个文件
**估算行数**：~200 行净减
**验证**：`just pre-commit`

### Phase 5 — Marker + 错误体验加固

**目标**：让"未来作者再次踩坑"在编译期 / 测试期立即可见。

- 在 `core/tool-runtime/CLAUDE.md` 写入"任何 `type Input = Value` 的 Tool 实现，
  必须 by-construction 经过 `ToolInputSchemaV2::from_wire` —— 不是 doc 警告，是
  schema 字段 + privacy 强制"
- 加 `cargo-deny` lint 或自定义 clippy 规则（可选）：检测 `Tool` 实现里
  `Self::Input = serde_json::Value` 且 schema 字段未来自 `from_wire` 的情况
- 加一个 integration test：`tests/integration/dynamic_tools_schema_validation.rs`
  扫描所有注册的 Tool，断言每个的 `input_schema().as_value()` 通过 strict
  provider schema 校验

**改动文件**：~3-5 个文件
**估算行数**：~150 行新增

---

## Risks & Mitigations

| 风险 | 严重度 | 缓解 |
|------|-------|------|
| Phase 3 改 41 个 Tool 是机械改但量大，PR review 累 | M | 分 PR 提交，每 PR 改 8-10 个 Tool；每个 PR 独立 `just pre-commit` |
| Tool struct 加 schema 字段，内存膨胀 | L | 每个 Tool ~100-500 bytes；41 个共 < 20 KB；singleton in registry |
| `from_typed` panic 在启动期，新人 panic 看不懂 | L | panic 消息明确指引 "use `from_wire()` for dynamic schemas"；并加 startup-time integration test 提前 catch |
| 消费侧 (engine_prompt / validator) `&ToolInputSchema` borrow 链调整 | M | Phase 3 后期实测；如有 lifetime 困难，单点改成 `Arc<ToolInputSchema>` |
| `Cow<'_, ToolInputSchema>` 在 session-aware 路径上加返回类型负担 | L | 现有 session-aware 调用点 < 5 处（AgentTool / TodoWrite 几个），手改 |
| 与未来 Plugin 框架对接需要约定 | L | Plugin 框架立项时把 `from_wire` 作为 ABI 边界；本计划完成后 Plugin 是受益方 |
| 多 crate 拆 `coco-error` 失败需返回 `Result<Self, SchemaError>` | L | 已有 `coco-error` 体系成熟；Phase 1 定型错误类型 |

### 已知架构问题不在本计划范围

| 问题 | 处理 |
|------|------|
| `Self::Output: JsonSchema` bound 不对称 | 暂保留 —— Output schema 历史上无 `Value` 问题；如未来出现，沿用同模式加 `ToolOutputSchema` |
| `engine.rs::run_session_loop` 1900 LoC | 独立工作（已记入 X2 deferred） |
| `turn_id: String` 改 `Arc<str>` | 独立优化，与本计划正交 |
| `ToolInputSchema` rename 大扫除 | Phase 4 时机一并做；如 Phase 4 已过大，留 Phase 6 |

---

## Benefits

### 立即收益（Phase 2 完成后）

- ✅ 任何动态 Tool 都**不可能**绕过 wire schema 校验 —— Rust privacy 强制
- ✅ Plugin Tool / Custom Tool 立项时不需要重新决策 schema 入口 —— `from_wire` 已是 API
- ✅ CLAUDE.md "MCPTool is the only dynamic tool" 这种文档漂移在结构上不再可能

### 全部完成后（Phase 4-5）

- ✅ 删除 `Tool::input_json_schema()` blanket default —— bug class 编译期消失
- ✅ 删除 `Self::Input: JsonSchema` bound —— `Value` 不再"伪合规"
- ✅ 单一 schema 数据源（每个 Tool 一个 `ToolInputSchema` 字段）—— drift 不可能
- ✅ 错误前置到启动期 / 构造期 —— 永远不会有 wire-time 400 因为 schema garbage
- ✅ TS-parity 在语义层（codex-rs `parse_tool_input_schema` 单一防线）+ Rust 优势在
  trait surface 层（typed Input/Output 保留）

### 长期外延

- 给将来"PluginTool 框架"、"CustomTool 用户脚本声明"、"HTTPTool OpenAPI
  集成"提供现成的 schema 校验入口 —— 不需要每个新动态 Tool 类型重新设计
- 给 `ToolOutputSchema` 类似改造留出对称模板（如出现 Output schema 问题）
- 给 `cargo-deny` / 自定义 clippy lint 提供可识别的"动态 Tool"模式：
  `type Input = Value` + `schema: ToolInputSchema` 来自 `from_wire`

---

## Out of Scope

不在本计划：

- 删除 `Tool` trait 改 codex-rs struct-based 形态（方案 H —— Rust 优势倒退）
- proc macro 隐藏 boilerplate（方案 D —— 加基础设施，不解决架构）
- sealed marker trait（方案 B —— 用 trait 模拟 enum，Rust 反 pattern）
- 把 `Tool::execute` 从 typed 改成 `Value`-only（H 的副作用）

不在本计划但与之协同：

- Plugin Tool 框架 —— 应 Phase 4 完成后立项，consumer 视角接 `from_wire`
- 协议层 `Turn*` 事件统一带 `stop_reason`（独立工作）

---

## Decision Log

| 决策 | 选择 | 理由 |
|------|------|------|
| Schema 是 trait 派生还是 struct 字段 | **struct 字段** | 派生时机由 Tool 控制，错误前置到构造期 |
| schema-source 独立 associated type 还是 newtype 字段 | **newtype 字段** | 不加 trait surface 复杂度，对 41 个静态 Tool 迁移更平 |
| `Self::Input: JsonSchema` bound 删 or 保 | **删** | 保留就让 `Value` 仍可"伪合规"，bug 不彻底关 |
| 校验失败：panic vs Result | **panic for `from_typed`, Result for `from_wire`** | 静态来源失败 = 编译/启动 bug 应立即崩；动态来源失败 = 业务错误应可恢复 |
| 老 `ToolInputSchema` 命名复用还是另起 | **Phase 1 用 V2 后缀，Phase 4 改回** | 避免 Phase 1-3 期间 namespace 冲突 |
| `Output: JsonSchema` bound 对称改 | **不改** | Output 历史上无 `Value` 问题；YAGNI |
| Migration 一次性还是分阶段 | **分阶段 5 phase** | 41 个 Tool 一次改的 review 负担过大 |

---

## Implementation Schedule（建议）

| Phase | 估算工时 | 阻塞依赖 |
|-------|---------|---------|
| Phase 1（新 newtype）| 0.5 day | 无 |
| Phase 2（McpTool / StructuredOutputTool 迁移）| 0.5 day | Phase 1 |
| Phase 3（41 个静态 Tool 批量迁移）| 1-1.5 day | Phase 1（不依赖 Phase 2） |
| Phase 4（trait surface 收紧）| 1 day | Phase 2 + Phase 3 全完 |
| Phase 5（marker + 测试加固）| 0.5 day | Phase 4 |
| **总计** | **3.5-4 day** | |

Phase 1 + Phase 2 可视作"最小可独立 ship 单元" —— 修了 wire 校验入口，给后续阶段
铺路；即便 Phase 3-5 暂缓，已有动态 Tool 路径在结构上已收紧。

---

## References

- TS source: `tools/SyntheticOutputTool/SyntheticOutputTool.ts`（StructuredOutputTool 的 TS 等价）
- codex-rs source: `codex-rs/tools/src/json_schema.rs::parse_tool_input_schema`（防线参考实现）
- Related: [docs/coco-rs/crate-coco-tools.md](crate-coco-tools.md)
- Related: [docs/coco-rs/crate-coco-tool.md](crate-coco-tool.md)（tool-runtime crate）
- Memory: `project_coco_rs_mcp_tool_input_json_schema.md`（McpTool override 来由）
- Memory: `project_coco_rs_turn_completed_semantics.md`（X2 wire 协议讨论上下文）
