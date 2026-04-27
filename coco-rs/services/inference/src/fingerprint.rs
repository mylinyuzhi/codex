//! `ProviderClientFingerprint` — turn-boundary coherence check.
//!
//! Closes the role-binding ↔ provider-client coherence gap (see
//! `multi-provider-plan.md` §11.1). At the start of every turn,
//! `QueryEngine` computes a fresh fingerprint from `RuntimeConfig`
//! and compares against `ApiClient::fingerprint()`. Mismatch → rebuild
//! the `Arc<dyn LanguageModelV4>`.
//!
//! Properties:
//!
//! - **Atomic with role-binding read.** Both `tool_overrides` and
//!   `api_client` are taken from the same `Arc<RuntimeConfig>`
//!   captured at turn start; they cannot diverge.
//! - **Cheap.** Equality compare over a byte-stable struct; no
//!   rebuild when nothing material changed (the common case during
//!   `settings.json` edits that touch only features).
//! - **Key rotation detected.** `api_key_origin_digest` is a SHA-256
//!   over the (env-var-name + resolved-secret) so we never store the
//!   live key. The digest itself is non-reversible.
//! - **`extra_body` is NOT in the fingerprint.** It is per-call
//!   (rebuilt every turn in `build_call_options`); changing it does
//!   not invalidate the cached client.

use coco_config::ProviderClientOptions;
use coco_config::ProviderConfig;
use coco_types::ProviderApi;
use coco_types::WireApi;
use sha2::Digest;
use sha2::Sha256;

/// Identity of the live `Arc<dyn LanguageModelV4>` for a (provider,
/// role) pair. Hashable, comparable, and intentionally **does not**
/// include any secret material in cleartext.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProviderClientFingerprint {
    pub provider: String,
    pub api: ProviderApi,
    pub api_model_name: String,
    pub base_url: String,
    /// Set only for `ProviderApi::Openai` (the lone API where the
    /// model_factory dispatches between `provider.chat()` and
    /// `provider.responses()`). For every other API the field is
    /// inert and would force a needless rebuild on toggle, so it is
    /// excluded from the digest by construction.
    pub wire_api: Option<WireApi>,
    /// SHA-256 over the canonical-JSON serialisation of
    /// `ProviderClientOptions`.
    pub client_options_digest: [u8; 32],
    pub timeout_secs: i64,
    /// SHA-256 over `(env_key_name, resolved_secret_or_empty)`. Detects
    /// rotated keys without storing the secret. Non-reversible.
    pub api_key_origin_digest: [u8; 32],
}

impl ProviderClientFingerprint {
    /// Build a fingerprint for a (provider, model_id) pair. The
    /// `api_model_name` is the resolved per-(provider, model)
    /// override — fall back to the role's `model_id` when no override
    /// is present.
    pub fn compute(provider_cfg: &ProviderConfig, api_model_name: &str) -> Self {
        Self {
            provider: provider_cfg.name.clone(),
            api: provider_cfg.api,
            api_model_name: api_model_name.to_string(),
            base_url: provider_cfg.base_url.clone(),
            wire_api: match provider_cfg.api {
                ProviderApi::Openai => Some(provider_cfg.wire_api),
                _ => None,
            },
            client_options_digest: digest_client_options(&provider_cfg.client_options),
            timeout_secs: provider_cfg.timeout_secs,
            api_key_origin_digest: digest_api_key_origin(provider_cfg),
        }
    }
}

/// SHA-256 with length-prefixed (be u64 + bytes) field encoding.
///
/// Length prefixing avoids any collision class via delimiter
/// confusion — the inverse mapping is unique. Each field is also
/// preceded by a one-byte tag so reordering produces a different
/// digest.
fn digest_client_options(opts: &ProviderClientOptions) -> [u8; 32] {
    let mut hasher = Sha256::new();
    // headers: tag 0x01, then count, then per-entry (key, value).
    update_u8(&mut hasher, 0x01);
    update_u64(&mut hasher, opts.headers.len() as u64);
    for (k, v) in &opts.headers {
        update_bytes(&mut hasher, k.as_bytes());
        update_bytes(&mut hasher, v.as_bytes());
    }
    update_u8(&mut hasher, 0x02);
    update_optional_bytes(
        &mut hasher,
        opts.auth_token.as_ref().map(|t| t.expose().as_bytes()),
    );
    update_u8(&mut hasher, 0x03);
    update_optional_bytes(
        &mut hasher,
        opts.organization_id.as_deref().map(str::as_bytes),
    );
    update_u8(&mut hasher, 0x04);
    update_optional_bytes(&mut hasher, opts.project_id.as_deref().map(str::as_bytes));
    update_u8(&mut hasher, 0x05);
    update_optional_bool(&mut hasher, opts.include_usage);
    update_u8(&mut hasher, 0x06);
    update_bool(&mut hasher, opts.full_url);
    update_u8(&mut hasher, 0x07);
    update_bool(&mut hasher, opts.supports_structured_outputs);
    hasher.finalize().into()
}

/// SHA-256 over `(env_key_name, resolved_secret_or_marker)` with
/// length-prefixed encoding. Detects rotated keys without storing
/// the secret; the digest is non-reversible.
fn digest_api_key_origin(cfg: &ProviderConfig) -> [u8; 32] {
    let mut hasher = Sha256::new();
    update_u8(&mut hasher, 0x10);
    update_bytes(&mut hasher, cfg.env_key.as_bytes());
    update_u8(&mut hasher, 0x11);
    if let Some(secret) = cfg.resolve_api_key() {
        update_optional_bytes(&mut hasher, Some(secret.as_bytes()));
    } else {
        update_optional_bytes(&mut hasher, None);
    }
    hasher.finalize().into()
}

fn update_u8(h: &mut Sha256, b: u8) {
    h.update([b]);
}

fn update_u64(h: &mut Sha256, n: u64) {
    h.update(n.to_be_bytes());
}

fn update_bytes(h: &mut Sha256, bytes: &[u8]) {
    update_u64(h, bytes.len() as u64);
    h.update(bytes);
}

fn update_optional_bytes(h: &mut Sha256, value: Option<&[u8]>) {
    match value {
        Some(b) => {
            update_u8(h, 0x01);
            update_bytes(h, b);
        }
        None => update_u8(h, 0x00),
    }
}

fn update_bool(h: &mut Sha256, b: bool) {
    update_u8(h, if b { 0x01 } else { 0x00 });
}

fn update_optional_bool(h: &mut Sha256, value: Option<bool>) {
    match value {
        Some(true) => update_u8(h, 0x02),
        Some(false) => update_u8(h, 0x01),
        None => update_u8(h, 0x00),
    }
}

#[cfg(test)]
#[path = "fingerprint.test.rs"]
mod tests;
