# Tool Input Schema — Source-of-Truth Refactor (v3.5, final)

> **Supersedes**:
> - [`tool-schema-validated-newtype-plan.md`](tool-schema-validated-newtype-plan.md) (v1, deprecated) — Input↔Schema type binding plus reject-style strict subset; refuted by the measured tool distribution.
> - [`tool-schema-source-plan.md`](tool-schema-source-plan.md) (v2, deprecated) — three-source-kind abstraction layer over-engineered; associated-type defaults unstable on rust-toolchain 1.93.1; Phase 0 crossed crate layering; StructuredOutput misclassified.
> - v3 / v3.1 / v3.2 / v3.3 / v3.4 (this document, earlier revisions) — see [Revision Log](#revision-log) for what each round got wrong.
>
> v3.5 is grounded in three Explore-agent surveys, line-by-line reading of the codex-rs source, and **five external review rounds**. Every field name, method name, and API signature in the templates has been re-verified against the live source — including the round-4 miss on `McpToolSchema`'s actual field names.
>
> **Single PR + 5 commits, one breaking reshape**, aligned with project rule "no backward-compat shims".

---

## Headline changes vs. v3.4

| Area | v3.4 | v3.5 (this revision) | Driver |
|---|---|---|---|
| Field-honesty enforcement | Invariant only stated as "runtime schema properties ⊆ `Self::Input` fields"; nothing closed the schema, so the model could set unknown fields that silently deserialize-drop | **All internally-built schemas (Bucket A/B/C/E) carry `"additionalProperties": false`** on both runtime and model views; Bucket A `Input` structs gain `#[serde(deny_unknown_fields)]` (defense in depth at the deserialize layer); external schemas (Bucket D / `--json-schema`) stay verbatim | Review round 5, finding 1 (P1) |
| Bash `_simulatedSedEdit` runtime-only field | Not addressed; closing the schema would have broken the TUI sed-edit rewrite path | **Dual-track**: runtime schema lists `_simulatedSedEdit` (TUI-injected); model schema omits it. Mirrors AgentTool's `mcp_servers` handling | Review round 5, finding 1 (P1) |
| MCP `skipped_tools` / `tombstoned_tools` source | Plan said handlers "populate the extended `McpServerStatus`" but provided no storage seam — `register_mcp_tools` returned `()`, `mcp/status` reads `McpConnectionManager` only | **New `McpRegistrationReports` store** on `SdkServerState` keyed by `server_name`; both SDK call sites write the report; `handle_mcp_status` reads it. `tool_count` reflects **registered** tools, not advertised | Review round 5, finding 2 (P1) |
| `SchemaCompileFailed` arm | Defensive `unreachable!()` + `debug_assert!` + `tracing::error!` — release builds still fell through and executed unvalidated input | **Fail-closed**: mark `tc.invalid = true` with `ToolInputInvalidReason::InternalSchemaError { message }`; tool call surfaces a `<tool_use_error>` to the model and never reaches `execute` | Review round 5, finding 3 (P1) |
| `ToolRegistry::replace_server_tools` alias handling | Removed tombstoned IDs only; aliases owned by *retained* tools (whose alias set changed across reconnect) survived in `inner.aliases` | **All server-owned aliases are wiped first**, then `register_with_aliases` re-populates them from the new batch under the same lock | Review round 5, finding 4 (P2) |
| Validator cache key | `(ToolId, canonical_bytes)` — correct but unbounded growth across reconnects with schema changes | **`HashMap<ToolId, CachedValidator { bytes, validator }>`** — replace-on-content-change keeps the cache O(tools-in-registry) regardless of reconnect count | Review round 5, finding 5 (P2) |
| `register_mcp_tools` field names | Template used `ts.wire_schema` + `ts.name`; actual fields are `ts.input_schema` + `ts.tool_name` ([`mcp_handle.rs:105-112`](../../coco-rs/core/tool-runtime/src/mcp_handle.rs)) | **Corrected to `ts.input_schema` / `ts.tool_name`** | Review round 5, finding 6 (P2) |

### Headline changes carried forward from v3.4

| Area | v3.3 | v3.4 | Status in v3.5 |
|---|---|---|---|
| AgentTool runtime schema fields | Included fictional `effort` + `model` fields | **Exactly the 10 fields in `AgentInput`** | unchanged |
| `derive_input_schema_value` visibility | "becomes `pub(crate)`" | **Stays `pub`** | unchanged |
| `require_root_type_object` array form | Accepted `["object", "null"]` | **Rejects array form entirely** | unchanged |
| `ToolRegistry::definitions(ctx)` handling | Not listed | **Deleted** (no production caller) | unchanged |
| `ToolId.as_str()` reference | Did not exist | **`tool_id.to_string()`** | unchanged |
| MCP report UI plumbing | Conflated SDK + TUI paths | **SDK-path only**, TUI follow-up | unchanged |

---

## Context

### Problems being solved

1. **Dynamic-schema garbage reaching the wire** ([`core/tool-runtime/src/traits.rs:480-491`](../../coco-rs/core/tool-runtime/src/traits.rs)).
   Tools whose `type Input = Value` fall through to the default `derive_input_schema_value::<Value>()`, which produces `{type:"null"}` or `anyOf` garbage; strict OpenAI-compatible providers respond 400 at the wire. We have already firefighted this twice in production (McpTool X3 fix `0303dc3ef2`; StructuredOutputTool P0 fix). The `debug_assert!` only trips in dev — release builds fail silently.

2. **Two schema assembly paths** (B2, corrected).
   - Production path `app/query/src/engine_prompt.rs:477-524` — through `&dyn DynTool`, calling `input_json_schema_for_session(...)`. Correct.
   - `services/inference/src/tool_schemas.rs::generate_tool_schemas:55` — **dead code**, grep across the workspace finds callers only in the colocated `.test.rs`. Leaving it around is bait for a future regression.

3. **Validator cache key does not include schema content** (B3).
   [`core/tool-runtime/src/schema.rs:135`](../../coco-rs/core/tool-runtime/src/schema.rs) keys the cache on `ToolId` only. After an MCP reconnect or a `register_mcp_tools` re-entry, stale validators stay live; `clear()` must be called by hand.

4. **The "must override" rule is a cultural convention.**
   The next Plugin / Custom Tool / HTTPTool / SDK-forwarded dynamic-schema tool will step on the same trap. The Rust type system can enforce this; a comment cannot.

5. **Late validation fails silently.**
   `ToolSchemaValidator::validate_collect` ([`schema.rs:191`](../../coco-rs/core/tool-runtime/src/schema.rs)) calls `jsonschema::validator_for(&schema)` on the **first validate call**, not at register time. An invalid schema makes it past registration; the first model call to that tool logs an error and **skips schema validation entirely** ([`tool_input_validate.rs:97-101`](../../coco-rs/app/query/src/tool_input_validate.rs)). The model appears to be able to call the tool, but its input is never validated.

6. **MCP reconnect path is not transactional.**
   `register_mcp_tools` ([`core/tools/src/lib.rs:129-149`](../../coco-rs/core/tools/src/lib.rs)) calls `registry.deregister_by_server(server_name)` then loops per-tool `registry.register(...)`. Registry methods at [`registry.rs:103`](../../coco-rs/core/tool-runtime/src/registry.rs) and [`registry.rs:258`](../../coco-rs/core/tool-runtime/src/registry.rs) each take an independent write lock — readers between iterations see a partial tool set.

### Non-goals

- Do **not** bind Input ↔ Schema at the type level (10 tools deliberately decouple their schema from the Input struct).
- Do **not** introduce a "three schema source kinds" trait + newtype abstraction layer.
- Do **not** introduce strict/lax sanitize modes — sanitize has no production use case (see [Why sanitize is gone](#why-sanitize-is-gone)).
- Do **not** rely on unstable associated-type defaults plus a `LegacyAdapter` bridge.
- Do **not** split into 6 phases / V2 naming suffixes / a separate Phase 6.
- Do **not** make Output schema share the Input contract (legal Output shapes include string / array / tagged union; no production consumer; out of scope).
- Do **not** run user-supplied or external-wire schemas through any lossy normalization.
- Do **not** narrow the **runtime validation schema** below what permission-rewriters / hooks may inject (e.g. AgentTool's `mcp_servers` must stay accepted by the validator).
- Do **not** widen the **model-facing schema** above what `AgentInput` deserializes (e.g. `effort` / `model` are not in the struct → must not appear in the model schema).
- Do **not** ship TUI-side display changes for MCP `skipped_tools` / `tombstoned_tools` in this PR (TUI uses an independent notification path — separate scope).
- Do **not** apply `additionalProperties: false` to **external** schemas (`McpTool` wire schema, `StructuredOutput` user schema). Closing those would silently reject valid third-party payloads; the field-honesty contract only applies to schemas coco-rs authors itself.

---

## Phase 1 findings (real distribution of tool schema sources)

### 1.1 Tool inventory — 42 statically registered + 2 dynamic

Source of truth: [`core/tools/src/lib.rs:28-85`](../../coco-rs/core/tools/src/lib.rs), 42 `registry.register(...)` calls in `register_all_tools`. Two additional tools (McpTool, StructuredOutputTool) are dynamically registered through `register_mcp_tools` / `register_structured_output_tool`, bringing the full production surface to **44**.

| Bucket | Count | Tools | Current shape |
|---|---|---|---|
| **A: derive-only** | **26** | Read/Write/Edit/Glob/Grep/NotebookEdit/ApplyPatch (7) · SendMessage/TeamCreate/TeamDelete (3) · PowerShell/REPL/Sleep (3) · CronCreate/CronDelete/CronList/RemoteTrigger (4) · EnterPlanMode/ExitPlanMode/VerifyPlanExecution/EnterWorktree/ExitWorktree (5) · ToolSearch/Config/Brief/Lsp (4) — total 26 | `type Input = TypedStruct`; no schema override |
| **B/C: override-input_schema** | **15** | Bash · WebFetch · WebSearch · AskUserQuestion · SkillTool (5) · TaskCreate/TaskGet/TaskList/TaskUpdate/TaskStop/TaskOutput/TodoWrite (7) · McpAuth/ListMcpResources/ReadMcpResource (3) — total 15 | Input is a typed struct; `input_schema()` overridden with hand-written Value or derive+mutate |
| **E: session-aware** | **1** | AgentTool | Overrides both `*_for_session` methods. Runtime validator accepts `mcp_servers` (hook-only) and `run_in_background`; model schema always omits `mcp_servers` and conditionally omits `run_in_background` |
| **Static registration subtotal** | **42** | | |
| **D: dynamic-wire** | **2** | McpTool/StructuredOutputTool | `type Input = Value`; registered via `register_mcp_tools` / `register_structured_output_tool` |
| **Full production surface** | **44** | | |

### 1.2 The truth about StructuredOutput today

`jsonschema::validator_for(&user_schema)` accepts top-level array / oneOf — it does not reject client-side. The provider paths diverge:

- **openai-compatible adapter wraps proactively** ([`vercel-ai/openai-compatible/src/chat/prepare_tools.rs:30-34`](../../coco-rs/vercel-ai/openai-compatible/src/chat/prepare_tools.rs)) coerces any non-object schema into `{"type":"object","properties":{}}` — lossy.
- **Anthropic adapter forwards verbatim** with no pre-check; top-level array gets 422/400 from Anthropic at runtime.

### 1.3 The DynTool surface already declares all four schema methods

[`traits.rs:281-285`](../../coco-rs/core/tool-runtime/src/traits.rs) lists four schema methods on DynTool; `:919-930` forwards them in the blanket impl. **All production callers go through `&dyn DynTool`.** Any new schema method must be added to DynTool + blanket impl.

### 1.4 Why sanitize is gone

[`codex-rs/tools/src/json_schema.rs:392-467`](../../codex-rs/tools/src/json_schema.rs) `sanitize_json_schema` adapts schemars output for OpenAI's strict-tool mode. Every transform is either a no-op against schemars 1.2 output (which never produces boolean / const / missing-type roots) or actively wrong against external/user contracts (would coerce `additionalProperties: true` to `{type:"string"}`).

**There is no schema shape in coco-rs where sanitize improves the outcome.** v3.4 does not port sanitize into the production path. All entries do `jsonschema::validator_for` meta-validation + strict root-type-object check; lossy normalization is rejected, not absorbed.

### 1.5 AgentTool's dual-track is by design

[`agent_tool.rs:38-79`](../../coco-rs/core/tools/src/tools/agent/agent_tool.rs) defines `AgentInput` with exactly **10 fields**. The struct doc comment (line 30-36) is explicit:

> "The model-facing schema is built by the manual `AgentTool::input_schema` override (TS-mirror with precise descriptions and enum lists). This struct only owns the runtime shape used by `AgentTool::execute` — adding fields here without adding them to `input_schema()` keeps them as internal-passthrough (e.g. `mcp_servers` is set by permission / hook rewrites, never by the model)."

The 10 fields:

| Field | Required | Model-visible | Notes |
|---|---|---|---|
| `prompt` | ✓ | ✓ | |
| `description` | ✓ | ✓ | |
| `subagent_type` | | ✓ | |
| `run_in_background` | | conditional | hidden when background tasks disabled / fork mode |
| `isolation` | | ✓ | |
| `name` | | ✓ | |
| `team_name` | | ✓ | |
| `mode` | | ✓ | |
| `cwd` | | ✓ | |
| `mcp_servers` | | **never** | hook-only injection |

[`agent_tool.rs:267-275`](../../coco-rs/core/tools/src/tools/agent/agent_tool.rs) further documents that `model` and `model_role` are operator-only knobs (set via `.md` frontmatter), **never LLM-pickable**:

> "Why neither is LLM-pickable: `model` requires knowledge of operator's `provider/model_id`"

v3.3 made the v3.4-corrected mistake of inventing `effort` and `model` fields in the AgentTool schema template. Neither exists in `AgentInput`; both would deserialize-and-silently-drop, creating a schema-honesty bug worse than v3.2's (the model would think it can set fields that simply vanish).

[`tool_call_preparer.rs:733-744`](../../coco-rs/app/query/src/tool_call_preparer.rs) revalidates input after hook rewrites. The runtime validation schema must therefore accept `mcp_servers` (hook injects it) and `run_in_background` (the runtime accepts it even when the model can't set it).

---

## Design

### Core type — one newtype, **two** constructor entries (owner = `coco-tool-runtime`)

```rust
// core/tool-runtime/src/schema.rs
// New owner. `coco_types::ToolInputSchema` is deleted outright — not renamed.

#[derive(Clone, Debug)]
pub struct ToolInputSchema { inner: Value }

impl ToolInputSchema {
    /// Bucket A entry — schemars-derived from `Self::Input`.
    /// Wraps `derive_input_schema_value::<T>()`, sets the field-honesty
    /// closure (`additionalProperties: false`) at the root, then validates
    /// via `from_value`.
    /// Bug ⇒ startup panic with a clear diagnostic.
    pub fn from_input_type<T: JsonSchema>() -> Self {
        let mut raw = derive_input_schema_value::<T>();
        if let Some(obj) = raw.as_object_mut() {
            obj.insert("additionalProperties".into(), Value::Bool(false));
        }
        Self::from_value(raw)
            .unwrap_or_else(|e| panic!(
                "schemars-derived schema for {} failed validation: {e}",
                std::any::type_name::<T>(),
            ))
    }

    /// Bucket B/C/E + Bucket D entry — programmer-written Value, derived+mutated
    /// Value (TodoWrite), external wire schema, or user `--json-schema`.
    ///
    /// Performs:
    ///   1. `jsonschema::validator_for` meta-validation
    ///   2. Strict root-type-object check (single-string `"object"` only;
    ///      array form like `["object", "null"]` is rejected because it would
    ///      let `null` inputs pass for dynamic Value tools)
    ///
    /// Does **not** sanitize, lower, or coerce. External / user schemas pass
    /// through verbatim; programmer-written schemas must already be well-formed.
    pub fn from_value(raw: Value) -> Result<Self, SchemaError> {
        jsonschema::validator_for(&raw)
            .map_err(|e| InvalidSchemaSnafu { message: e.to_string() }.build())?;
        require_root_type_object(&raw)?;
        Ok(Self { inner: raw })
    }

    pub fn as_value(&self) -> &Value { &self.inner }

    /// Returns a copy with `field` removed from both `properties` and `required`.
    /// If `required` becomes empty, the `required` key itself is removed.
    pub fn omit_property(&self, field: &str) -> Self {
        let mut value = self.inner.clone();
        if let Some(obj) = value.as_object_mut() {
            if let Some(props) = obj.get_mut("properties").and_then(Value::as_object_mut) {
                props.remove(field);
            }
            if let Some(required) = obj.get_mut("required").and_then(Value::as_array_mut) {
                required.retain(|v| v.as_str() != Some(field));
                if required.is_empty() {
                    obj.remove("required");
                }
            }
        }
        Self { inner: value }
    }

    /// Canonical JSON bytes (BTreeMap-ordered) — used by the validator
    /// cache to detect schema content changes across MCP reconnects
    /// without growing the cache map (see [Validator Cache Correctness]).
    pub fn canonical_bytes(&self) -> Arc<[u8]>;
}

// ===== derive helper stays pub =====

// core/tool-runtime/src/derive.rs
/// **Stays pub** in v3.4: TodoWrite (in `core/tools`, a separate crate)
/// uses this to derive-then-mutate a Value before constructing via
/// `ToolInputSchema::from_value`.
pub fn derive_input_schema_value<T: JsonSchema>() -> Value;

/// **Deleted** in v3.4: the old `derive_input_schema -> ToolInputSchema`
/// signature no longer compiles after the old `coco_types::ToolInputSchema`
/// is removed. Callers migrate to `from_input_type::<T>` (for the trivial case)
/// or `derive_input_schema_value` + mutate + `from_value` (for the mutate case).

// ===== Root-type check: STRICT single-string only (v3.4) =====

/// Rejects array-form `type: [...]` even if it contains `"object"`.
/// Reason: `type: ["object", "null"]` would let `null` inputs pass jsonschema
/// validation on Bucket D dynamic-Value tools and reach `execute(Value::Null, ...)`.
pub(crate) fn require_root_type_object(value: &Value) -> Result<(), SchemaError> {
    let obj = value.as_object().ok_or_else(|| RootNotObjectSnafu.build())?;
    match obj.get("type") {
        Some(Value::String(s)) if s == "object" => Ok(()),
        Some(Value::String(s)) if s == "null"   => Err(RootTypeNullSnafu.build()),
        _ => Err(RootTypeNotObjectSnafu.build()),
    }
}

// ===== SchemaError uses snafu + ErrorExt (tier 3, core/ crate) =====
use coco_error::{ErrorExt, StatusCode};
use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum SchemaError {
    #[snafu(display("schema root must be a JSON object (got non-object value)"))]
    RootNotObject { #[snafu(implicit)] location: snafu::Location },

    #[snafu(display(
        "schema root must declare type:\"object\" as a single string \
         (composition keywords like anyOf, and array forms like [\"object\",\"null\"], are rejected)"
    ))]
    RootTypeNotObject { #[snafu(implicit)] location: snafu::Location },

    #[snafu(display("schema root is the singleton null type"))]
    RootTypeNull { #[snafu(implicit)] location: snafu::Location },

    #[snafu(display("schema failed JSON Schema meta-validation: {message}"))]
    InvalidSchema { message: String, #[snafu(implicit)] location: snafu::Location },
}

impl ErrorExt for SchemaError {
    fn status_code(&self) -> StatusCode { StatusCode::InvalidArguments }
    fn is_retryable(&self) -> bool { false }
}
```

### Output schema path — untouched in this PR

Status quo retained: `Tool::output_schema(&self) -> Option<Value>` + `derive_output_schema::<T>()` default derive ([`derive.rs:103`](../../coco-rs/core/tool-runtime/src/derive.rs); no `_value` suffix on the output variant). Reasons:
- No production consumer across the workspace (tests only); symmetry would be empty symmetry.
- Output's legal shapes (string / array / tagged enum / arbitrary Value) do not match the "tool-parameter root object" invariant.

### Tool trait reshape

```rust
pub trait Tool: Send + Sync + 'static {
    type Input: for<'de> Deserialize<'de> + Send + Sync + 'static;
    //         ↑ Drop the JsonSchema bound.

    type Output: Serialize + for<'de> Deserialize<'de> + JsonSchema + Send + Sync + 'static;
    //         ↑ Keep the JsonSchema bound — Output path untouched.

    /// **Runtime validation schema** — always static, no session context.
    /// Enforced by `ToolSchemaValidator` on every tool call, including
    /// hook-rewritten inputs (see `tool_call_preparer.rs:733`).
    /// No default impl ⇒ forgetting this is E0046.
    ///
    /// **MUST be a superset of every `model_schema_for_session(ctx)` view.**
    /// Any field a hook / permission rewriter may inject must be accepted here.
    /// **MUST close the schema** with `"additionalProperties": false` for
    /// internally-built schemas (Bucket A/B/C/E). External schemas (Bucket D —
    /// `McpTool` wire, `StructuredOutput` user input) pass through verbatim
    /// and may keep their author's chosen `additionalProperties` policy.
    /// **MUST NOT contain properties absent from `Self::Input`** — otherwise
    /// the model can set fields that deserialize-and-silently-drop, creating
    /// a schema-honesty bug. (See v3.3 → v3.4 finding 1.)
    fn runtime_validation_schema(&self) -> &ToolInputSchema;

    /// **Model-facing schema** — what the LLM sees in its tool list.
    /// Default borrows the validation schema; tools with runtime-only
    /// hook-injected fields (AgentTool's `mcp_servers`, Bash's
    /// `_simulatedSedEdit`) override to omit those fields.
    ///
    /// **Invariants:**
    /// - Never wider (in property set) than `runtime_validation_schema()`.
    /// - For internally-built schemas, MUST carry `"additionalProperties": false`
    ///   so the model is told unambiguously which fields are settable.
    fn model_schema_for_session(&self, _ctx: &SchemaContext) -> Cow<'_, ToolInputSchema> {
        Cow::Borrowed(self.runtime_validation_schema())
    }

    /// Output schema — status quo retained.
    fn output_schema(&self) -> Option<Value> {
        Some(crate::derive::derive_output_schema::<Self::Output>())
    }

    // execute / render_for_model / everything else unchanged.
}
```

**Subset invariant** asserted by integration test (commit 5): `model_schema_for_session(ctx).properties ⊆ runtime_validation_schema().properties` for every tool × every SchemaContext.

**Field-honesty invariant** asserted by integration test, in **both directions**:

1. **Schema → Input**: every property name in `runtime_validation_schema().properties` exists as a field in `Self::Input` (or is documented as a hook-injected field — `mcp_servers` on AgentTool and `_simulatedSedEdit` on Bash are the only two qualifiers in v3.5).
2. **Input → Schema closure**: for internally-built schemas (Bucket A/B/C/E), `runtime_validation_schema().as_value()["additionalProperties"] == false`. This is what prevents the model from sending unknown fields that serde silently drops. External schemas (Bucket D) are excluded from this check by construction.
3. **Deserialize closure** (defense in depth): Bucket A `Input` structs carry `#[serde(deny_unknown_fields)]`. Without it the schema's `additionalProperties: false` is the only line of defense — a permission/hook rewrite path that bypasses the validator could still smuggle unknown fields past deserialize. With it, the deserialize layer rejects too. Bucket B/C/E Input structs may opt out when a runtime-only field would otherwise have to round-trip (e.g. `BashInput` keeps `_simulatedSedEdit`-accepting deserialize; the runtime schema declares the field instead).

**Deleted**:
- `Self::Input: JsonSchema` bound
- Default `fn input_schema()` derive path
- `fn input_schema()` (renamed to `runtime_validation_schema`)
- `fn input_schema_for_session()` (renamed to `model_schema_for_session`)
- `fn input_json_schema()` / `fn input_json_schema_for_session()` (folded into `Cow` view)
- `core/tool-runtime/src/derive.rs::derive_input_schema` (the `_value` variant stays `pub`; the no-suffix old wrapper is removed because its return type `coco_types::ToolInputSchema` is being deleted)
- `coco_types::ToolInputSchema { properties, required }` — entire type deleted (see [Schema Ownership](#schema-ownership))
- `ToolRegistry::definitions(&self, ctx: &ToolUseContext) -> Vec<(String, ToolInputSchema)>` — **deleted, no production caller** (workspace grep confirms; vercel-ai's `registry.definitions()` is on a different `ToolRegistry` type in `vercel-ai/ai`)

**Kept**:
- `Self::Output: JsonSchema` bound
- `derive_output_schema` (used by Output default derive)
- `derive_input_schema_value` **stays `pub`** so TodoWrite can derive-then-mutate from a sibling crate

### DynTool surface sync

```rust
#[async_trait::async_trait]
pub trait DynTool: Send + Sync + 'static {
    // ... everything else preserved
    fn runtime_validation_schema(&self) -> &ToolInputSchema;
    fn model_schema_for_session(&self, ctx: &SchemaContext) -> Cow<'_, ToolInputSchema>;
    // Removed: input_schema / input_schema_for_session / input_json_schema / input_json_schema_for_session
    // output_schema retained as-is.
}

impl<T: Tool> DynTool for T {
    fn runtime_validation_schema(&self) -> &ToolInputSchema {
        Tool::runtime_validation_schema(self)
    }
    fn model_schema_for_session(&self, ctx: &SchemaContext) -> Cow<'_, ToolInputSchema> {
        Tool::model_schema_for_session(self, ctx)
    }
}
```

### Consumer-side changes (exhaustive list)

| File:Line | Current | After |
|---|---|---|
| `app/query/src/engine_prompt.rs:494-499` | calls `tool.input_json_schema_for_session(&schema_ctx)` | `tool.model_schema_for_session(&schema_ctx).as_value().clone()` |
| `core/tool-runtime/src/schema.rs:135,191` (`ToolSchemaValidator::validate*`) | reads `effective_tool_schema(tool)`; cache key = `ToolId` | reads `tool.runtime_validation_schema()`; cache key = `schema_cache_key(tool)` content-addressed |
| `core/tool-runtime/src/registry.rs:242-247` (`ToolRegistry::definitions`) | returns `Vec<(String, coco_types::ToolInputSchema)>` | **method deleted** (no production caller) |
| `app/query/src/tool_input_validate.rs:94-104` (`SchemaCompileFailed` branch) | logs error, **skips schema validation and proceeds to permission check + `execute`** | **Fail-closed**: sets `tc.invalid = true` and `tc.invalid_reason = Some(ToolInputInvalidReason::InternalSchemaError { message })`; tool call is rejected to the model with a `<tool_use_error>` and never reaches `execute` |
| `app/query/src/tool_input_invalid.rs` (`ToolInputInvalidReason`) | enum with `SchemaViolation { message }` + `ParseError { message }` variants | adds `InternalSchemaError { message }` for the fail-closed branch above; user-facing rendering says "tool unavailable due to internal schema error" (does not leak internal details) |
| `core/tools/src/tools/task_tools.rs:1396-1419` (`TodoWriteTool::input_schema`) | overrides trait method, returns mutated `coco_types::ToolInputSchema` | moved into `TodoWriteTool::new()`; uses `derive_input_schema_value` (still `pub`) + mutate `enum` + `ToolInputSchema::from_value`; post-derive sets `additionalProperties: false` |
| `core/tools/src/tools/bash.rs:32-60` (`BashInput`) | accepts unknown fields by default; `_simulatedSedEdit` deserializes via `#[serde(rename = "_simulatedSedEdit")]` | unchanged at the struct (must keep accepting `_simulatedSedEdit` from upstream rewrite); the **runtime schema** now explicitly declares `_simulatedSedEdit` while the **model schema** omits it (mirrors AgentTool's `mcp_servers`) |
| `services/inference/src/tool_schemas.rs` (entire file) | dead module that imports the deleted `coco_types::ToolInputSchema` | **entire file deleted**, plus its `.test.rs` and the 6 `pub use` + 1 `mod` declaration in `services/inference/src/lib.rs:85-92` |
| `app/cli/src/sdk_server/handlers/mcp.rs:119`, `sdk_server/sdk_mcp.rs:74` | call `register_mcp_tools(rt, name, schemas)` (returns `()`) | consume `RegisterMcpToolsReport`; persist via `SdkServerState::record_mcp_registration_report(server, report)`; `handle_mcp_status` reads from this store + the registry tool count |
| `app/cli/src/sdk_server/handlers/mod.rs` (`SdkServerState`) | already holds `mcp_manager`, `session_runtime`, `transport` | **adds** `mcp_registration_reports: RwLock<HashMap<String, RegisterMcpToolsReport>>` (see [MCP Registration Atomicity](#mcp-registration-atomicity) for read/write protocol) |
| `app/cli/src/sdk_server/handlers/mcp.rs:24-71` (`handle_mcp_status`) | `tool_count = server.tools.len()` — count of MCP-**advertised** tools | `tool_count = registry.count_by_server(name)` — count of tools **actually registered** in `ToolRegistry`; `skipped_tools` + `tombstoned_tools` read from `SdkServerState::mcp_registration_reports` |
| `core/tool-runtime/src/registry.rs` | no `count_by_server` accessor | adds `pub fn count_by_server(&self, server_name: &str) -> i32` — single read lock, filters `tool.mcp_info()` by name |
| `common/types/src/server_request.rs:212` (`McpServerStatus`) | server-level `tool_count` + `error` only | adds `skipped_tools: Vec<McpSkippedToolStatus>` + `tombstoned_tools: Vec<String>`, both `#[serde(default, skip_serializing_if)]` so old clients are unaffected. `tool_count` semantics tightened: registered, not advertised |

**Out-of-scope consumers** (deliberately not touched in this PR):

| Path | Reason |
|---|---|
| `common/types/src/event.rs:983` (`McpStartupStatusParams { server, status }`) | TUI notification path — extending it requires TUI state struct + render changes; tracked as follow-up |
| `app/tui/src/state/session.rs:746` (TUI's `McpServerStatus { name, connected, tool_count }`) | TUI-side display struct; same follow-up |

---

## Tool migration templates (by bucket)

### Bucket A — 26 tools (mechanical)

```rust
// Before
#[derive(Deserialize, JsonSchema)]
pub struct ReadInput { /* fields */ }

pub struct ReadTool;
impl Tool for ReadTool {
    type Input = ReadInput;
    type Output = ReadOutput;
    // Default derive for input_schema and output_schema.
}

// After
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]   // ← deserialize-layer closure
pub struct ReadInput { /* fields */ }

pub struct ReadTool {
    schema: ToolInputSchema,
}
impl ReadTool {
    pub fn new() -> Self {
        // from_input_type<T> runs schemars derive, sets
        // `additionalProperties: false` at the root, then meta-validates.
        Self { schema: ToolInputSchema::from_input_type::<ReadInput>() }
    }
}
impl Default for ReadTool { fn default() -> Self { Self::new() } }
impl Tool for ReadTool {
    type Input = ReadInput;
    type Output = ReadOutput;
    fn runtime_validation_schema(&self) -> &ToolInputSchema { &self.schema }
    // model_schema_for_session uses the default (borrows the validation schema).
    // output_schema uses the default (derive_output_schema).
}
```

Registration: `registry.register(Arc::new(ReadTool::new()))`. Roughly +7 lines per tool plus the one-line `#[serde(deny_unknown_fields)]` on each Input struct; script-assisted.

**`from_input_type<T>` post-derive step.** schemars 1.2 does **not** emit `additionalProperties: false` for `Object`-typed roots by default. `from_input_type<T>` therefore sets it explicitly before calling `from_value`:

```rust
pub fn from_input_type<T: JsonSchema>() -> Self {
    let mut raw = derive_input_schema_value::<T>();
    if let Some(obj) = raw.as_object_mut() {
        obj.insert("additionalProperties".into(), Value::Bool(false));
    }
    Self::from_value(raw)
        .unwrap_or_else(|e| panic!(
            "schemars-derived schema for {} failed validation: {e}",
            std::any::type_name::<T>(),
        ))
}
```

### Bucket B/C — 15 tools

#### Pattern 1: hand-built schema (Bash dual-track, AskUserQuestion, MCP-management, Task* tools)

```rust
pub struct BashTool {
    runtime_schema: ToolInputSchema,
    /* existing fields */
}
impl BashTool {
    pub fn new(/* existing args */) -> Self {
        // Runtime schema includes _simulatedSedEdit — the TUI sed-edit
        // permission dialog injects this field into the tool input before
        // re-validation at tool_call_preparer.rs:733. Closing the schema
        // (`additionalProperties: false`) is what enforces field-honesty
        // for the model-facing view (omit_property derives that).
        let runtime_schema = ToolInputSchema::from_value(json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "command":                   { "type": "string",  "description": "..." },
                "timeout":                   { "type": "number",  "description": "..." },
                "description":               { "type": "string",  "description": "..." },
                "run_in_background":         { "type": "boolean", "description": "..." },
                "dangerouslyDisableSandbox": { "type": "boolean", "description": "..." },
                // Internal — TUI injects this after the sed-edit
                // permission dialog. Model view omits it.
                "_simulatedSedEdit": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "filePath":   { "type": "string" },
                        "newContent": { "type": "string" }
                    },
                    "required": ["filePath", "newContent"],
                    "description": "(internal) TUI-injected sed-edit payload"
                }
            },
            "required": ["command"]
        })).expect("BashTool schema must be a valid object schema");
        Self { runtime_schema, /* existing fields */ }
    }
}
impl Tool for BashTool {
    type Input = BashInput;

    fn runtime_validation_schema(&self) -> &ToolInputSchema {
        &self.runtime_schema   // wide: accepts _simulatedSedEdit
    }

    fn model_schema_for_session(&self, _ctx: &SchemaContext) -> Cow<'_, ToolInputSchema> {
        // Strip the upstream-only field from the model's view — same
        // pattern as AgentTool::mcp_servers.
        Cow::Owned(self.runtime_schema.omit_property("_simulatedSedEdit"))
    }
}
```

Note that `BashInput` keeps **without** `#[serde(deny_unknown_fields)]` — the dual-track pattern depends on the runtime schema explicitly declaring `_simulatedSedEdit`, and the deserialize layer must accept it for the rewrite path to land.

**WebFetch / WebSearch / SkillTool / AskUserQuestion / MCP-management / Task* tools** follow the same hand-built schema pattern minus the dual-track — those have no upstream-injected fields, so model and runtime schemas are identical (returned via the default `model_schema_for_session`) and both carry `additionalProperties: false`. Hand-built schema bodies preserved verbatim (no attempt to replace with schemars derive in this PR — would be unverified behaviour change).

#### Pattern 2: derive + mutate (TodoWrite)

```rust
pub struct TodoWriteTool { schema: ToolInputSchema }
impl TodoWriteTool {
    pub fn new() -> Self {
        // derive_input_schema_value stays pub in v3.4 — TodoWrite is the only
        // external-crate caller that needs the raw Value form for mutation.
        let mut raw = coco_tool_runtime::derive_input_schema_value::<TodoWriteInput>();
        // Inject the status enum constraint that schemars can't synthesize
        // from the `String` field type.
        if let Some(todos)  = raw.pointer_mut("/properties/todos")
            && let Some(items)  = todos.pointer_mut("/items")
            && let Some(props)  = items.pointer_mut("/properties")
            && let Some(status) = props.pointer_mut("/status")
            && let Some(obj)    = status.as_object_mut()
        {
            obj.insert("enum".into(),
                json!(["pending", "in_progress", "completed"]));
        }
        // Close the schema — derive-then-mutate path doesn't go through
        // from_input_type<T>, so set additionalProperties: false manually.
        if let Some(obj) = raw.as_object_mut() {
            obj.insert("additionalProperties".into(), Value::Bool(false));
        }
        let schema = ToolInputSchema::from_value(raw)
            .expect("TodoWrite schema must be a valid object schema");
        Self { schema }
    }
}
impl Default for TodoWriteTool { fn default() -> Self { Self::new() } }
impl Tool for TodoWriteTool {
    type Input = TodoWriteInput;
    fn runtime_validation_schema(&self) -> &ToolInputSchema { &self.schema }
}
```

### Bucket D — 2 tools (dynamically registered)

**McpTool**:

```rust
impl McpTool {
    pub fn new(wire_schema: Value, ...) -> Result<Self, SchemaError> {
        let schema = ToolInputSchema::from_value(wire_schema)?;
        Ok(Self { schema, ... })
    }
}
```

Wire schema preserved verbatim — no sanitize, no lowering. Rejection only on JSON-Schema-invalid input or non-single-`"object"` root (array form `["object", "null"]` rejected since v3.4).

**StructuredOutputTool**:

```rust
impl StructuredOutputTool {
    pub fn new(user_schema: Value) -> Result<Self, String> {
        let schema = ToolInputSchema::from_value(user_schema.clone())
            .map_err(|e| format!("--json-schema rejected: {e}"))?;
        let validator = jsonschema::validator_for(&user_schema)
            .map_err(|e| format!("invalid JSON schema: {e}"))?;
        Ok(Self { user_schema, validator: Arc::new(validator), schema })
    }
}
```

Invariant: `schema.as_value()` (shown to model) ≡ `user_schema` (runtime validator), byte-for-byte.

### Bucket E — 1 tool (AgentTool)

The runtime schema lists **exactly the 10 fields in `AgentInput`** ([`agent_tool.rs:38-79`](../../coco-rs/core/tools/src/tools/agent/agent_tool.rs)). No `effort`, no `model` — those are operator-only knobs (see [§1.5](#15-agenttools-dual-track-is-by-design)).

```rust
impl AgentTool {
    pub fn new() -> Self {
        // Runtime validation schema — includes ALL fields the validator must accept,
        // including mcp_servers (hook-injected) and run_in_background (runtime-accepted).
        // Field list MUST match AgentInput exactly.
        let schema = ToolInputSchema::from_value(json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "prompt":            { "type": "string", "description": "..." },
                "description":       { "type": "string", "description": "..." },
                "subagent_type":     { "type": "string", "description": "..." },
                "run_in_background": { "type": "boolean", "description": "..." },
                "isolation":         { "type": "string", "description": "..." },
                "name":              { "type": "string", "description": "..." },
                "team_name":         { "type": "string", "description": "..." },
                "mode":              { "type": "string", "description": "..." },
                "cwd":               { "type": "string", "description": "..." },
                "mcp_servers": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "(internal) permission/hook-injected"
                }
            },
            "required": ["description", "prompt"]
        })).expect("AgentTool schema must be a valid object schema");
        Self { schema, /* ... */ }
    }
}

impl Tool for AgentTool {
    type Input = AgentInput;

    fn runtime_validation_schema(&self) -> &ToolInputSchema {
        &self.schema  // wide: includes mcp_servers + run_in_background
    }

    fn model_schema_for_session(&self, ctx: &SchemaContext) -> Cow<'_, ToolInputSchema> {
        // Always omit mcp_servers from the model view (hook-only).
        let base = self.schema.omit_property("mcp_servers");
        // Conditionally omit run_in_background.
        let view = if ctx.background_tasks_disabled || ctx.fork_mode_active {
            base.omit_property("run_in_background")
        } else {
            base
        };
        Cow::Owned(view)
    }
}
```

The field-honesty integration test (commit 5) compares the property set of `runtime_validation_schema().as_value()["properties"]` against the set of `AgentInput`'s deserialize-recognized field names — they must be equal. This catches future regressions where someone adds a property to the schema without adding a struct field (or vice versa).

---

## Schema Ownership

The new owner of `ToolInputSchema` is **`coco-tool-runtime`**, not `coco-types`:

- **Delete**: `coco-rs/common/types/src/tool.rs::ToolInputSchema` (no re-export, no alias).
- **Create**: `coco-rs/core/tool-runtime/src/schema.rs::ToolInputSchema`.
- **Migrate consumers**: all `use coco_types::ToolInputSchema;` → `use coco_tool_runtime::ToolInputSchema;`.

Dependency direction:
- `coco-types` is L1 (no business-crate dependencies). The old `ToolInputSchema` lived in L1 but conceptually needed schemars output — a latent reverse dependency.
- `coco-tool-runtime` is L3 and depends on `coco-types` (L1) + `schemars` + `jsonschema` + `coco-error`. Natural direction.

`services/mcp-types::ToolInputSchema` is a different type (MCP protocol DTO), independent.

---

## Dead-code cleanup (full extent)

Delete in commit 5:

| File | Action |
|---|---|
| `services/inference/src/tool_schemas.rs` | Delete entire file |
| `services/inference/src/tool_schemas.test.rs` | Delete entire file |
| `services/inference/src/lib.rs` — `mod tool_schemas;` | Delete the `mod` declaration |
| `services/inference/src/lib.rs` — 6 `pub use tool_schemas::*` lines (`GeneratedSchemas`, `ToolSchemaOrigin`, `ToolSchemaSource`, `estimate_schema_tokens`, `filter_schemas_by_model`, `generate_tool_schemas`, `merge_tool_schemas`) | Delete each |
| `core/tool-runtime/src/registry.rs:242-247` (`ToolRegistry::definitions`) | Delete the method (no production caller) |
| `core/tool-runtime/src/derive.rs::derive_input_schema` (no-`_value` wrapper) | Delete; `derive_input_schema_value` stays |
| `core/tool-runtime/src/traits.rs` — `Tool::input_schema` / `input_schema_for_session` / `input_json_schema` / `input_json_schema_for_session` | Delete (replaced by `runtime_validation_schema` + `model_schema_for_session`) |
| `core/tool-runtime/src/traits.rs` blanket impl — same 4 methods | Delete |
| `common/types/src/tool.rs::ToolInputSchema` | Delete entire type |

Commit 5 runs `grep -rn "ToolInputSchema\|input_schema\|input_json_schema" coco-rs/` (workspace-wide) to catch any stragglers; expected hits should be limited to the new `coco_tool_runtime::ToolInputSchema` symbol.

---

## MCP Registration Atomicity

Current `register_mcp_tools` at [`core/tools/src/lib.rs:129-149`](../../coco-rs/core/tools/src/lib.rs) loops `deregister_by_server` then per-tool `register` — both methods take independent write locks ([`registry.rs:103`](../../coco-rs/core/tool-runtime/src/registry.rs), [`registry.rs:258`](../../coco-rs/core/tool-runtime/src/registry.rs)). Readers between iterations see a partial tool set.

### New atomic method on `ToolRegistry`

```rust
// core/tool-runtime/src/registry.rs
impl ToolRegistry {
    /// Atomically replace all tools belonging to a single MCP server.
    /// Single write lock for the entire diff swap.
    /// Returns the set of `ToolId`s that existed in the previous batch but
    /// not in `new_tools` (tombstones).
    pub fn replace_server_tools(
        &self,
        server_name: &str,
        new_tools: Vec<Arc<dyn DynTool>>,
    ) -> Vec<ToolId> /* tombstones */ {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());

        // 1. Diff old vs. new.
        let old_ids: HashSet<ToolId> = inner.tools.iter()
            .filter(|(_, t)| t.mcp_info().is_some_and(|i| i.server_name == server_name))
            .map(|(_, t)| t.id())
            .collect();
        let new_ids: HashSet<ToolId> = new_tools.iter().map(|t| t.id()).collect();
        let tombstones: Vec<ToolId> = old_ids.difference(&new_ids).cloned().collect();

        // 2. Wipe ALL server-owned aliases before any tool removal /
        //    re-registration. Without this, aliases owned by a retained
        //    tool whose alias set changed across reconnect would survive
        //    (v3.5 finding 4): if `mcp__srv__foo` advertised
        //    `{foo_v1}` last connect and `{foo_v2}` this connect, the
        //    `foo_v1 → mcp__srv__foo` alias would otherwise leak.
        let server_canonical_names: HashSet<String> = inner.tools.iter()
            .filter(|(_, t)| t.mcp_info().is_some_and(|i| i.server_name == server_name))
            .map(|(name, _)| name.clone())
            .collect();
        inner.aliases.retain(|_, canonical| !server_canonical_names.contains(canonical));

        // 3. Drop tombstoned tools (any remaining aliases already gone by step 2).
        for id in &tombstones {
            inner.remove_tool_by_id(id);
        }
        // 4. Register / overwrite the new batch — `register_with_aliases`
        //    re-establishes the alias set fresh from each new tool's
        //    `aliases()`, so retained tools end up with exactly their
        //    new advertised alias set, not the union of old + new.
        for tool in new_tools {
            inner.register_with_aliases(tool);
        }

        tombstones
    }
}
```

`inner.remove_tool_by_id` / `inner.register_with_aliases` are private helpers extracted from the existing `register` / `deregister_by_server` to share the alias-handling logic without re-acquiring the lock. The pre-wipe in step 2 is what closes finding 4 from review round 5 — `replace_server_tools` is the only call site of `register_with_aliases` that re-enters with retained-tool overwrites, and `register` itself is additive on aliases.

### Rewritten `register_mcp_tools`

```rust
// core/tools/src/lib.rs
pub fn register_mcp_tools(
    registry: &coco_tool_runtime::ToolRegistry,
    server_name: &str,
    mcp_tools: Vec<coco_tool_runtime::McpToolSchema>,
) -> RegisterMcpToolsReport {
    // 1. Per-tool validation — independent, doesn't block valid tools.
    //    Field names match `core/tool-runtime/src/mcp_handle.rs:105-112`:
    //    McpToolSchema { server_name, tool_name, description, input_schema, annotations }
    let mut valid = Vec::new();
    let mut skipped = Vec::new();
    for ts in mcp_tools {
        match McpTool::new(ts.input_schema.clone(), &ts.tool_name, ...) {
            Ok(tool) => valid.push(Arc::new(tool) as Arc<dyn DynTool>),
            Err(e)   => skipped.push(SkippedTool { name: ts.tool_name, error: e }),
        }
    }

    // 2. Single atomic swap.
    let registered: Vec<ToolId> = valid.iter().map(|t| t.id()).collect();
    let tombstones = registry.replace_server_tools(server_name, valid);

    RegisterMcpToolsReport { registered, skipped, tombstones }
}

pub struct RegisterMcpToolsReport {
    pub registered: Vec<ToolId>,
    pub skipped:    Vec<SkippedTool>,
    pub tombstones: Vec<ToolId>,
}
pub struct SkippedTool {
    pub name:  String,
    pub error: SchemaError,
}
```

### Caller plumbing — SDK protocol path only this PR

The two production call sites of `register_mcp_tools`:

- [`app/cli/src/sdk_server/handlers/mcp.rs:119`](../../coco-rs/app/cli/src/sdk_server/handlers/mcp.rs)
- [`app/cli/src/sdk_server/sdk_mcp.rs:74`](../../coco-rs/app/cli/src/sdk_server/sdk_mcp.rs)

Both **persist** the `RegisterMcpToolsReport` per server onto `SdkServerState`. `handle_mcp_status` reads the persisted report at query time — the registration call site no longer "populates" the status struct directly (v3.4 had no place to hold the report between registration and a later `mcp/status` poll).

#### New state on `SdkServerState`

```rust
// app/cli/src/sdk_server/handlers/mod.rs
pub struct SdkServerState {
    // ... existing fields (mcp_manager, session_runtime, transport, ...) ...

    /// Last `RegisterMcpToolsReport` per server. Written by both SDK
    /// `register_mcp_tools` call sites (`handlers/mcp.rs::register_server_tools`
    /// and `sdk_mcp.rs::register_and_connect`); read by `handle_mcp_status`.
    ///
    /// Lifecycle:
    /// - **First connect** or **reconnect**: full overwrite.
    /// - **Disconnect**: the entry is **removed** so `mcp/status` falls
    ///   back to empty `skipped_tools` / `tombstoned_tools` (matches the
    ///   "server gone" semantics — the registry has no tools for it either).
    pub mcp_registration_reports:
        tokio::sync::RwLock<HashMap<String, coco_tools::RegisterMcpToolsReport>>,
}

impl SdkServerState {
    pub async fn record_mcp_registration_report(
        &self,
        server_name: &str,
        report: coco_tools::RegisterMcpToolsReport,
    ) {
        let mut map = self.mcp_registration_reports.write().await;
        map.insert(server_name.to_string(), report);
    }

    pub async fn clear_mcp_registration_report(&self, server_name: &str) {
        let mut map = self.mcp_registration_reports.write().await;
        map.remove(server_name);
    }
}
```

Both call sites change to capture + persist:

```rust
// handlers/mcp.rs::register_server_tools (and the equivalent in sdk_mcp.rs)
async fn register_server_tools(
    ctx: &HandlerContext,
    server_name: &str,
    schemas: Vec<coco_tool_runtime::McpToolSchema>,
) {
    let rt_guard = ctx.state.session_runtime.read().await;
    let Some(rt) = rt_guard.as_ref() else { return };
    let report = coco_tools::register_mcp_tools(rt.tools(), server_name, schemas);
    ctx.state.record_mcp_registration_report(server_name, report).await;
}

// handlers/mcp.rs::deregister_server_tools
async fn deregister_server_tools(ctx: &HandlerContext, server_name: &str) {
    let rt_guard = ctx.state.session_runtime.read().await;
    if let Some(rt) = rt_guard.as_ref() {
        coco_tools::deregister_mcp_server(rt.tools(), server_name);
    }
    ctx.state.clear_mcp_registration_report(server_name).await;
}
```

#### Read path: `handle_mcp_status`

`handle_mcp_status` joins three sources: the manager's connection state (status / error), the registry's per-server tool count (registered, not advertised), and the persisted report (skipped / tombstoned):

```rust
let registry = /* via session_runtime */;
let reports = ctx.state.mcp_registration_reports.read().await;

for name in &names {
    let state = manager.get_state(name).await;
    let (status, error) = /* manager state mapping (unchanged) */;

    // Registry is the source of truth for `tool_count` — what the model
    // can actually call. The manager's `server.tools.len()` is the
    // advertised count and can exceed `tool_count` when some tools
    // were rejected at registration.
    let tool_count = registry
        .map(|r| r.count_by_server(name))
        .unwrap_or(0);

    let (skipped_tools, tombstoned_tools) = reports.get(name).map(|r| {
        let skipped = r.skipped.iter().map(|s| McpSkippedToolStatus {
            tool_name:   s.name.clone(),
            error:       s.error.to_string(),                  // ErrorExt::output_msg
            status_code: s.error.status_code().to_string(),
        }).collect();
        let tomb = r.tombstones.iter().map(|tid| tid.to_string()).collect();
        (skipped, tomb)
    }).unwrap_or_default();

    statuses.push(coco_types::McpServerStatus {
        name: name.clone(),
        status,
        tool_count,
        error,
        skipped_tools,
        tombstoned_tools,
    });
}
```

#### Extended DTO

```rust
// common/types/src/server_request.rs
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    pub name: String,
    pub status: McpConnectionStatus,
    #[serde(default)]
    pub tool_count: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    // v3.4 additions — opt-in via serde defaults, old clients unaffected:
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skipped_tools: Vec<McpSkippedToolStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tombstoned_tools: Vec<String>,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSkippedToolStatus {
    pub tool_name: String,
    pub error: String,        // human-readable rejection message
    pub status_code: String,  // StatusCode::InvalidArguments
}
```

`tool_count` semantics tighten in v3.5: it now reflects what's **actually in the registry** (via `ToolRegistry::count_by_server`), not the advertised count. Old clients see the same numeric field; the value is just no longer inflated by tools that failed `from_value`.

This **only** affects the SDK `mcp/status` reply — external SDK consumers (IDE, web client) see the extended fields plus the corrected `tool_count`.

### TUI display path — out of scope this PR

The TUI uses an independent notification path:

- `McpStartupStatusParams { server, status }` at [`common/types/src/event.rs:983`](../../coco-rs/common/types/src/event.rs) — no per-tool fields.
- TUI's local `McpServerStatus { name, connected, tool_count }` at [`app/tui/src/state/session.rs:746`](../../coco-rs/app/tui/src/state/session.rs).

Extending these requires:
1. New per-tool fields on `McpStartupStatusParams`,
2. New fields on the TUI state struct,
3. New render logic in the MCP status panel.

This is tracked as a follow-up (≈ +1 day). For this PR, TUI users see MCP registration failures via:
- `tracing::warn!` log lines at the SDK handler sites,
- The general status row (`server.status = Error`) when the entire batch fails.

---

## Validator Cache Correctness

Today `ToolSchemaValidator::cache` keys on `ToolId`, with no schema content in the key (`schema.rs:144` uses `entry().or_insert()` which is a no-op when a stale entry exists). After an MCP reconnect the same tool name may have a new wire schema; the stale validator still hits → silent validation against the wrong schema.

v3.5 keeps `ToolId` as the **only** key but stores the schema bytes alongside the cached validator, replacing the entry on content change. This gives the same correctness as a content-addressed key without the unbounded historical-version growth that `(ToolId, canonical_bytes)` would accumulate on long-running SDK servers (v3.4 finding 5):

```rust
// core/tool-runtime/src/schema.rs

/// Validator + the bytes it was compiled from. The bytes are kept
/// inline so a stale-schema lookup can be detected without re-running
/// `validator_for` on the new schema.
pub(crate) struct CachedValidator {
    pub schema_bytes: Arc<[u8]>,
    pub validator:    Arc<jsonschema::Validator>,
}

pub struct ToolSchemaValidator {
    cache: RwLock<HashMap<ToolId, CachedValidator>>,
}

/// Returns the canonical (BTreeMap-ordered) JSON bytes of the schema
/// the registry currently advertises for `tool`. Same input ⇒ same
/// bytes; any property add/remove/rename produces a different `Arc<[u8]>`.
fn current_schema_bytes(tool: &dyn DynTool) -> Arc<[u8]> {
    let value = tool.runtime_validation_schema().as_value();
    Arc::from(canonical_json_bytes(value).into_boxed_slice())
}

impl ToolSchemaValidator {
    fn get_or_compile(
        &self,
        tool: &dyn DynTool,
    ) -> Result<Arc<jsonschema::Validator>, SchemaValidationError> {
        let tool_id = tool.id();
        let want_bytes = current_schema_bytes(tool);

        // Fast path: read lock, byte-equal hit returns immediately.
        if let Some(entry) = self.cache.read().await.get(&tool_id)
            && Arc::ptr_eq(&entry.schema_bytes, &want_bytes)
                || *entry.schema_bytes == *want_bytes
        {
            return Ok(entry.validator.clone());
        }

        // Slow path: compile and replace any stale entry under the write
        // lock. Replacement is unconditional — a content change means the
        // old validator is wrong; we never grow the map beyond
        // `registry.len()` entries.
        let validator = Arc::new(
            jsonschema::validator_for(tool.runtime_validation_schema().as_value())
                .map_err(|e| SchemaValidationError::SchemaCompileFailed {
                    message: e.to_string(),
                })?,
        );
        let mut cache = self.cache.write().await;
        cache.insert(tool_id, CachedValidator {
            schema_bytes: want_bytes,
            validator:    validator.clone(),
        });
        Ok(validator)
    }
}
```

Both `validate` and `validate_collect` call `get_or_compile(tool)`. **One entry per `ToolId`, always reflecting the most-recent schema bytes** — the long-running SDK server case (frequent MCP reconnects with schema churn) stays bounded by `registry.len()`.

### `SchemaCompileFailed` is fail-closed, not skip-and-execute

v3.5 moves meta-validation to construction time. Every tool in the registry has already passed `jsonschema::validator_for` (in `from_input_type` or `from_value`), so the "compile error at first validate call" path should be unreachable in production.

The v3.4 plan handled the impossible-but-must-handle arm with `tracing::error!` + `debug_assert!`, which **falls through in release builds** and executes the tool with unvalidated input — exactly the bug the plan claimed to fix (v3.4 finding 3). v3.5 explicitly marks the call invalid so it surfaces as a `<tool_use_error>` to the model and never reaches `execute`:

```rust
match validator.validate_collect(tool.as_ref(), &tc.input).await {
    Ok(Ok(())) => { /* clean */ }
    Ok(Err(issues)) => {
        let message = format_schema_error(&tc.tool_name, &issues);
        tc.invalid = true;
        tc.invalid_reason = Some(ToolInputInvalidReason::SchemaViolation { message });
    }
    Err(SchemaValidationError::SchemaCompileFailed { message }) => {
        // Registration-time meta-validation should make this unreachable,
        // but in production we fail CLOSED rather than skip-and-execute.
        tracing::error!(
            target: "coco_query::tool_input",
            tool = %tc.tool_name,
            %message,
            "tool schema failed to compile at validate site — failing the tool call closed",
        );
        tc.invalid = true;
        tc.invalid_reason = Some(ToolInputInvalidReason::InternalSchemaError {
            // Generic user-facing message — does not leak internals.
            message: format!(
                "tool `{}` is currently unavailable due to an internal schema error",
                tc.tool_name,
            ),
        });
    }
    Err(SchemaValidationError::Rejected { .. }) => {
        // Same fail-closed treatment — `Rejected` is also a compile-path
        // signal from `validate_collect`, not a normal validation result.
        tracing::error!(
            target: "coco_query::tool_input",
            tool = %tc.tool_name,
            "unexpected Rejected from validate_collect",
        );
        tc.invalid = true;
        tc.invalid_reason = Some(ToolInputInvalidReason::InternalSchemaError {
            message: format!(
                "tool `{}` is currently unavailable due to an internal schema error",
                tc.tool_name,
            ),
        });
    }
}
```

This matches the design intent: a tool whose schema can't be compiled at the validate site is broken from the model's perspective, and the model should be told so it can choose a different approach — silently executing with no validation is the worst outcome.

---

## Implementation sequence (single PR, 5 logical commits)

| Commit | Contents | Estimate |
|---|---|---|
| 1 | `ToolInputSchema` newtype (2 entries: `from_input_type<T>` sets `additionalProperties: false` post-derive + `from_value`); strict `require_root_type_object` (single-string `"object"` only, no array form); `omit_property` with required-list invariant; `canonical_bytes`; `SchemaError` (snafu + `ErrorExt` + `StatusCode::InvalidArguments`); unit tests including (a) rejects `type: ["object","null"]`, (b) `from_input_type<T>` emits `additionalProperties: false`. | 4h |
| 2 | Tool / DynTool trait reshape (rename to `runtime_validation_schema` / `model_schema_for_session`; remove old 4 schema methods + `Self::Input: JsonSchema` bound) + blanket impl forward + delete `coco_types::ToolInputSchema` entirely + delete `ToolRegistry::definitions` (no production caller) + add `ToolRegistry::replace_server_tools` atomic method (with the **wipe-server-aliases-first** step per v3.5 finding 4) + add `ToolRegistry::count_by_server` accessor + extract `remove_tool_by_id` / `register_with_aliases` private helpers | 4h |
| 3 | Migrate the 26 Bucket A tools (script-assisted; ~7 lines per tool plus `#[serde(deny_unknown_fields)]` on each Input struct) + registry count test (asserts `register_all_tools` registers 42 tools, every tool's `runtime_validation_schema().as_value()` has `type:"object"`, and every Bucket-A schema has `additionalProperties: false`) | 4h |
| 4 | Migrate 15 Bucket B/C + 2 Bucket D + 1 Bucket E tools (18 total). **AgentTool**: schema lists the exact 10 fields of `AgentInput` (no fictional fields) with `additionalProperties: false`; `model_schema_for_session` always omits `mcp_servers` and conditionally omits `run_in_background`. **BashTool**: dual-track — runtime schema declares `_simulatedSedEdit`; `model_schema_for_session` omits it via `omit_property`. **TodoWrite**: derive-then-mutate via the still-`pub` `derive_input_schema_value` + post-mutate set `additionalProperties: false` + `from_value`. `register_mcp_tools` rewritten to use `replace_server_tools` + return `RegisterMcpToolsReport` (correct field names `ts.input_schema` / `ts.tool_name`). StructuredOutput verbatim user schema via `from_value`. | 5h |
| 5 | Update consumers per the [Consumer-side changes](#consumer-side-changes-exhaustive-list) table: `engine_prompt` uses `model_schema_for_session`; `tool_input_validate` uses `runtime_validation_schema` + **fail-closes `SchemaCompileFailed`** via new `ToolInputInvalidReason::InternalSchemaError` (no skip-and-execute); `ToolSchemaValidator::cache` becomes `HashMap<ToolId, CachedValidator { schema_bytes, validator }>` with replace-on-content-change (no unbounded growth). **Delete `services/inference/src/tool_schemas.rs` entirely** + `.test.rs` + 6 `pub use` + `mod` declaration. Extend `McpServerStatus` (in `coco-types`) with `skipped_tools` + `tombstoned_tools`. Add `SdkServerState::mcp_registration_reports` store. Thread `RegisterMcpToolsReport` write into both SDK server handlers; `handle_mcp_status` reads from the store + `registry.count_by_server`. Integration test that asserts (a) registry count = 42, (b) every tool's runtime schema is root-`"object"` with `additionalProperties: false` (Bucket A/B/C/E only), (c) every tool's `model_schema_for_session` properties ⊆ `runtime_validation_schema` properties, (d) every property in AgentTool's runtime schema corresponds to a field in `AgentInput`, (e) BashTool's runtime schema declares `_simulatedSedEdit` but the model schema omits it, (f) `SchemaCompileFailed` arm sets `tc.invalid = true` (regression test using a deliberately-broken dynamic tool). | 5h |
| **Total** | | **~3 days** |

---

## Verification

```bash
cd coco-rs
just quick-check                     # compile + clippy + check-error-policy
just test-crate coco-tool-runtime    # ToolInputSchema unit tests + atomic swap test
just test-crate coco-tools           # all 42 tools construct without panicking; bucket count asserted
just test-crate coco-query           # engine_prompt path runs end-to-end
just pre-commit                      # full sweep (run last, once)
```

Manual checks:

- `cargo run -- --json-schema '{"type":"array",...}' --print "..."` fails cleanly at startup with `--json-schema rejected: schema root must declare type:"object" ...`.
- `cargo run -- --json-schema '{"type":["object","null"],"properties":{}}' --print "..."` **also fails** (v3.4 strict check rejects array form).
- `cargo run -- --json-schema '{"additionalProperties": true, "type":"object", ...}' --print "..."` passes; model receives the schema with `additionalProperties: true` intact.
- Stand up an MCP server and exercise four scenarios:
  - First connect with mix of valid+invalid tools → valid tools register, `skipped_tools` populated in SDK `mcp/status`, no tombstones.
  - Reconnect with a tool removed server-side → `tombstoned_tools` populated; remaining tools still callable across the swap (concurrent-reader test asserts no partial state observable).
  - Reconnect with schema-changed tool → validator cache misses (new `canonical_bytes`), fresh validator compiles.
  - Reconnect with every tool invalid → all tombstoned in one swap.
- **AgentTool hook-rewrite path**: configure a hook that injects `mcp_servers`; confirm:
  - Model schema does NOT show `mcp_servers` (assert `model_schema_for_session(default ctx).as_value()["properties"]` has no `mcp_servers`).
  - Runtime validator DOES accept hook-rewritten input containing `mcp_servers` (`tool_call_preparer.rs:733` revalidation passes).
  - Model-facing schema serializes `"additionalProperties": false`.
- **BashTool sed-edit rewrite path**: trigger the TUI sed-edit permission dialog (`SedEditPermissionRequest`); confirm:
  - Model never sees `_simulatedSedEdit` in the tool list (assert `model_schema_for_session(...)["properties"]` does NOT contain `_simulatedSedEdit`; both views contain `"additionalProperties": false`).
  - Runtime validator accepts the TUI-rewritten input that contains `_simulatedSedEdit` (revalidation at `tool_call_preparer.rs:733` passes; `BashTool::execute` short-circuits via the sed-edit path).
- **AgentTool field honesty**: feed the model the AgentTool definition; assert the model can only call AgentTool with fields from `AgentInput` (no `effort`, no `model`). A negative test sends a tool_use call with an unknown field — assert the schema validator rejects it via `additionalProperties: false`, not a silent serde drop.
- Disable background tasks, invoke AgentTool, confirm the model schema does not include `run_in_background` (also not in `required`).
- **Fail-closed validator regression test**: register a `Tool` whose `runtime_validation_schema()` returns a hand-crafted `ToolInputSchema` whose **canonical_bytes** would still meta-validate but whose stored validator was poisoned at slow-path compile time (forced via a test seam); assert that a tool_use call against it surfaces a `<tool_use_error>` to the model and `execute` is **never** entered. Asserts the v3.5 finding 3 fix.
- **Validator cache bounded growth**: register an MCP server with a tool; trigger 100 reconnects, each with a content-changed schema; assert `ToolSchemaValidator::cache.len() == registry.len()` after the loop (not 100×).
- **MCP report storage round-trip**: reconnect a server whose tools partition into 3 valid / 2 invalid / 1 tombstone; assert `mcp/status.tool_count == 3` (the registry count, not the advertised 5) and that `skipped_tools.len() == 2` + `tombstoned_tools.len() == 1`. Disconnect; assert the entry is cleared and `mcp/status` returns empty `skipped_tools` / `tombstoned_tools` for that server.
- **Replace-server-tools alias hygiene**: register a server with tool `mcp__srv__foo` advertising alias `foo_v1`; reconnect with the same canonical name advertising alias `foo_v2`; assert that `registry.get_by_name("foo_v1") == None` and `registry.get_by_name("foo_v2") == Some(...)` after the swap, all under a single write lock (the swap-atomicity test asserts this via a concurrent reader).
- Open `coco-error` log; `SchemaError` variants show up with snafu Location and `StatusCode::InvalidArguments`.

---

## Risks and mitigations

| Risk | Mitigation |
|---|---|
| Touching 42 tools in one go makes review heavy | Script-assisted Bucket A (26); commit-by-bucket; commit 3 lands the registry count assertion (missed tools become test failures, not silent compile breaks) |
| `Cow<'_, ToolInputSchema>` lifetime on a dyn trait | Confirmed object-safe |
| `from_input_type::<T>()` panic at startup is opaque | Panic message carries the failing input type name + meta-validation / root-type error |
| User-supplied non-object schema for `--json-schema` now fails at startup | User-visible behaviour change; clearer than current silent wrap / runtime 422; release-note item |
| Strict rejection of `type: ["object","null"]` may surprise authors who used the array form | Error message points at the offending root type; `from_input_type<T>` for typed Input always passes (schemars emits single-string `"object"`); only hand-written schemas can hit this — and the change is the right thing because the alternative is silent null-input acceptance |
| MCP server author hits a `SchemaError` rejection where today the openai-compat lower silently wraps | `RegisterMcpToolsReport.skipped` surfaces through SDK `McpServerStatus`; warn log carries the same info |
| AgentTool field-honesty regression (someone adds a property to the schema without adding a struct field, or vice versa) | Integration test asserts equality of the two property sets + `additionalProperties: false` on the root |
| `additionalProperties: false` rejects a payload that worked before (e.g. SDK clients sending `x-trace-id` alongside tool input) | Release-note item; runtime fields hidden from the model are explicitly listed in the runtime schema (currently only `mcp_servers` and `_simulatedSedEdit`); any other use case should add the field to `Self::Input` or document a new runtime-only field |
| Bash dual-track regression: schema and Input drift apart (someone adds a hook-injected field to `BashInput` without declaring it in the runtime schema) | Field-honesty test enforces equality between runtime-schema properties and the union of `Self::Input` deserialize-recognised fields; CI catches drift |
| `BashInput` keeps no `#[serde(deny_unknown_fields)]`, weakening the deserialize closure | Acceptable trade-off: the runtime schema's `additionalProperties: false` provides the closure at the validator layer; `BashInput`'s open serde is what lets the upstream-injected `_simulatedSedEdit` round-trip. Documented in the Bash template. |
| Validator cache stale after MCP reconnect | `HashMap<ToolId, CachedValidator { schema_bytes, validator }>` with replace-on-content-change — bounded size, content-correct |
| `SchemaCompileFailed` fail-closed surprises a downstream consumer that depended on skip-and-execute | The skip-and-execute behaviour was a documented bug; surfacing as `InternalSchemaError` to the model is the correct signal. Worst case is a tool becomes unreachable until restart — the registration-time meta-validation makes this unreachable in practice. |
| TUI users see MCP `skipped` / `tombstoned` only via logs in this PR | Documented as follow-up; SDK consumers (IDE / web) get the rich surface immediately |
| `replace_server_tools` private helpers diverge from `register` / `deregister_by_server` | Extract helpers in the same commit; swap-atomicity test under contention; alias-hygiene regression test (v3.5) covers retained-tool alias churn |
| `McpServerStatus.tool_count` semantics tighten (advertised → registered) — old dashboards may show different numbers | Release-note item; the new value is the *correct* count (what the model can call); any consumer counting on advertised should use `manager.get_state(name).tools.len()` directly |
| `SdkServerState::mcp_registration_reports` map grows across many distinct server registrations | Bounded by `len(registered MCP servers)`; `clear_mcp_registration_report` removes on disconnect; no per-tool persistence — only the last report per server |
| `McpServerStatus` DTO change affects SDK consumers | New fields `#[serde(default, skip_serializing_if)]` → old clients see same shape |

---

## Decision Log

| Decision | Choice | Rationale |
|---|---|---|
| Schema vs Input type binding | **Decoupled** | 10 tools are counter-examples; v1 death |
| Schema source abstraction layer | **None** (one newtype + two entries) | Bucket distribution shows trait + multi-newtype is over-engineering |
| Sanitize integration | **Removed from production path** | Every transform is no-op against schemars or actively wrong against external/user contracts |
| Constructor entries | **Two** (`from_input_type<T>` panicking, `from_value` returning Result) | TodoWrite needs `from_value` for derive-then-mutate; `from_input_type` is a convenience wrapper |
| Meta-validation site | **All constructor entries** | Catches invalid schemas at registration; `SchemaCompileFailed` becomes unreachable in production |
| Root-type check | **Strict single-string `"object"`** (v3.4) | Array form `["object","null"]` would let null inputs reach `execute(Value::Null,...)` for dynamic Value tools |
| `SchemaError` variants | **Four** (`RootNotObject` / `RootTypeNotObject` / `RootTypeNull` / `InvalidSchema`) | Snafu-derived |
| `SchemaError` library | **snafu + `coco-error` + `ErrorExt`** | `core/*` crates use tier 3 per `CLAUDE.md` |
| `StatusCode` for `SchemaError` | **`InvalidArguments`** | Schema shape mismatch |
| Default impl for `runtime_validation_schema` | **Removed** | Type-level enforcement; E0046 surfaces missed tools at compile time |
| `Self::Input: JsonSchema` bound | **Removed** | `Value` no longer "passes" the default |
| `Self::Output: JsonSchema` bound | **Kept** | Output path untouched |
| Output schema path | **Not done this round** | No production consumer |
| StructuredOutput user contract | **Verbatim via `from_value`** | External/user schemas preserved byte-for-byte |
| Schema ownership | **`coco-tool-runtime`** (delete `coco_types::ToolInputSchema`) | New type calls derive + jsonschema; coco-types (L1) must not reverse-depend on L3 |
| DynTool surface sync | **Explicit sync** (trait + blanket) | Production callers all go through dyn |
| Trait method naming | **`runtime_validation_schema` + `model_schema_for_session`** | Names encode the subset invariant |
| Subset invariant: `model ⊆ runtime` | **Encoded in trait doc + integration test** | AgentTool `mcp_servers` is the canonical example |
| **Field-honesty invariant: `runtime` ⊆ `Self::Input` fields** (v3.4) | **Encoded in trait doc + integration test** | v3.3 invented fictional `effort` + `model` on AgentTool; v3.4 prevents recurrence |
| **Field-honesty closure: `additionalProperties: false`** (v3.5) | **Required on all internally-built schemas (Bucket A/B/C/E); excluded for Bucket D** | v3.4's one-way invariant left a hole — model could send unknown fields that serde silently dropped. Closing the schema enforces field-honesty for the model's view; external schemas stay verbatim so MCP / `--json-schema` aren't silently rejected |
| **Bucket A `#[serde(deny_unknown_fields)]`** (v3.5) | **Required on Bucket A `Input` structs (defense in depth at the deserialize layer)** | Without it, the runtime schema's `additionalProperties: false` is the only line of defense — a permission/hook rewrite path that bypasses the validator could smuggle unknown fields through. Bucket B/C/E may opt out per-tool when a runtime-only field needs to round-trip (Bash dual-track) |
| **Bash `_simulatedSedEdit` handling** (v3.5) | **Dual-track**: runtime declares it; model omits it via `omit_property` | Parallels AgentTool's `mcp_servers`. v3.4 didn't address this and adding `additionalProperties: false` blindly would have broken the TUI sed-edit rewrite |
| AgentTool `mcp_servers` | **In runtime schema, omitted from model schema** | Hook/permission rewrites inject this field; runtime must accept it |
| AgentTool `effort` / `model` fields | **NEVER in any schema** (v3.4) | Not in `AgentInput`; operator-only knobs (`.md` frontmatter) per `agent_tool.rs:267-275` |
| `omit_property` invariant | **Removes from both `properties` and `required`; drops empty `required`** | Matches existing AgentTool TS-parity; unit-tested |
| `derive_input_schema_value` visibility (v3.4) | **Stays `pub`** | TodoWrite (sibling crate `core/tools`) needs raw Value form for derive-then-mutate |
| `derive_input_schema` (no-`_value` wrapper) | **Deleted** | Returned the deleted `coco_types::ToolInputSchema`; no longer compiles |
| `ToolRegistry::definitions` (v3.4) | **Deleted** | Workspace grep confirms no production caller; vercel-ai's `registry.definitions()` is on its own `ToolRegistry` type |
| `ToolRegistry::count_by_server` (v3.5) | **Added** — used by `handle_mcp_status` to source `tool_count` from the registry, not the manager | Closes the misleading "advertised but not registered" gap when some MCP tools fail `from_value` |
| `ToolId` wire string in MCP report (v3.4) | **`tool_id.to_string()`** via `Display` impl | `ToolId::as_str` does not exist; `Display` is the canonical wire serialization |
| `McpToolSchema` field names in `register_mcp_tools` (v3.5) | **`ts.input_schema` + `ts.tool_name`** (verified at `mcp_handle.rs:105-112`) | v3.4 template used `ts.wire_schema` / `ts.name` — neither field exists; v3.5 corrects |
| `replace_server_tools` alias hygiene (v3.5) | **Wipe all server-owned aliases BEFORE re-registering** | Without the pre-wipe, retained tools whose alias set changed across reconnect leak old aliases; v3.4 only removed tombstoned IDs' aliases |
| `ToolSchemaValidator::cache` key (v3.5) | **`HashMap<ToolId, CachedValidator { schema_bytes, validator }>` with replace-on-content-change** | v3.4's `(ToolId, canonical_bytes)` key gave correctness but unbounded growth across reconnects; v3.5 is bounded by `registry.len()` |
| `SchemaCompileFailed` validation branch (v3.5) | **Fail-closed via `ToolInputInvalidReason::InternalSchemaError`** — never reach `execute` | v3.4's defensive `unreachable!() + debug_assert!` fell through in release builds; v3.5 truly closes the path |
| **MCP `RegisterMcpToolsReport` storage** (v3.5) | **`SdkServerState::mcp_registration_reports: RwLock<HashMap<String, RegisterMcpToolsReport>>`** | v3.4 said handlers "populate" `McpServerStatus` but `register_mcp_tools` returned `()` and `mcp/status` reads `McpConnectionManager` only — no storage seam existed |
| `McpServerStatus.tool_count` semantics (v3.5) | **Registered count** (`ToolRegistry::count_by_server`), not advertised | Aligns the wire field with what the model can actually call |
| Phase count | **One PR + 5 commits** | Project rule "no backward-compat" |
| LegacyAdapter / V2 naming suffix | **Avoided** | Conflicts with the rule; rust 1.93.1 doesn't support associated-type defaults |
| Phase 0 cross-crate calls | **Avoided**; delete dead code | services/inference stays tool-agnostic |
| Validator cache key | **`(ToolId, canonical_bytes)`** content-addressed | u64 hash collision → wrong validator silently passes invalid input |
| Validator cache key computation site | **Shared `schema_cache_key(tool)`** | `validate` / `validate_collect` share path |
| MCP registration error handling | **Partition + atomic diff swap** with `RegisterMcpToolsReport` | All-or-nothing preserved stale schemas; non-atomic diff leaked partial state |
| MCP registration atomicity | **Single `ToolRegistry::replace_server_tools` method, one write lock** | v3.2 pseudo-code re-acquired locks per iteration |
| MCP report surfacing site (v3.4) | **SDK protocol path only this PR**; TUI path follow-up | TUI uses independent `McpStartupStatusParams` + `tui::state::session::McpServerStatus`; extending those is out of scope |
| `McpServerStatus` extension | **Add `skipped_tools` + `tombstoned_tools`, both serde-default optional** | Forward-compatible with old SDK clients |
| Dead-code cleanup | **Delete `tool_schemas.rs` module + 6 `pub use` + `mod` + `ToolRegistry::definitions` + `derive_input_schema` wrapper** | v3.3 was incomplete; v3.4 is exhaustive |
| Output helper name | **`derive_output_schema`** (no `_value` suffix) | Actual name in `derive.rs:103` |
| `SchemaCompileFailed` validation branch | **Defensive `unreachable!()` + tracing** | Meta-validation at registration makes this dead in production |

---

## Out of Scope

- **Output schema path refactor**. Status quo retained. Triggers: (a) `type Output = Value` tool appears; (b) strict provider introduces output_schema strict validation; (c) Output schema gains production consumer.
- **Route StructuredOutput through provider-native `response_format`** (separate initiative).
- **AgentTool wire/runtime Input split** (separate initiative). Dual-track surfaced via `runtime_validation_schema` + `model_schema_for_session`.
- **Plugin / Custom Tool framework** (`ToolInputSchema::from_value` is the ready entry point).
- **Provider-side schema lowering** (e.g. OpenAI strict-tool mode lowering anyOf). Lives in the provider crate.
- **WebFetch/WebSearch derive-only migration** (replacing hand-written `input_schema()` with `from_input_type::<T>`). Would change model-facing schema in unverified ways.
- **TUI MCP status display of `skipped_tools` / `tombstoned_tools`** (v3.4). Requires extending `McpStartupStatusParams` + TUI state + render; +1 day follow-up.

---

## Revision Log

### v3.5 — fifth external review (6 findings)

| # | Finding | Severity | Revision |
|---|---|---|---|
| 1 | Field-honesty invariant only enforced one direction; no `additionalProperties` policy specified → model could send unknown fields that serde silently drops. Closing schemas requires dual-track for Bash's `_simulatedSedEdit`, not just AgentTool's `mcp_servers`. | **P1** | All internally-built schemas now declare `additionalProperties: false`; `from_input_type<T>` sets it post-derive; Bucket A `Input` structs gain `#[serde(deny_unknown_fields)]`; BashTool gains the dual-track pattern; field-honesty integration test asserts the closure on both runtime and model schemas |
| 2 | `register_mcp_tools` returned `()` and `mcp/status` read `McpConnectionManager` only — no place to hold `skipped_tools` / `tombstoned_tools` between registration and the next status poll; `tool_count` reflected advertised, not registered | **P1** | New `SdkServerState::mcp_registration_reports` keyed on `server_name`; both SDK call sites write; `handle_mcp_status` reads; `ToolRegistry::count_by_server` sources the corrected `tool_count` |
| 3 | `SchemaCompileFailed` arm used `tracing::error!` + `debug_assert!` — release builds fell through and executed tool with unvalidated input | **P1** | New `ToolInputInvalidReason::InternalSchemaError`; the arm sets `tc.invalid = true` so the call surfaces as `<tool_use_error>` to the model and never reaches `execute` |
| 4 | `replace_server_tools` removed tombstoned IDs only; aliases owned by retained tools whose alias set changed across reconnect survived in `inner.aliases` | P2 | Wipe all server-owned aliases before re-registering — `register_with_aliases` re-establishes from each new tool's `aliases()` |
| 5 | `(ToolId, canonical_bytes)` cache key gave correctness but unbounded growth across MCP reconnects with schema churn | P2 | `HashMap<ToolId, CachedValidator { schema_bytes, validator }>` with replace-on-content-change — bounded by `registry.len()` |
| 6 | `register_mcp_tools` template used `ts.wire_schema` / `ts.name`; actual fields are `ts.input_schema` / `ts.tool_name` ([`mcp_handle.rs:105-112`](../../coco-rs/core/tool-runtime/src/mcp_handle.rs)) | P2 | Corrected — the v3.4 round-4 "every field name re-verified" claim missed this site |

### v3.4 — fourth external review (6 findings)

| # | Finding | Severity | Revision |
|---|---|---|---|
| 1 | AgentTool template included fictional `effort` + `model` fields not in `AgentInput`; would create silent serde-drop schema-honesty bug | **P0** | Schema lists exactly the 10 fields of `AgentInput`; added field-honesty invariant + integration test |
| 2 | `derive_input_schema_value` marked `pub(crate)` but TodoWrite (sibling crate) needs it for derive-then-mutate | P1 | Keep `pub`; only the old no-`_value` wrapper is deleted |
| 3 | `require_root_type_object` accepted `type: ["object","null"]` → null inputs would pass for dynamic Value tools | P1 | Strict single-string `"object"` only; array form rejected |
| 4 | `ToolRegistry::definitions(ctx)` not listed in consumer-side changes; would compile-fail | P1 | Workspace grep confirmed no production caller (vercel-ai's `registry.definitions()` is a different type); method deleted |
| 5 | Plan used `ToolId.as_str()`; method doesn't exist (`Display` is the wire serialization) | P2 | `tool_id.to_string()` |
| 6 | "SDK protocol carries to UI naturally" conflated SDK `McpServerStatus` with TUI's independent `McpStartupStatusParams` path | P2 | Made SDK-path-only explicit; TUI display listed as follow-up |

### v3.3 — third external review (6 findings, preserved for context)

| # | Finding | Severity | Revision |
|---|---|---|---|
| 1 | Bucket A listed 28 but counted as 26; 44 ≠ 42 | P1 | WebFetch/WebSearch back in B/C; A=26 / B/C=15 / E=1 / D=2 |
| 2 | Pseudo-code looped deregister/register across independent locks | P1 | `ToolRegistry::replace_server_tools` single-write-lock method |
| 3 | Deleting only `generate_tool_schemas` left stale imports/re-exports | P1 | Delete whole module + `.test.rs` + 6 `pub use` + `mod` |
| 4 | MCP report site pointed at app/query (wrong layer) | P2 | SDK server handlers identified; `McpServerStatus` extended |
| 5 | AgentTool runtime schema built "without `mcp_servers`" — broke hook-rewrite revalidation | **P0** | Runtime includes `mcp_servers` + `run_in_background`; model omits |
| 6 | Plan referenced `derive_output_schema_value` (doesn't exist) | P3 | Renamed to `derive_output_schema` |

### v3.2 — second external review (7 findings, preserved for context)

| # | Finding | Revision |
|---|---|---|
| 1 | Tool inventory stale (35 vs. actual 42) | Regenerated; v3.2 introduced counting error itself, fixed in v3.3 |
| 2 | `from_wire` would sanitize external MCP contracts | Sanitize removed from production path |
| 3 | Constructors didn't meta-validate | All entries meta-validate at construction |
| 4 | MCP all-or-nothing preserved stale tools | Partition + diff swap (v3.2); atomic via single lock (v3.3) |
| 5 | `input_schema_for_session` / `input_schema()` shared a prefix | Renamed to `runtime_validation_schema` / `model_schema_for_session` |
| 6 | `omit_property` didn't specify `required`-list handling | Documented + tested |
| 7 | `SchemaError` used `thiserror` (wrong tier for `core/*`) | snafu + `ErrorExt` + `StatusCode` |

### v3.1 — first external review (6 findings, preserved for context)

| # | Finding | Revision |
|---|---|---|
| 1 | Root-type check missed semantic root | Added `RootTypeNotObject` |
| 2 | `ToolOutputSchema` symmetry inherited wrong contract | Output path lifted out |
| 3 | StructuredOutput sanitize would drift model from runtime | Verbatim via dedicated entry (v3.1) → generalized to "no sanitize" (v3.2) |
| 4 | "rename to replace `coco_types::ToolInputSchema`" was self-contradictory | Owner = `coco-tool-runtime`; old type deleted |
| 5 | "skip + warn" could disappear prior tools on reconnect | Partition + diff swap (v3.2) → atomic (v3.3) |
| 6 | `stable_hash() -> u64` had collision risk | Content-addressed `(ToolId, Arc<[u8]>)` |

### Net effect across all revisions

- **Simpler core**: 4 entries → 2; sanitize removed; subset invariant explicit; field-honesty invariant added (v3.4) and **closed** with `additionalProperties: false` (v3.5).
- **Tighter correctness contracts**: every instance meta-validated at construction; root type is strictly single-string `"object"`; `omit_property` preserves the invariant; `runtime ⊇ model_for_session(ctx)` and `runtime ⊆ Input fields` both test-enforced; closure direction (`additionalProperties: false` + Bucket-A `deny_unknown_fields`) **also** test-enforced (v3.5).
- **Cleaner dependency direction**: `coco-types` no longer holds `ToolInputSchema`; `SchemaError` matches L3 error tier; `services/inference` stays tool-agnostic.
- **Production correctness fixes**: MCP atomic swap with **alias hygiene** for retained tools (v3.5); content-detection cache that's also **bounded** (v3.5); registration-time meta-validation **and a fail-closed validate-site** (no silent skip, even in release — v3.5); AgentTool hook-rewrite path no longer breaks (`mcp_servers` accepted); BashTool sed-edit rewrite path covered by the same dual-track pattern (v3.5); AgentTool / BashTool field honesty preserved in both directions; MCP `RegisterMcpToolsReport` has a durable storage seam so `mcp/status` surfaces `skipped_tools` / `tombstoned_tools` correctly (v3.5).
- **Implementation cost**: ~3 days. v3.5 adds a half-day vs v3.4 — the closure rollout across 26 Bucket-A Input structs (`#[serde(deny_unknown_fields)]`), the BashTool dual-track template, the `SdkServerState::mcp_registration_reports` plumbing, the alias-wipe step in `replace_server_tools`, the `ToolRegistry::count_by_server` accessor, the `CachedValidator` cache refactor, and the `InternalSchemaError` invalid-reason variant + regression test.

---

## References

- History: [`tool-schema-validated-newtype-plan.md`](tool-schema-validated-newtype-plan.md) (v1) / [`tool-schema-source-plan.md`](tool-schema-source-plan.md) (v2)
- codex-rs reference (not ported into v3.4 production path): `codex-rs/tools/src/json_schema.rs` + `codex-rs/tools/src/json_schema_tests.rs`
- Adjacent crate docs: [`crate-coco-tool.md`](crate-coco-tool.md) / [`crate-coco-tools.md`](crate-coco-tools.md)
- TS equivalent: `tools/SyntheticOutputTool/SyntheticOutputTool.ts`
- Project rules: [`coco-rs/CLAUDE.md`](../../coco-rs/CLAUDE.md), "Code Hygiene" + "Error Handling" sections
- User memory: `project_coco_rs_mcp_tool_input_json_schema.md`
