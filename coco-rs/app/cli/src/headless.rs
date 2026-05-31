//! Headless (`coco -p "<prompt>"`) entry point exposed as a library
//! function so live tests, embeddings, and the binary all drive the
//! same code path.
//!
//! `run_chat` returns a structured [`RunChatOutcome`] instead of
//! printing to stdout. The binary's `main()` thin-wraps this and
//! formats stdout from the outcome.
//!
//! Helpers shared by `run_chat` and the SDK runner (`MockModel`,
//! `resolve_main_model`, `cli_runtime_overrides`,
//! `build_runtime_config_for_cli`, `build_system_prompt[_for_model]`,
//! `resolve_startup_permission_state`) live here as well, so a test
//! can drive any of them in isolation.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

use anyhow::Result;
use coco_inference::AISdkError;
use coco_inference::LanguageModel;
use coco_inference::LanguageModelCallOptions;
use coco_inference::LanguageModelGenerateResult;
use coco_inference::LanguageModelStreamResult;
use coco_llm_types::AssistantContentPart;
use coco_llm_types::FinishReason;
use coco_llm_types::StopReason;
use coco_llm_types::TextPart;
use coco_llm_types::Usage;
use coco_messages::CostTracker;
use coco_query::ContinueReason;
use coco_query::QueryEngine;
use coco_query::QueryEngineConfig;
use coco_tool_runtime::ToolRegistry;
use coco_types::TokenUsage;
use tokio_util::sync::CancellationToken;

use crate::Cli;

/// Fallback base instructions used when a resolved `ModelInfo`
/// declares no `base_instructions` (e.g. Claude built-ins and any
/// user-added non-builtin model in `~/.coco/providers.json` /
/// `models.json` that doesn't set `base_instructions[_file]`). Routed
/// through `coco_config::DEFAULT_BASE_INSTRUCTIONS` so the on-disk
/// `instructions/default_prompt.md` is the single source of truth.
pub const DEFAULT_SYSTEM_PROMPT_IDENTITY: &str = coco_config::DEFAULT_BASE_INSTRUCTIONS;

// ─── Mock model (no-credentials fallback) ────────────────────────────

/// Built-in mock model for development/testing.
pub struct MockModel {
    call_count: AtomicI32,
}

impl MockModel {
    pub fn new() -> Self {
        Self {
            call_count: AtomicI32::new(0),
        }
    }
}

impl Default for MockModel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl LanguageModel for MockModel {
    fn provider(&self) -> &str {
        "mock"
    }
    fn model_id(&self) -> &str {
        "mock-model"
    }
    async fn do_generate(
        &self,
        options: &LanguageModelCallOptions,
        _abort_signal: Option<tokio_util::sync::CancellationToken>,
    ) -> std::result::Result<LanguageModelGenerateResult, AISdkError> {
        let call = self.call_count.fetch_add(1, Ordering::SeqCst);
        let user_text: String = options
            .prompt
            .iter()
            .filter_map(|msg| match msg {
                coco_llm_types::LlmMessage::User { content, .. } => Some(
                    content
                        .iter()
                        .filter_map(|c| match c {
                            coco_llm_types::UserContentPart::Text(t) => Some(t.text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");

        let response = format!(
            "[mock model, call #{call}] Received: \"{user_text}\"\n\n\
             No model configured. Set a model via settings.json or --model to use a real provider."
        );

        Ok(LanguageModelGenerateResult {
            content: vec![AssistantContentPart::Text(TextPart {
                text: response,
                provider_metadata: None,
            })],
            usage: Usage::new(user_text.len() as u64 / 4, 50),
            finish_reason: FinishReason::new(StopReason::EndTurn),
            warnings: vec![],
            provider_metadata: None,
            request: None,
            response: None,
        })
    }
    async fn do_stream(
        &self,
        options: &LanguageModelCallOptions,
        abort_signal: Option<tokio_util::sync::CancellationToken>,
    ) -> std::result::Result<LanguageModelStreamResult, AISdkError> {
        // Compose `do_generate` output into a synthetic stream so the
        // QueryEngine streaming path (which always calls `query_stream`)
        // works against the mock.
        let result = self.do_generate(options, abort_signal).await?;
        Ok(coco_inference::synthetic_stream_from_content(
            result.content,
            result.usage,
            result.finish_reason,
        ))
    }
}

// ─── RuntimeConfig + model resolution ────────────────────────────────

/// Derive `RuntimeOverrides` from the parsed CLI flags.
///
/// Validates numeric flags up-front so a non-positive value can't
/// silently propagate down to the budget tracker (where `<=0` would
/// trigger immediate "budget exhausted" and short-circuit every LLM
/// call to an empty response).
pub fn cli_runtime_overrides(cli: &Cli) -> Result<coco_config::RuntimeOverrides> {
    use coco_types::ProviderModelSelection;

    let mut overrides = coco_config::RuntimeOverrides::default();
    if let Some(raw) = cli.model.as_deref() {
        overrides.model_override = Some(
            ProviderModelSelection::from_slash_str(raw)
                .map_err(|e| anyhow::anyhow!("--model: {e}"))?,
        );
    }
    if let Some(mode) = cli.permission_mode.as_deref()
        && let Ok(pm) = serde_json::from_value::<coco_types::PermissionMode>(
            serde_json::Value::String(mode.to_string()),
        )
    {
        overrides.permission_mode_override = Some(pm);
    }
    overrides.fallback_model_overrides = cli
        .fallback_model
        .iter()
        .map(|raw| {
            ProviderModelSelection::from_slash_str(raw)
                .map_err(|e| anyhow::anyhow!("--fallback-model: {e}"))
        })
        .collect::<Result<Vec<_>>>()?;
    if let Some(max_tokens) = cli.max_tokens
        && max_tokens <= 0
    {
        anyhow::bail!(
            "--max-tokens must be > 0 (got {max_tokens}); a non-positive value short-circuits \
             the budget tracker and produces empty responses"
        );
    }
    if let Some(max_turns) = cli.max_turns
        && max_turns < 1
    {
        anyhow::bail!(
            "--max-turns must be >= 1 (got {max_turns}); 0 or negative would prevent the \
             agent loop from executing any turn"
        );
    }
    Ok(overrides)
}

/// Build a `RuntimeConfig` honoring CLI-level overrides.
pub fn build_runtime_config_for_cli(cli: &Cli, cwd: &Path) -> Result<coco_config::RuntimeConfig> {
    let mut builder = coco_config::RuntimeConfigBuilder::from_process(cwd)
        .with_overrides(cli_runtime_overrides(cli)?);
    if let Some(path) = cli.settings.as_deref() {
        builder = builder.with_flag_settings(path);
    }
    Ok(builder.build()?)
}

#[derive(Debug, Clone)]
pub struct ResolvedMainModel {
    pub provider: String,
    pub provider_api: Option<coco_types::ProviderApi>,
    pub model_id: String,
    pub supports_prompt_cache: bool,
}

pub fn resolve_main_model(runtime_config: &coco_config::RuntimeConfig) -> ResolvedMainModel {
    use coco_types::ModelRole;

    if let Some(main_spec) = runtime_config.model_roles.get(ModelRole::Main) {
        let supports_prompt_cache = matches!(main_spec.api, coco_types::ProviderApi::Anthropic)
            && runtime_config
                .model_registry
                .resolve(&main_spec.provider, &main_spec.model_id)
                .is_some_and(|model| {
                    model
                        .info
                        .capabilities
                        .as_ref()
                        .is_some_and(|caps| caps.contains(&coco_types::Capability::PromptCache))
                });
        return ResolvedMainModel {
            provider: main_spec.provider.clone(),
            provider_api: Some(main_spec.api),
            model_id: main_spec.model_id.clone(),
            supports_prompt_cache,
        };
    }

    let model = MockModel::new();
    ResolvedMainModel {
        provider: model.provider().to_string(),
        provider_api: None,
        model_id: model.model_id().to_string(),
        supports_prompt_cache: false,
    }
}

// ─── Output style manager ────────────────────────────────────────────

/// Build a [`coco_output_styles::OutputStyleManager`] from settings,
/// the standard on-disk dirs ([`crate::paths::user_output_style_dir`],
/// [`crate::paths::project_output_style_dirs`],
/// [`crate::paths::managed_output_style_dir`]), and the supplied
/// plugin sources.
///
/// Headless and SDK paths share this helper so a future addition (e.g.,
/// project-tree ancestor walk) lands in one place. The plugin
/// pipeline isn't yet plumbed in headless — pass an empty slice.
pub fn build_output_style_manager(
    runtime_config: &coco_config::RuntimeConfig,
    cwd: &Path,
    plugin_sources: &[coco_output_styles::PluginOutputStyleSource],
) -> coco_output_styles::OutputStyleManager {
    coco_output_styles::OutputStyleManager::builder()
        .settings_name(runtime_config.settings.merged.output_style.clone())
        .user_dir(Some(crate::paths::user_output_style_dir()))
        .project_dirs(crate::paths::project_output_style_dirs(cwd))
        .managed_dir(Some(crate::paths::managed_output_style_dir()))
        .plugins(plugin_sources.to_vec())
        .build()
}

// ─── System prompt assembly ──────────────────────────────────────────

/// Convert a resolved [`OutputStyleConfig`] into the borrowed view the
/// `coco-context` prompt builder accepts. Built-in styles set
/// `keep_coding_instructions: Some(true)`; unset custom/plugin styles
/// default to `false`, matching TS's strict
/// `keepCodingInstructions === true` gate.
fn output_style_section(
    style: &coco_output_styles::OutputStyleConfig,
) -> coco_context::prompt::OutputStyleSection<'_> {
    coco_context::prompt::OutputStyleSection {
        name: &style.name,
        prompt: &style.prompt,
        keep_coding_instructions: style.keep_coding_instructions.unwrap_or(false),
    }
}

/// Build the system prompt with environment context and CLAUDE.md content.
pub fn build_system_prompt(
    cwd: &Path,
    model_id: &str,
    base_instructions: Option<&str>,
    output_style: Option<&coco_output_styles::OutputStyleConfig>,
    additional_working_directories: &[String],
) -> String {
    let claude_files = coco_context::discover_memory_files(cwd);
    let env_info = coco_context::get_environment_info(cwd, model_id);
    let identity = base_instructions.unwrap_or(DEFAULT_SYSTEM_PROMPT_IDENTITY);
    let section = output_style.map(output_style_section);
    coco_context::build_system_prompt(
        identity,
        &claude_files,
        &env_info,
        None,
        None,
        None,
        section,
        additional_working_directories,
    )
    .full_text()
}

/// Resolve model-specific instructions from runtime config, then build
/// the prompt. Shared by headless, SDK, and TUI bootstraps.
pub fn build_system_prompt_for_model(
    cwd: &Path,
    runtime_config: &coco_config::RuntimeConfig,
    provider: &str,
    model_id: &str,
    output_style: Option<&coco_output_styles::OutputStyleConfig>,
    additional_working_directories: &[String],
) -> String {
    let resolved = runtime_config.model_registry.resolve(provider, model_id);
    let base_instructions = resolved
        .as_ref()
        .and_then(|model| model.info.base_instructions.as_deref());
    build_system_prompt(
        cwd,
        model_id,
        base_instructions,
        output_style,
        additional_working_directories,
    )
}

// ─── Permission resolution ───────────────────────────────────────────

/// Resolved startup permission state.
pub struct StartupPermissionState {
    pub mode: coco_types::PermissionMode,
    pub bypass_available: bool,
    pub notification: Option<String>,
}

/// Resolve the session's initial `PermissionMode` and the bypass capability.
pub fn resolve_startup_permission_state(
    cli: &Cli,
    settings: &coco_config::Settings,
) -> Result<StartupPermissionState> {
    use coco_types::PermissionMode;

    let policy_flag = Some(settings.permissions.disable_bypass_mode);

    let permission_mode_cli = cli.permission_mode.as_deref().and_then(|raw| {
        match serde_json::from_value::<PermissionMode>(serde_json::json!(raw)) {
            Ok(m) => Some(m),
            Err(e) => {
                eprintln!("warning: invalid --permission-mode {raw:?}: {e}; ignoring");
                None
            }
        }
    });

    let resolved = coco_permissions::resolve_initial_permission_mode(
        cli.dangerously_skip_permissions,
        permission_mode_cli,
        settings.permissions.default_mode,
        policy_flag,
    );
    let mode = resolved.mode;

    let bypass_available = coco_permissions::compute_bypass_capability(
        mode == PermissionMode::BypassPermissions,
        cli.allow_dangerously_skip_permissions,
        policy_flag,
    );

    let requesting_bypass =
        mode == PermissionMode::BypassPermissions || cli.allow_dangerously_skip_permissions;
    enforce_dangerous_skip_safety(requesting_bypass)?;

    Ok(StartupPermissionState {
        mode,
        bypass_available,
        notification: resolved.notification,
    })
}

/// Reject requesting bypass when the host is not a sandbox.
/// Parse `--json-schema` (if set) and register the synthetic
/// `StructuredOutput` tool against `registry` + a matching Stop
/// function hook on `hook_registry`.
///
/// TS parity: `main.tsx:1879-1901` plus
/// `registerStructuredOutputEnforcement` (`hookHelpers.ts:70-83`).
/// Only the non-interactive bootstrap (headless print mode / SDK
/// NDJSON) calls this; TUI must not, by design — the tool is excluded
/// from `register_all_tools` and only installed through this helper.
///
/// Returns `Ok(true)` when the flag was set and both the tool and
/// Stop hook were registered. Returns `Ok(false)` when the flag was
/// absent (caller proceeds without structured output). Returns
/// `Err(_)` when:
///   - `--json-schema` is not valid JSON
///   - the parsed value fails JSON-Schema meta-validation
///   - the Stop function hook fails to register (programmer error —
///     duplicate id, unsupported event)
///
/// TS logs `tengu_structured_output_failure` for the first two; coco-rs
/// surfaces the failure to the operator instead of silently continuing
/// without schema enforcement.
pub fn inject_structured_output_tool_if_requested(
    cli: &Cli,
    registry: &ToolRegistry,
    hook_registry: &coco_hooks::HookRegistry,
) -> Result<bool> {
    let Some(raw) = cli.json_schema.as_deref() else {
        return Ok(false);
    };
    let schema: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| anyhow::anyhow!("--json-schema is not valid JSON: {e}"))?;
    coco_tools::register_structured_output_tool(registry, schema)
        .map_err(|e| anyhow::anyhow!("--json-schema rejected: {e}"))?;

    // TS parity: `registerStructuredOutputEnforcement`
    // (`hookHelpers.ts:70-83`). Block the model from ending its turn
    // until it has pushed at least one valid `StructuredOutput`
    // attachment into history. Coco-rs uses the typed AttachmentKind
    // directly instead of a fragile `hasSuccessfulToolCall(name)` scan.
    hook_registry
        .register_function_hook(
            format!("structured-output-enforcement-{}", uuid::Uuid::new_v4()),
            coco_types::HookEventType::Stop,
            None,
            std::time::Duration::from_millis(5_000),
            std::sync::Arc::new(StructuredOutputEnforcement),
            format!(
                "You MUST call the {} tool to complete this request. Call this tool now.",
                coco_types::ToolName::StructuredOutput.as_str()
            ),
        )
        .map_err(|e| anyhow::anyhow!("failed to register StructuredOutput Stop hook: {e}"))?;

    tracing::info!(
        target: "coco_cli::headless",
        "registered StructuredOutput tool + Stop enforcement hook from --json-schema"
    );
    Ok(true)
}

/// [`coco_hooks::FunctionHookPredicate`] impl for the TS-parity
/// `StructuredOutput` Stop enforcement. Returns `true` when history
/// already contains a successful `StructuredOutput` tool call (the
/// silent attachment is only pushed on schema-conforming input, per
/// `StructuredOutputTool::execute`).
#[derive(Debug)]
struct StructuredOutputEnforcement;

impl coco_hooks::FunctionHookPredicate for StructuredOutputEnforcement {
    fn evaluate(&self, messages: &[std::sync::Arc<coco_messages::Message>]) -> bool {
        use coco_messages::AttachmentBody;
        use coco_messages::Message;
        use coco_messages::SilentPayload;
        use coco_types::AttachmentKind;
        messages.iter().any(|m| match m.as_ref() {
            Message::Attachment(att) => {
                att.kind == AttachmentKind::StructuredOutput
                    && matches!(
                        &att.body,
                        AttachmentBody::Silent(SilentPayload::StructuredOutput(_))
                    )
            }
            _ => false,
        })
    }

    fn name(&self) -> &str {
        "StructuredOutputEnforcement"
    }
}

fn enforce_dangerous_skip_safety(requesting_bypass: bool) -> Result<()> {
    if !requesting_bypass {
        return Ok(());
    }
    if is_root_like_env() && !is_sandboxed_env() {
        return Err(anyhow::anyhow!(
            "Bypass permissions refuses to run as root/sudo outside a \
             sandbox. Set IS_SANDBOX=1 (or run under bubblewrap) if you \
             know what you're doing."
        ));
    }
    Ok(())
}

fn is_root_like_env() -> bool {
    if std::env::var_os("SUDO_USER").is_some() || std::env::var_os("SUDO_UID").is_some() {
        return true;
    }
    let is_root_name = |var: &str| -> bool {
        std::env::var(var)
            .map(|v| v.trim() == "root")
            .unwrap_or(false)
    };
    is_root_name("USER") || is_root_name("LOGNAME") || is_root_name("USERNAME")
}

fn is_sandboxed_env() -> bool {
    let truthy = |var: &str| -> bool {
        std::env::var(var)
            .map(|v| {
                matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    };
    truthy("IS_SANDBOX") || coco_config::env::is_env_truthy(coco_config::EnvKey::CocoBubblewrap)
}

// ─── run_chat ────────────────────────────────────────────────────────

/// Outcome of a single headless `coco -p` invocation.
///
/// Mirrors the data the binary's `main()` would have printed, but
/// returns it structured so tests / embeddings can assert on individual
/// fields.
#[derive(Debug)]
pub struct RunChatOutcome {
    /// Final assistant response text (what the binary prints to stdout).
    pub response_text: String,
    /// Number of agent loop turns executed.
    pub turns: i32,
    /// Total token usage accumulated across the session.
    pub total_usage: TokenUsage,
    /// Per-model cost / token tracking.
    pub cost_tracker: CostTracker,
    /// Resolved model id (provider-side wire name).
    pub model_id: String,
    /// `Some(api)` when a real provider was wired; `None` for mock fallback.
    pub provider_api: Option<coco_types::ProviderApi>,
    /// Resolved permission mode after CLI + settings + killswitch merge.
    pub permission_mode: coco_types::PermissionMode,
    /// `true` when the session is allowed to transition to `BypassPermissions`.
    pub bypass_permissions_available: bool,
    /// Optional notification surfaced when permission resolution downgraded
    /// (e.g. killswitch forced Bypass → AcceptEdits). Caller should print
    /// to stderr.
    pub permission_notification: Option<String>,
    /// Total wall-clock duration in milliseconds.
    pub duration_ms: i64,
    /// API time in milliseconds.
    pub duration_api_ms: i64,
    /// Whether the run hit the budget limit.
    pub budget_exhausted: bool,
    /// Whether the run was cancelled.
    pub cancelled: bool,
    /// Last continue reason from the engine loop.
    pub last_continue_reason: Option<ContinueReason>,
    /// Number of fallback runtime slots installed on the engine.
    /// (from `--fallback-model` flags + `models.<role>.fallbacks`).
    pub installed_fallback_count: usize,
    /// Final message history at session end, including the user prompt,
    /// any tool calls + results, and the final assistant reply. Tests
    /// or embedding callers can feed this into the next [`run_chat_with_options`]
    /// call (`opts.prior_messages = previous.final_messages`) to
    /// continue the conversation in-process.
    pub final_messages: Vec<std::sync::Arc<coco_messages::Message>>,
    /// Working directory the engine actually used. Reflects the
    /// effective resolution: `--cwd <flag>` then `RunChatOptions::cwd`
    /// then `std::env::current_dir()`. Useful for tests asserting the
    /// flag-precedence rule.
    pub effective_cwd: PathBuf,
    /// Additional directories declared via `--add-dir` (resolved to
    /// absolute paths). Threaded onto every tool's permission context
    /// so file-system tools may read from them. Empty = no extras.
    pub additional_dirs: Vec<PathBuf>,
    /// Tool filter built from `--allowed-tools` / `--disallowed-tools`.
    /// `None` ⇒ both flags were empty (engine uses `unrestricted()`).
    pub tool_filter_summary: Option<ToolFilterSummary>,
}

/// Lightweight surface of [`coco_types::ToolFilter`] for tests — the
/// underlying type uses `HashSet<ToolId>` whose iteration is
/// non-deterministic, so we project to sorted vectors.
#[derive(Debug, Clone, Default)]
pub struct ToolFilterSummary {
    pub allowed: Vec<String>,
    pub disallowed: Vec<String>,
}

/// Options for [`run_chat_with_options`]. All fields default to the
/// same behavior as `run_chat`.
#[derive(Default)]
pub struct RunChatOptions {
    /// Override the working directory for this run. When `None`, the
    /// process-global `std::env::current_dir()` is used. Pass an
    /// explicit path to keep parallel tests / embeddings isolated.
    pub cwd: Option<PathBuf>,
    /// Cancellation token threaded into the engine. When the token is
    /// cancelled mid-run, the engine returns a `cancelled = true`
    /// outcome. `None` = a fresh token is created internally.
    pub cancel: Option<CancellationToken>,
    /// Pre-built message history to seed the conversation. Empty =
    /// start a fresh conversation (the default `run_chat` behavior).
    /// Non-empty = continue from the prior turns; the engine drives
    /// `run_with_messages(prior + user_prompt)` instead of `run`.
    pub prior_messages: Vec<std::sync::Arc<coco_messages::Message>>,
    /// Override the engine's session id. Used by `--resume` /
    /// `--continue` / `--fork-session` so the resumed run writes
    /// transcript entries under the source (or fork) session id
    /// instead of a fresh per-process uuid. `None` keeps the
    /// engine's default empty-session-id behavior.
    pub session_id_override: Option<String>,
}

/// Drive one headless agent run with default options. See
/// [`run_chat_with_options`] for cwd / cancellation / session-continuation.
pub async fn run_chat(cli: &Cli, prompt: Option<&str>) -> Result<RunChatOutcome> {
    run_chat_with_options(cli, prompt, RunChatOptions::default()).await
}

/// Drive one headless agent run with explicit options.
///
/// Mirrors `coco -p "<prompt>"` with the same flag plumbing the
/// binary uses, plus three test-friendly knobs:
///
/// - `opts.cwd` — override `std::env::current_dir()` so parallel
///   embeddings / tests stay isolated.
/// - `opts.cancel` — thread an external [`CancellationToken`] for
///   mid-run cancellation.
/// - `opts.prior_messages` — seed the conversation with a previous
///   `RunChatOutcome.final_messages`, simulating `--continue` /
///   `--resume` in-process.
///
/// Honors these `Cli` flags end-to-end:
/// `--model`, `--fallback-model`, `--permission-mode`,
/// `--dangerously-skip-permissions` / `--allow-…`, `--max-turns`,
/// `--max-tokens`, `--settings`, `--system-prompt`,
/// `--append-system-prompt`, `--append-system-prompt-file`,
/// `--cwd`, `--add-dir`, `--allowed-tools`, `--disallowed-tools`.
pub async fn run_chat_with_options(
    cli: &Cli,
    prompt: Option<&str>,
    opts: RunChatOptions,
) -> Result<RunChatOutcome> {
    let prompt = prompt.unwrap_or("Hello!");
    // Cwd precedence: explicit user `--cwd` flag > `RunChatOptions::cwd`
    // (test/embedder injection) > `std::env::current_dir()`.
    let cwd: PathBuf = if let Some(flag) = cli.cwd.as_deref() {
        std::path::Path::new(flag).to_path_buf()
    } else if let Some(p) = opts.cwd {
        p
    } else {
        std::env::current_dir()?
    };
    tracing::info!(
        target: "coco_cli::headless",
        cwd = %cwd.display(),
        prompt_len = prompt.len(),
        has_prior_messages = !opts.prior_messages.is_empty(),
        "headless run starting"
    );

    let runtime_config = build_runtime_config_for_cli(cli, &cwd)?;
    crate::model_card_refresh::spawn_if_enabled(&runtime_config);
    let settings = &runtime_config.settings;

    // Resolve the active output style once — fed into the system
    // prompt builder + threaded onto `SessionBootstrap` for the
    // per-turn reminder generator. Plugin styles aren't loaded in the
    // headless path (no plugin discovery yet); user / project /
    // managed dirs are walked.
    let output_style_manager = build_output_style_manager(&runtime_config, &cwd, &[]);
    let active_output_style = output_style_manager.active().cloned();

    let main_model = resolve_main_model(&runtime_config);
    let provider_api = main_model.provider_api;
    let model_id = main_model.model_id.clone();
    let model_runtimes = Arc::new(coco_inference::ModelRuntimeRegistry::new(
        Arc::new(runtime_config.clone()),
        Some(crate::provider_login::shared_resolver()),
    )?);
    let installed_fallback_count = runtime_config
        .model_roles
        .fallbacks(coco_types::ModelRole::Main)
        .len();
    let fallback_policy = runtime_config
        .model_roles
        .policy(coco_types::ModelRole::Main);
    tracing::info!(
        target: "coco_cli::headless",
        provider = main_model.provider,
        model_id = %model_id,
        real_provider = provider_api.is_some(),
        fallback_count = installed_fallback_count,
        fallback_policy_set = fallback_policy.is_some(),
        "model client resolved"
    );

    let registry = ToolRegistry::new();
    coco_tools::register_all_tools(&registry);

    // Headless doesn't load settings hooks yet (TS handles those via
    // `runHeadless`'s settings loader; coco-rs's headless path skips
    // them for now). We still need an in-memory `HookRegistry` so the
    // `StructuredOutput` Stop function hook can register against it.
    // Once headless hook loading lands, swap this for the shared
    // registry built at session bootstrap.
    let hook_registry = Arc::new(coco_hooks::HookRegistry::new());

    // Conditional `StructuredOutput` tool injection + Stop hook
    // registration. TS-parity: `main.tsx:1879-1901` plus
    // `registerStructuredOutputEnforcement(setAppState, sessionId)`.
    // TUI never reaches this code path (matches TS
    // `isNonInteractiveSession` gate).
    inject_structured_output_tool_if_requested(cli, &registry, &hook_registry)?;
    let tool_count = registry.len();
    let tools = Arc::new(registry);
    let cancel = opts.cancel.unwrap_or_default();

    let startup = resolve_startup_permission_state(cli, &settings.merged)?;
    let permission_mode = startup.mode;
    let bypass_permissions_available = startup.bypass_available;
    tracing::info!(
        target: "coco_cli::headless",
        permission_mode = ?permission_mode,
        bypass_available = bypass_permissions_available,
        permission_notification = startup.notification.is_some(),
        tool_count,
        sandbox_mode = ?runtime_config.sandbox.mode,
        "permissions + tools ready"
    );

    let system_prompt = compose_system_prompt(
        cli,
        &cwd,
        &runtime_config,
        &main_model.provider,
        &model_id,
        active_output_style.as_ref(),
    )?;

    // Bootstrap the per-source permission rule maps; see
    // `crate::permission_rule_loader` for the conversion path. Mirrors
    // TS `loadPermissionRules()` so headless runs honor the same
    // settings.json deny/allow/ask rules as the TUI.
    let (allow_rules, deny_rules, ask_rules) =
        crate::permission_rule_loader::typed_permission_rules(&runtime_config.settings);
    let permission_rule_source_roots =
        crate::permission_rule_loader::permission_rule_source_roots(&runtime_config.settings, &cwd);

    let config = QueryEngineConfig {
        model_id: model_id.clone(),
        // `--resume` / `--continue` / `--fork-session` route through
        // `RunChatOptions::session_id_override`; absent it the engine
        // defaults to a per-run uuid (TS parity: anonymous headless
        // runs aren't keyed against a persistent transcript).
        session_id: opts
            .session_id_override
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        permission_mode,
        bypass_permissions_available,
        allow_rules,
        deny_rules,
        ask_rules,
        permission_rule_source_roots,
        context_window: 200_000,
        max_output_tokens: 16_384,
        max_turns: cli
            .max_turns
            .or(runtime_config.loop_config.max_turns)
            .unwrap_or(30),
        total_token_budget: cli
            .max_tokens
            .or_else(|| runtime_config.loop_config.total_token_budget.map(i64::from)),
        prompt_cache: main_model
            .supports_prompt_cache
            .then(|| coco_types::PromptCacheConfig {
                mode: coco_types::PromptCacheMode::Auto,
                ttl: coco_types::CacheTtl::OneHour,
                scope: None,
                requested_betas: Default::default(),
                skip_cache_write: false,
            }),
        system_prompt: Some(system_prompt),
        streaming_tool_execution: runtime_config.loop_config.enable_streaming_tools,
        project_dir: Some(
            runtime_config
                .paths
                .project_dir
                .clone()
                .unwrap_or_else(|| cwd.clone()),
        ),
        cwd_override: Some(cwd.clone()),
        tool_filter: build_tool_filter(cli),
        plans_directory: settings.merged.plans_directory.clone(),
        plan_mode_settings: settings.merged.plan_mode.clone(),
        system_reminder: settings.merged.system_reminder.clone(),
        tool_config: runtime_config.tool.clone(),
        sandbox_config: runtime_config.sandbox.clone(),
        sandbox_state: crate::session_runtime::build_sandbox_state(&runtime_config, &cwd)?,
        memory_config: runtime_config.memory.clone(),
        shell_config: runtime_config.shell.clone(),
        web_fetch_config: runtime_config.web_fetch.clone(),
        web_search_config: runtime_config.web_search.clone(),
        lsp_config: runtime_config.lsp.clone(),
        compact: runtime_config.compact.clone(),
        features: Arc::new(runtime_config.features.clone()),
        skill_overrides: Arc::new(runtime_config.skill_overrides.clone()),
        tool_overrides: runtime_config.tool_overrides.clone(),
        ..Default::default()
    };

    tracing::info!(
        target: "coco_cli::headless",
        max_turns = config.max_turns,
        total_token_budget = ?config.total_token_budget,
        context_window = config.context_window,
        streaming_tools = config.streaming_tool_execution,
        plan_mode = ?config.plan_mode_settings,
        "engine config built"
    );

    // Per-call FileReadState — gives the Read tool's dedup AND the
    // shared @-mention pipeline a session-scoped cache. One-shot scope
    // (dies with the function) matches `coco -p` semantics.
    let file_read_state = Arc::new(tokio::sync::RwLock::new(coco_context::FileReadState::new()));

    let session_id_for_engine = config.session_id.clone();
    let agent_id_for_engine = config.agent_id.clone();
    let mut engine = QueryEngine::new(config, model_runtimes, tools, cancel, Some(hook_registry))
        .with_file_read_state(file_read_state.clone());
    let mut session_usage_flush = None;
    // Wire the JSONL transcript writer for resume / continue runs so
    // headless turns persist into the same `<sessions_dir>/<id>.jsonl`
    // the TUI / SDK paths use. Pre-populate the dedup set with the
    // resumed messages' uuids — those entries are already on disk
    // and re-appending them would corrupt the chain.
    if opts.session_id_override.is_some() {
        let store = Arc::new(coco_session::TranscriptStore::new(
            crate::paths::project_paths(&cwd),
        ));
        let store_for_load = Arc::clone(&store);
        let usage_session_id = session_id_for_engine.clone();
        let loaded_usage = tokio::task::spawn_blocking(move || {
            store_for_load.load_usage_snapshot(&usage_session_id)
        })
        .await
        .ok()
        .and_then(std::result::Result::ok)
        .flatten()
        .map(CostTracker::from_snapshot)
        .unwrap_or_default();
        let session_usage_tracker = Arc::new(tokio::sync::Mutex::new(loaded_usage));
        let session_usage_write_lock = Arc::new(tokio::sync::Mutex::new(()));
        let mut seen: std::collections::HashSet<uuid::Uuid> = std::collections::HashSet::new();
        for msg in &opts.prior_messages {
            if let Some(uuid) = msg.uuid() {
                seen.insert(*uuid);
            }
        }
        let dedup = Arc::new(tokio::sync::Mutex::new(seen));
        let records = store
            .load_content_replacements_for_chain(
                &session_id_for_engine,
                agent_id_for_engine.as_deref(),
            )
            .unwrap_or_default();
        let mut replacement_state =
            coco_tool_runtime::tool_result_storage::ContentReplacementState::new(i64::MAX);
        for msg in &opts.prior_messages {
            if let coco_messages::Message::ToolResult(tr) = msg.as_ref() {
                replacement_state.seen_ids.insert(tr.tool_use_id.clone());
            }
        }
        for record in records {
            replacement_state
                .seen_ids
                .insert(record.tool_use_id().to_string());
            replacement_state.replacements.insert(
                record.tool_use_id().to_string(),
                record.replacement().to_string(),
            );
        }
        engine = engine
            .with_transcript_store(store.clone(), session_id_for_engine.clone())
            .with_session_usage_tracker(session_usage_tracker.clone())
            .with_session_usage_write_lock(session_usage_write_lock.clone())
            .with_transcript_dedup(dedup)
            .with_tool_result_replacement_state(Arc::new(tokio::sync::RwLock::new(
                replacement_state,
            )));
        session_usage_flush = Some((
            store,
            session_id_for_engine.clone(),
            session_usage_tracker,
            session_usage_write_lock,
        ));
    }

    // Resolve `@`-mentions in the prompt to file-content system-reminder
    // messages. TS parity: `getAttachmentMessages` from
    // `processUserInput.ts:504`. Both branches below now share one
    // expansion pipeline so headless behaves like TUI / SDK.
    let inputs =
        crate::at_mention_turn::resolve_turn_inputs_text_only(prompt, &cwd, &file_read_state).await;
    let new_turn_messages = crate::at_mention_turn::build_messages_for_turn(&inputs);
    let messages: Vec<std::sync::Arc<coco_messages::Message>> = if opts.prior_messages.is_empty() {
        new_turn_messages
            .into_iter()
            .map(std::sync::Arc::new)
            .collect()
    } else {
        let mut combined = opts.prior_messages;
        combined.extend(new_turn_messages.into_iter().map(std::sync::Arc::new));
        combined
    };
    if !inputs.mentioned_paths.is_empty() {
        engine
            .note_mentioned_paths(inputs.mentioned_paths.clone())
            .await;
    }

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(64);
    // Drain events to /dev/null — callers wanting events should drop
    // down to `coco_query::QueryEngine::run_with_events` directly.
    let drainer = tokio::spawn(async move { while event_rx.recv().await.is_some() {} });
    let result = engine
        .run_with_messages(messages, event_tx, coco_types::TurnId::generate())
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    drainer.abort();

    if let Some((store, session_id, tracker, write_lock)) = session_usage_flush {
        let _write_guard = write_lock.lock().await;
        let snapshot = tracker.lock().await.snapshot(&session_id);
        let session_id_for_write = session_id.clone();
        match tokio::task::spawn_blocking(move || {
            store.write_usage_snapshot(&session_id_for_write, &snapshot)
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!(error = %e, session_id, "failed to flush headless session usage");
            }
            Err(e) => {
                tracing::warn!(error = %e, session_id, "headless session usage flush task failed");
            }
        }
    }

    // Wait for any in-flight auto-memory extraction + session-memory
    // fork to complete before we return so partial writes aren't
    // dropped on process exit. TS parity: `print.ts` awaits
    // `drainPendingExtraction(60_000)` here; we additionally cover SM
    // via `waitForSessionMemoryExtraction(15_000)` so a half-written
    // `summary.md` doesn't survive into the next `--resume`.
    if let Some(memory_runtime) = engine.memory_runtime() {
        let _ = memory_runtime
            .extract
            .drain(coco_memory::service::extract::DEFAULT_DRAIN_TIMEOUT)
            .await;
        let _ = memory_runtime
            .session_memory
            .wait_for_extraction(coco_memory::service::session::DEFAULT_WAIT_TIMEOUT)
            .await;
    }

    let additional_dirs = resolve_additional_dirs(cli, &cwd);
    let tool_filter_summary = summarize_tool_filter(cli);

    Ok(RunChatOutcome {
        effective_cwd: cwd.clone(),
        additional_dirs,
        tool_filter_summary,
        response_text: result.response_text,
        turns: result.turns,
        total_usage: result.total_usage,
        cost_tracker: result.cost_tracker,
        model_id,
        provider_api,
        permission_mode,
        bypass_permissions_available,
        permission_notification: startup.notification,
        duration_ms: result.duration_ms,
        duration_api_ms: result.duration_api_ms,
        budget_exhausted: result.budget_exhausted,
        cancelled: result.cancelled,
        last_continue_reason: result.last_continue_reason,
        installed_fallback_count,
        final_messages: result.final_messages,
    })
}

/// Compose the session's system prompt, honoring `--system-prompt`
/// (full override), `--append-system-prompt` (text appended after the
/// default), and `--append-system-prompt-file` (file contents appended).
fn compose_system_prompt(
    cli: &Cli,
    cwd: &Path,
    runtime_config: &coco_config::RuntimeConfig,
    provider: &str,
    model_id: &str,
    output_style: Option<&coco_output_styles::OutputStyleConfig>,
) -> Result<String> {
    // 1. Base layer: `--system-prompt` wholly replaces the default
    //    identity + CLAUDE.md discovery. Otherwise build the default.
    let additional_dirs = resolve_additional_dirs_display(cli, cwd);
    let mut prompt = if let Some(custom) = cli.system_prompt.as_deref() {
        custom.to_string()
    } else {
        build_system_prompt_for_model(
            cwd,
            runtime_config,
            provider,
            model_id,
            output_style,
            &additional_dirs,
        )
    };
    // 2. Append from `--append-system-prompt` (verbatim).
    if let Some(append) = cli.append_system_prompt.as_deref() {
        if !prompt.ends_with('\n') {
            prompt.push('\n');
        }
        prompt.push_str(append);
    }
    // 3. Append from `--append-system-prompt-file` (read once, fail
    //    fast if the file's missing rather than silently dropping).
    if let Some(path) = cli.append_system_prompt_file.as_deref() {
        let body = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("--append-system-prompt-file {path:?}: {e}"))?;
        if !prompt.ends_with('\n') {
            prompt.push('\n');
        }
        prompt.push_str(&body);
    }
    Ok(prompt)
}

/// Translate `--allowed-tools` / `--disallowed-tools` into a
/// [`coco_types::ToolFilter`]. Empty inputs ⇒ `unrestricted()`.
fn build_tool_filter(cli: &Cli) -> coco_types::ToolFilter {
    if cli.allowed_tools.is_empty() && cli.disallowed_tools.is_empty() {
        return coco_types::ToolFilter::unrestricted();
    }
    coco_types::ToolFilter::new(cli.allowed_tools.clone(), cli.disallowed_tools.clone())
}

/// Lightweight summary of the resolved tool filter for [`RunChatOutcome`].
/// Returns `None` when both `--allowed-tools` and `--disallowed-tools`
/// are empty (caller can equate that with `unrestricted`).
fn summarize_tool_filter(cli: &Cli) -> Option<ToolFilterSummary> {
    if cli.allowed_tools.is_empty() && cli.disallowed_tools.is_empty() {
        return None;
    }
    let mut allowed = cli.allowed_tools.clone();
    let mut disallowed = cli.disallowed_tools.clone();
    allowed.sort();
    disallowed.sort();
    Some(ToolFilterSummary {
        allowed,
        disallowed,
    })
}

/// Resolve `--add-dir` flag values to absolute paths anchored at `cwd`.
/// Used internally by `compose_system_prompt` to anchor `--add-dir`
/// paths for fence checks; callers that need the rendered display form
/// for the env block should use [`resolve_additional_dirs_display`].
fn resolve_additional_dirs(cli: &Cli, cwd: &Path) -> Vec<PathBuf> {
    cli.add_dir
        .iter()
        .map(|raw| {
            let p = Path::new(raw);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                cwd.join(p)
            }
        })
        .collect()
}

/// Public sibling of [`resolve_additional_dirs`] returning the display
/// form (`String`) that flows into `coco_context::build_system_prompt`'s
/// `additional_working_directories` slot. Single source of truth for the
/// `--add-dir` → env-block transformation; previously duplicated in
/// `session_bootstrap.rs` and `headless::compose_system_prompt`.
pub fn resolve_additional_dirs_display(cli: &Cli, cwd: &Path) -> Vec<String> {
    resolve_additional_dirs(cli, cwd)
        .iter()
        .map(|p| p.display().to_string())
        .collect()
}
