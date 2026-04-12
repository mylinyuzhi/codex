use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use serde::Deserialize;
use serde::Serialize;

/// TTL for cached API keys from helper commands.
const API_KEY_CACHE_TTL: Duration = Duration::from_secs(300);

/// OAuth token storage file name.
const OAUTH_TOKEN_FILE: &str = "oauth_tokens.json";

/// Cached API key with fetch timestamp.
struct CachedApiKey {
    key: String,
    fetched_at: Instant,
}

impl CachedApiKey {
    fn is_valid(&self) -> bool {
        self.fetched_at.elapsed() < API_KEY_CACHE_TTL
    }
}

/// Global cache keyed by helper command string.
fn api_key_cache() -> &'static Mutex<HashMap<String, CachedApiKey>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedApiKey>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Authentication method for API access.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Direct API key.
    ApiKey { key: String },
    /// OAuth tokens (with refresh).
    OAuth(OAuthTokens),
    /// AWS Bedrock (uses AWS SDK auth).
    Bedrock {
        region: String,
        profile: Option<String>,
    },
    /// GCP Vertex AI.
    Vertex { project_id: String, region: String },
    /// Azure Foundry.
    Foundry { endpoint: String },
}

/// OAuth token set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    /// Subscription type from token response (pro, max, team, enterprise).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_type: Option<String>,
    /// Organization UUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_uuid: Option<String>,
}

impl OAuthTokens {
    /// Whether the access token has expired.
    pub fn is_expired(&self, now_ms: i64) -> bool {
        self.expires_at.is_some_and(|exp| now_ms >= exp)
    }

    /// Whether the token needs refresh (expired or within 5 min of expiry).
    pub fn needs_refresh(&self, now_ms: i64) -> bool {
        self.expires_at.is_some_and(|exp| now_ms >= exp - 300_000)
    }
}

/// Login method enforcement.
///
/// TS: getAuthTokenSource() priority chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginMethod {
    /// Direct API key.
    ApiKey,
    /// OAuth flow.
    OAuth,
    /// API key from helper command.
    ApiKeyHelper,
}

/// Auth resolution options.
#[derive(Debug, Clone, Default)]
pub struct AuthResolveOptions {
    /// Config directory for OAuth token persistence (e.g., ~/.cocode/).
    pub config_dir: Option<PathBuf>,
    /// API key helper command from settings.
    pub api_key_helper: Option<String>,
    /// Force a specific login method.
    pub force_login_method: Option<LoginMethod>,
    /// Bare mode: no external auth, env-only.
    pub bare_mode: bool,
}

/// Resolve the auth method from environment variables and config.
///
/// TS: services/api/client.ts — environment-based provider detection.
/// Priority order:
/// 1. ANTHROPIC_AUTH_TOKEN env var (raw auth token)
/// 2. ANTHROPIC_API_KEY env var → ApiKey
/// 3. apiKeyHelper command → ApiKey
/// 4. AWS credentials + AWS_REGION → Bedrock
/// 5. ANTHROPIC_FOUNDRY_RESOURCE → Foundry
/// 6. ANTHROPIC_VERTEX_PROJECT_ID → Vertex
/// 7. Stored OAuth tokens → OAuth
///
/// If `force_login_method` is set, only that method is tried.
/// If `bare_mode` is true, only env vars are checked (no stored tokens, no helpers).
pub fn resolve_auth(options: &AuthResolveOptions) -> Option<AuthMethod> {
    // Bare mode: env-only, no external auth
    if options.bare_mode {
        return resolve_auth_from_env();
    }

    // Force specific login method
    if let Some(method) = &options.force_login_method {
        return match method {
            LoginMethod::ApiKey => resolve_api_key_from_env(),
            LoginMethod::OAuth => {
                load_stored_oauth_tokens(options.config_dir.as_deref()).map(AuthMethod::OAuth)
            }
            LoginMethod::ApiKeyHelper => options
                .api_key_helper
                .as_ref()
                .and_then(|cmd| get_api_key_from_helper(cmd).map(|key| AuthMethod::ApiKey { key })),
        };
    }

    // Full priority chain

    // 1. ANTHROPIC_AUTH_TOKEN (raw token — used by bridge/CCR)
    if let Ok(token) = std::env::var("ANTHROPIC_AUTH_TOKEN") {
        if !token.is_empty() {
            return Some(AuthMethod::ApiKey { key: token });
        }
    }

    // 2. Direct API key
    if let Some(auth) = resolve_api_key_from_env() {
        return Some(auth);
    }

    // 3. API key helper
    if let Some(cmd) = &options.api_key_helper {
        if let Some(key) = get_api_key_from_helper(cmd) {
            return Some(AuthMethod::ApiKey { key });
        }
    }

    // 4-6. Cloud providers
    if let Some(auth) = resolve_cloud_provider_from_env() {
        return Some(auth);
    }

    // 7. Stored OAuth tokens
    if let Some(tokens) = load_stored_oauth_tokens(options.config_dir.as_deref()) {
        return Some(AuthMethod::OAuth(tokens));
    }

    None
}

/// Resolve auth from environment variables only (backward-compatible).
pub fn resolve_auth_from_env() -> Option<AuthMethod> {
    if let Some(auth) = resolve_api_key_from_env() {
        return Some(auth);
    }
    resolve_cloud_provider_from_env()
}

/// Try to resolve API key from ANTHROPIC_API_KEY env var.
fn resolve_api_key_from_env() -> Option<AuthMethod> {
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            return Some(AuthMethod::ApiKey { key });
        }
    }
    None
}

/// Try to resolve cloud provider auth from env vars.
fn resolve_cloud_provider_from_env() -> Option<AuthMethod> {
    // AWS Bedrock
    if std::env::var("AWS_REGION").is_ok() || std::env::var("AWS_DEFAULT_REGION").is_ok() {
        let region = std::env::var("AWS_REGION")
            .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
            .unwrap_or_else(|_| "us-east-1".to_string());
        let profile = std::env::var("AWS_PROFILE").ok();
        if std::env::var("AWS_ACCESS_KEY_ID").is_ok()
            || std::env::var("AWS_BEARER_TOKEN_BEDROCK").is_ok()
        {
            return Some(AuthMethod::Bedrock { region, profile });
        }
    }

    // Azure Foundry
    if let Ok(endpoint) = std::env::var("ANTHROPIC_FOUNDRY_RESOURCE") {
        return Some(AuthMethod::Foundry { endpoint });
    }

    // Vertex AI
    if let Ok(project_id) = std::env::var("ANTHROPIC_VERTEX_PROJECT_ID") {
        let region = std::env::var("CLOUD_ML_REGION").unwrap_or_else(|_| "us-east5".to_string());
        return Some(AuthMethod::Vertex { project_id, region });
    }

    None
}

// ── OAuth token persistence ──

/// Save OAuth tokens to disk for session persistence.
pub fn save_oauth_tokens(config_dir: &Path, tokens: &OAuthTokens) -> anyhow::Result<()> {
    std::fs::create_dir_all(config_dir)?;
    let path = config_dir.join(OAUTH_TOKEN_FILE);
    let json = serde_json::to_string_pretty(tokens)?;

    // Write atomically via temp file
    let tmp_path = path.with_extension("tmp");
    let mut file = std::fs::File::create(&tmp_path)?;
    file.write_all(json.as_bytes())?;
    file.sync_all()?;
    std::fs::rename(tmp_path, path)?;

    Ok(())
}

/// Load stored OAuth tokens from disk.
pub fn load_stored_oauth_tokens(config_dir: Option<&Path>) -> Option<OAuthTokens> {
    let config_dir = config_dir?;
    let path = config_dir.join(OAUTH_TOKEN_FILE);
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str::<OAuthTokens>(&content).ok()
}

/// Remove stored OAuth tokens (logout).
pub fn clear_stored_oauth_tokens(config_dir: &Path) -> anyhow::Result<()> {
    let path = config_dir.join(OAUTH_TOKEN_FILE);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

// ── API key helper ──

/// Get API key from a helper command (e.g., api_key_helper in settings).
///
/// Results are cached for 5 minutes per command string to avoid repeated
/// subprocess invocations on every API call.
///
/// TS: getApiKeyFromApiKeyHelper() -- runs a command to get the key.
pub fn get_api_key_from_helper(helper_command: &str) -> Option<String> {
    // Check cache first.
    if let Ok(cache) = api_key_cache().lock() {
        if let Some(entry) = cache.get(helper_command) {
            if entry.is_valid() {
                return Some(entry.key.clone());
            }
        }
    }

    // Cache miss or expired -- run the command.
    let key = run_helper_command(helper_command)?;

    // Store in cache.
    if let Ok(mut cache) = api_key_cache().lock() {
        cache.insert(
            helper_command.to_string(),
            CachedApiKey {
                key: key.clone(),
                fetched_at: Instant::now(),
            },
        );
    }

    Some(key)
}

/// Execute the helper command and return the trimmed output.
fn run_helper_command(helper_command: &str) -> Option<String> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(helper_command)
        .output()
        .ok()?;

    if output.status.success() {
        let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !key.is_empty() {
            return Some(key);
        }
    }
    None
}

/// Check if the resolved auth method uses a first-party Anthropic endpoint.
pub fn is_first_party_auth(auth: &AuthMethod) -> bool {
    matches!(auth, AuthMethod::ApiKey { .. } | AuthMethod::OAuth(_))
}

#[cfg(test)]
#[path = "auth.test.rs"]
mod tests;
