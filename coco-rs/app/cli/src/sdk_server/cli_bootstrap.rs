//! CLI-side [`InitializeBootstrap`] implementation.
//!
//! Concentrates the cross-subsystem data sources for the `initialize`
//! wire response so every source (commands, output styles, agents,
//! auth) lives in one place rather than spraying 5+ fields across
//! `SdkServerState`. The server holds an `Arc<dyn InitializeBootstrap>`
//! trait object; the concrete impl below knows about every subsystem
//! and wires them together at CLI startup.
//!
//! Fields that require richer cross-crate plumbing (agent discovery,
//! ApiClient auth exposure) return stub values today and will be
//! filled in as their data sources grow an accessor.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use coco_commands::CommandRegistry;
use coco_inference::auth::AuthMethod;
use coco_types::AgentDefinition;
use coco_types::FastModeState;
use coco_types::SdkAccountInfo;
use coco_types::SdkAgentInfo;
use coco_types::SdkApiProvider;
use coco_types::SdkSlashCommand;

use crate::sdk_server::handlers::InitializeBootstrap;

/// Built-in output style names shipped with coco-rs. Matches TS
/// `OUTPUT_STYLE_CONFIG` at `constants/outputStyles.ts:41-135` which uses
/// a lowercase `"default"` sentinel plus capitalized `"Explanatory"` and
/// `"Learning"`. Case matters: TS clients looking up a style by name do
/// an exact-string match.
pub const BUILTIN_OUTPUT_STYLES: &[&str] = &["default", "Explanatory", "Learning"];

/// Concrete [`InitializeBootstrap`] wired from CLI startup.
///
/// Holds `Arc` references to the data sources so the trait object can
/// be cheaply shared between `SdkServerState` and any future consumers.
/// Each accessor reads from its paired field — missing sources return
/// empty / default values instead of erroring so `initialize` is always
/// a successful handshake.
pub struct CliInitializeBootstrap {
    /// Slash-command registry populated at CLI startup (built-ins +
    /// plugin + user markdown). `None` disables `commands`.
    pub command_registry: Option<Arc<CommandRegistry>>,
    /// Current output style from `Settings.output_style`; defaults to
    /// `"default"`.
    pub output_style: String,
    /// User / project directories to walk for custom output-style
    /// markdown files. Built-ins from [`BUILTIN_OUTPUT_STYLES`] are
    /// always included.
    pub output_style_dirs: Vec<PathBuf>,
    /// User / project directories to walk for custom agent definition
    /// markdown files. Built-ins from
    /// [`coco_tools::tools::agent_spawn::builtin_agents`] are always
    /// included on top.
    pub agent_dirs: Vec<PathBuf>,
    /// Resolved auth method for the active session. Controls how
    /// `account()` maps to [`coco_types::SdkAccountInfo`]. `None` →
    /// no auth configured, empty account.
    pub auth_method: Option<Arc<AuthMethod>>,
}

impl CliInitializeBootstrap {
    /// Construct a new provider with only the output style wired.
    /// Other sources default to empty until explicitly set.
    pub fn new(output_style: String) -> Self {
        Self {
            command_registry: None,
            output_style,
            output_style_dirs: Vec::new(),
            agent_dirs: Vec::new(),
            auth_method: None,
        }
    }

    pub fn with_command_registry(mut self, registry: Arc<CommandRegistry>) -> Self {
        self.command_registry = Some(registry);
        self
    }

    pub fn with_output_style_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.output_style_dirs = dirs;
        self
    }

    pub fn with_agent_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.agent_dirs = dirs;
        self
    }

    pub fn with_auth_method(mut self, auth: AuthMethod) -> Self {
        self.auth_method = Some(Arc::new(auth));
        self
    }
}

#[async_trait]
impl InitializeBootstrap for CliInitializeBootstrap {
    async fn commands(&self) -> Vec<SdkSlashCommand> {
        let Some(registry) = self.command_registry.as_ref() else {
            return Vec::new();
        };
        // `sdk_safe()` is strictly tighter than `visible()`: it also
        // filters `is_sensitive` so remote SDK clients never see
        // command names / descriptions / argument hints for commands
        // flagged as sensitive (even though local TUI completions
        // may show them).
        registry
            .sdk_safe()
            .iter()
            .map(|cmd| SdkSlashCommand {
                name: cmd.base.name.clone(),
                description: cmd.base.description.clone(),
                // TS `argumentHint` is REQUIRED (not optional). When
                // coco-rs has no hint, we advertise an empty string so
                // strict zod parsers accept the response.
                argument_hint: cmd.base.argument_hint.clone().unwrap_or_default(),
            })
            .collect()
    }

    async fn agents(&self) -> Vec<SdkAgentInfo> {
        let dirs = self.agent_dirs.clone();
        tokio::task::spawn_blocking(move || {
            let mut defs = coco_tools::tools::agent_spawn::builtin_agents();
            defs.extend(coco_tools::tools::agent_spawn::load_agents_from_dirs(&dirs));
            // User-defined agents with the same name as a built-in
            // override the built-in — HashMap::insert replaces on
            // duplicate key, and `load_agents_from_dirs` appends after
            // built-ins so the user-defined entries land second and win.
            let mut by_name: std::collections::HashMap<String, AgentDefinition> =
                std::collections::HashMap::new();
            for def in defs {
                by_name.insert(def.name.clone(), def);
            }
            let mut out: Vec<SdkAgentInfo> =
                by_name.into_values().map(def_to_sdk_agent_info).collect();
            out.sort_by(|a, b| a.name.cmp(&b.name));
            out
        })
        .await
        .unwrap_or_else(|_| {
            // spawn_blocking panicked inside the closure. Fall back to
            // the built-in set so `initialize.agents` is never empty
            // just because a markdown file had a parse bug.
            coco_tools::tools::agent_spawn::builtin_agents()
                .into_iter()
                .map(def_to_sdk_agent_info)
                .collect()
        })
    }

    async fn account(&self) -> SdkAccountInfo {
        match self.auth_method.as_deref() {
            Some(auth) => auth_method_to_account(auth),
            None => SdkAccountInfo::default(),
        }
    }

    async fn output_style(&self) -> String {
        self.output_style.clone()
    }

    async fn available_output_styles(&self) -> Vec<String> {
        let dirs = self.output_style_dirs.clone();
        tokio::task::spawn_blocking(move || discover_output_styles(&dirs))
            .await
            .unwrap_or_else(|_| BUILTIN_OUTPUT_STYLES.iter().map(|s| (*s).into()).collect())
    }

    async fn fast_mode_state(&self) -> Option<FastModeState> {
        // Fast-mode state is runtime-tracked by the rate limiter and
        // not currently exposed. Advertise `None` until an accessor
        // lands.
        None
    }
}

/// Shared projection from a coco-rs [`AgentDefinition`] to the TS wire
/// [`SdkAgentInfo`] shape. Missing descriptions become `""` to satisfy
/// the TS `z.string()` (required) schema.
fn def_to_sdk_agent_info(def: AgentDefinition) -> SdkAgentInfo {
    SdkAgentInfo {
        name: def.name,
        description: def.description.unwrap_or_default(),
        model: def.model,
    }
}

/// Map a resolved [`AuthMethod`] to the TS-aligned `SdkAccountInfo`.
///
/// Wire semantics match TS `getAccountInformation()` in
/// `src/utils/auth.ts:1863-1906`:
///
/// - **Third-party providers** (Bedrock / Vertex / Foundry): TS returns
///   `undefined` from `getAccountInformation()` because those backends
///   use external credentials. coco-rs returns [`SdkAccountInfo::default`]
///   (all fields `None`) which serializes to `{}` on the wire — the
///   closest analogue to TS `undefined` that the optional `account`
///   field can carry.
/// - **First-party OAuth**: `api_provider = FirstParty`,
///   `subscription_type` from the token, `organization` is the raw
///   `org_uuid` (TS fetches the human-readable name via a separate API
///   call we don't make yet). `token_source` is intentionally `None`
///   because TS's canonical token-source strings
///   (`CLAUDE_CODE_OAUTH_TOKEN`, `claude.ai`, etc.) don't map cleanly
///   from the coco-rs `AuthMethod::OAuth` variant — sending an
///   incompatible string would mislead TS SDK consumers that key on
///   those values. `email` is always `None` (OAuth token doesn't embed
///   it).
/// - **First-party API key**: `api_provider = FirstParty` only.
///   `api_key_source` stays `None` until coco-rs tracks the env var /
///   helper origin — TS's canonical values are from
///   `ApiKeySourceSchema` (`user` / `project` / `org` / `temporary` /
///   `oauth`).
pub fn auth_method_to_account(auth: &AuthMethod) -> SdkAccountInfo {
    match auth {
        AuthMethod::OAuth(tokens) => SdkAccountInfo {
            email: None,
            organization: tokens.org_uuid.clone(),
            subscription_type: tokens.subscription_type.clone(),
            token_source: None,
            api_key_source: None,
            api_provider: Some(SdkApiProvider::FirstParty),
        },
        AuthMethod::ApiKey { .. } => SdkAccountInfo {
            api_provider: Some(SdkApiProvider::FirstParty),
            ..Default::default()
        },
        // Third-party provider paths: return a bare default so the wire
        // shape matches TS's `undefined` semantics. TS SDK consumers
        // that check `account.apiProvider === undefined` to detect 3P
        // auth would otherwise see a populated apiProvider and treat
        // the session as "logged in".
        AuthMethod::Bedrock { .. } | AuthMethod::Vertex { .. } | AuthMethod::Foundry { .. } => {
            SdkAccountInfo::default()
        }
    }
}

/// Walk the given dirs for `*.md` output style definitions, merge with
/// the built-in list, sort and deduplicate. File names (without
/// extension) become style identifiers.
pub fn discover_output_styles(dirs: &[std::path::PathBuf]) -> Vec<String> {
    let mut styles: Vec<String> = BUILTIN_OUTPUT_STYLES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                styles.push(stem.to_string());
            }
        }
    }
    styles.sort();
    styles.dedup();
    styles
}

#[cfg(test)]
#[path = "cli_bootstrap.test.rs"]
mod tests;
