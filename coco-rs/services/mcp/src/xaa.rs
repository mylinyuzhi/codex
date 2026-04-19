//! Cross-App Access (XAA) / Enterprise Managed Authorization.
//!
//! TS: `services/mcp/xaa.ts` (SEP-990, ID-JAG spec).
//!
//! Obtains an MCP access token WITHOUT a browser consent screen by
//! chaining:
//!   1. RFC 8693 Token Exchange at the IdP: `id_token` → ID-JAG
//!   2. RFC 7523 JWT Bearer Grant at the AS: ID-JAG → `access_token`
//!
//! # Scope
//!
//! This module implements the wire-level token exchange + bearer grant
//! primitives, plus the two-leg orchestration. PRM discovery (RFC 9728)
//! is separated into `xaa_idp_login.rs`; callers that already know the
//! IdP's `token_endpoint` can call [`exchange_id_token_for_jag`] and
//! [`exchange_jag_for_access_token`] directly.
//!
//! Redacts tokens from debug logs via a pattern matching the TS
//! implementation — failing to do so leaks credentials if a misbehaving
//! AS echoes the subject_token in an error body.

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use serde::Serialize;
use tracing::debug;
use tracing::warn;

/// Default HTTP timeout for XAA requests (TS: `XAA_REQUEST_TIMEOUT_MS`).
pub const XAA_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// `urn:ietf:params:oauth:grant-type:token-exchange` (RFC 8693).
pub const TOKEN_EXCHANGE_GRANT: &str = "urn:ietf:params:oauth:grant-type:token-exchange";

/// `urn:ietf:params:oauth:grant-type:jwt-bearer` (RFC 7523).
pub const JWT_BEARER_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";

/// ID-JAG token type (IETF draft: `draft-ietf-oauth-identity-assertion-authz-grant`).
pub const ID_JAG_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:id-jag";

/// `urn:ietf:params:oauth:token-type:id_token` — id_token subject type.
pub const ID_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:id_token";

/// Configuration for an XAA token exchange.
#[derive(Debug, Clone)]
pub struct XaaConfig {
    /// Client ID registered at the AS (MCP server's resource identifier).
    pub as_client_id: String,
    /// IdP's token endpoint (from OIDC well-known discovery).
    pub idp_token_endpoint: String,
    /// IdP's client ID (the IdP-registered identity for this app).
    pub idp_client_id: String,
    /// Authorization server's token endpoint (from PRM / well-known).
    pub as_token_endpoint: String,
    /// Audience claim: typically the MCP server's resource URL.
    pub audience: String,
    /// Optional scope to request.
    pub scope: Option<String>,
    /// IdP client secret (for confidential-client token exchange). When
    /// `None`, the caller should inject client_secret via `client_id`
    /// alone (public client flow), which many IdPs don't allow for
    /// token exchange — the orchestrator checks this.
    pub idp_client_secret: Option<String>,
}

/// Successful XAA orchestration result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XaaResult {
    /// The AS-issued access token (what the MCP client sends as a
    /// bearer token on subsequent requests).
    pub access_token: String,
    /// Seconds until `access_token` expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<i64>,
    /// Token type — almost always `"Bearer"`.
    #[serde(default)]
    pub token_type: String,
    /// The ID-JAG from the first leg (retained for observability / audit).
    /// Intentionally not returned to callers in normal flows; field
    /// included so tests and integration scripts can verify the
    /// two-leg exchange actually ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_jag_length: Option<usize>,
}

/// Errors returned by the XAA exchange path.
#[derive(Debug, thiserror::Error)]
pub enum XaaError {
    #[error("http error: {0}")]
    Http(String),
    #[error("provider returned {status}: {body}")]
    Provider {
        status: u16,
        body: String,
        /// Whether callers should drop any cached id_token because the
        /// IdP deemed it invalid (TS: `shouldClearIdToken`).
        should_clear_id_token: bool,
    },
    #[error("malformed response: {0}")]
    MalformedResponse(String),
    #[error("config error: {0}")]
    Config(String),
}

/// RFC 8693 Token Exchange at the IdP.
///
/// Exchange an `id_token` (opaque to this function — caller provides it
/// from their existing OIDC session) for an ID-JAG that the downstream
/// Authorization Server can bearer-grant against.
pub async fn exchange_id_token_for_jag(
    client: &Client,
    config: &XaaConfig,
    id_token: &str,
) -> Result<String, XaaError> {
    let mut form: Vec<(&str, String)> = vec![
        ("grant_type", TOKEN_EXCHANGE_GRANT.into()),
        ("client_id", config.idp_client_id.clone()),
        ("subject_token", id_token.to_string()),
        ("subject_token_type", ID_TOKEN_TYPE.into()),
        ("requested_token_type", ID_JAG_TOKEN_TYPE.into()),
        ("audience", config.as_client_id.clone()),
    ];
    if let Some(scope) = &config.scope {
        form.push(("scope", scope.clone()));
    }
    if let Some(secret) = &config.idp_client_secret {
        form.push(("client_secret", secret.clone()));
    }

    debug!(endpoint = %config.idp_token_endpoint, "xaa: leg 1 (id_token -> ID-JAG)");
    let response = client
        .post(&config.idp_token_endpoint)
        .timeout(XAA_REQUEST_TIMEOUT)
        .form(&form)
        .send()
        .await
        .map_err(|e| XaaError::Http(redact_tokens(&e.to_string())))?;

    let status = response.status();
    let body_text = response
        .text()
        .await
        .map_err(|e| XaaError::Http(redact_tokens(&e.to_string())))?;

    if !status.is_success() {
        let should_clear = should_clear_id_token_on_status(status.as_u16(), &body_text);
        warn!(
            status = status.as_u16(),
            clear_id = should_clear,
            "xaa: IdP token exchange failed"
        );
        return Err(XaaError::Provider {
            status: status.as_u16(),
            body: redact_tokens(&body_text),
            should_clear_id_token: should_clear,
        });
    }

    let parsed: TokenExchangeResponse = serde_json::from_str(&body_text).map_err(|e| {
        // Protocol violation with a 2xx — safer to drop the cached
        // id_token so we don't keep hitting a broken flow.
        XaaError::MalformedResponse(format!("invalid JSON in 200 body: {e}"))
    })?;

    if parsed.issued_token_type.as_deref() != Some(ID_JAG_TOKEN_TYPE) {
        return Err(XaaError::MalformedResponse(format!(
            "expected issued_token_type={ID_JAG_TOKEN_TYPE}, got {:?}",
            parsed.issued_token_type
        )));
    }
    if parsed.access_token.is_empty() {
        return Err(XaaError::MalformedResponse("empty access_token".into()));
    }
    Ok(parsed.access_token)
}

/// RFC 7523 JWT Bearer Grant at the AS.
///
/// Present the ID-JAG (from `exchange_id_token_for_jag`) as a JWT
/// assertion and receive the AS-issued access token.
pub async fn exchange_jag_for_access_token(
    client: &Client,
    config: &XaaConfig,
    id_jag: &str,
) -> Result<XaaResult, XaaError> {
    let mut form: Vec<(&str, String)> = vec![
        ("grant_type", JWT_BEARER_GRANT.into()),
        ("assertion", id_jag.to_string()),
        ("client_id", config.as_client_id.clone()),
    ];
    if let Some(scope) = &config.scope {
        form.push(("scope", scope.clone()));
    }

    debug!(endpoint = %config.as_token_endpoint, "xaa: leg 2 (ID-JAG -> access_token)");
    let response = client
        .post(&config.as_token_endpoint)
        .timeout(XAA_REQUEST_TIMEOUT)
        .form(&form)
        .send()
        .await
        .map_err(|e| XaaError::Http(redact_tokens(&e.to_string())))?;

    let status = response.status();
    let body_text = response
        .text()
        .await
        .map_err(|e| XaaError::Http(redact_tokens(&e.to_string())))?;

    if !status.is_success() {
        return Err(XaaError::Provider {
            status: status.as_u16(),
            body: redact_tokens(&body_text),
            // 4xx/5xx on the AS leg does NOT invalidate the id_token;
            // only the IdP owns that decision.
            should_clear_id_token: false,
        });
    }

    let parsed: AccessTokenResponse = serde_json::from_str(&body_text)
        .map_err(|e| XaaError::MalformedResponse(format!("invalid JSON: {e}")))?;

    if parsed.access_token.is_empty() {
        return Err(XaaError::MalformedResponse("empty access_token".into()));
    }
    Ok(XaaResult {
        access_token: parsed.access_token,
        expires_in: parsed.expires_in,
        token_type: parsed.token_type.unwrap_or_else(|| "Bearer".into()),
        id_jag_length: Some(id_jag.len()),
    })
}

/// Orchestrator: run both legs in sequence, returning the AS-issued
/// access token. Intended as the normal entry point.
pub async fn exchange_id_token(
    client: &Client,
    config: &XaaConfig,
    id_token: &str,
) -> Result<XaaResult, XaaError> {
    let id_jag = exchange_id_token_for_jag(client, config, id_token).await?;
    exchange_jag_for_access_token(client, config, &id_jag).await
}

// ── Private types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenExchangeResponse {
    access_token: String,
    issued_token_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AccessTokenResponse {
    access_token: String,
    expires_in: Option<i64>,
    token_type: Option<String>,
}

/// Decide whether a non-2xx IdP response should invalidate the cached
/// id_token. TS semantics: 4xx / invalid_grant / invalid_token → clear;
/// 5xx → keep (server-side glitch).
fn should_clear_id_token_on_status(status: u16, body: &str) -> bool {
    if (500..600).contains(&status) {
        return false;
    }
    // 4xx: trust the server
    if (400..500).contains(&status) {
        return true;
    }
    // Other non-2xx: rare, default to clearing
    if body.contains("invalid_grant") || body.contains("invalid_token") {
        return true;
    }
    false
}

/// Redact any token-bearing JSON fields from `raw`. Matches TS
/// `SENSITIVE_TOKEN_RE` — crucial for not leaking the subject_token on
/// 4xx error bodies that echo it back.
pub(crate) fn redact_tokens(raw: &str) -> String {
    const SENSITIVE: &[&str] = &[
        "access_token",
        "refresh_token",
        "id_token",
        "assertion",
        "subject_token",
        "client_secret",
    ];
    let mut out = raw.to_string();
    for key in SENSITIVE {
        // Match the JSON form `"key":"<anything without a quote>"` and
        // replace the value with `[REDACTED]`. This is a best-effort
        // text transform, not a JSON-aware rewrite.
        //
        // Advance `search_from` past each redaction so the scan
        // doesn't re-match the same key on the next iteration (which
        // would be an infinite loop since we only replace the value).
        let pattern = format!("\"{key}\":");
        let mut search_from = 0usize;
        while let Some(rel_start) = out[search_from..].find(&pattern) {
            let start = search_from + rel_start;
            let value_start = start + pattern.len();
            let rest = &out[value_start..];
            if let Some(q1) = rest.find('"') {
                let value_content_start = value_start + q1 + 1;
                if let Some(q2) = out[value_content_start..].find('"') {
                    let value_content_end = value_content_start + q2;
                    out.replace_range(value_content_start..value_content_end, "[REDACTED]");
                    // Continue scanning *after* the redacted region so
                    // subsequent occurrences of the same key still get
                    // found, but the just-replaced one is skipped.
                    search_from = value_content_start + "[REDACTED]".len();
                    continue;
                }
            }
            // Couldn't find a terminating quote; bail on this key
            break;
        }
    }
    out
}

#[cfg(test)]
#[path = "xaa.test.rs"]
mod tests;
