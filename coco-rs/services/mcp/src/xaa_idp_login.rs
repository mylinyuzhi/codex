//! IdP login orchestration for XAA.
//!
//! TS: `services/mcp/xaaIdpLogin.ts` — discovers the IdP via OIDC
//! well-known, fetches `token_endpoint`, and (when a fresh id_token is
//! required) walks the ROPC / client-credentials flow.
//!
//! This module implements the **discovery** pieces and the cached
//! id_token refresh. It does NOT run a browser-based flow; the user is
//! expected to have a pre-provisioned `id_token` (via `claude login`,
//! `gcloud auth print-identity-token`, etc.) that this module validates
//! + renews against the IdP.

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use tracing::debug;

use super::xaa::redact_tokens;

/// OIDC well-known discovery suffix.
pub const OIDC_WELL_KNOWN_SUFFIX: &str = "/.well-known/openid-configuration";

/// Default timeout for discovery requests.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

/// Subset of the OIDC discovery document that XAA needs.
#[derive(Debug, Clone, Deserialize)]
pub struct IdpMetadata {
    /// The IdP's token endpoint, fed into `XaaConfig::idp_token_endpoint`.
    pub token_endpoint: String,
    /// The IdP's issuer (for validating tokens).
    #[serde(default)]
    pub issuer: String,
    /// Optional jwks_uri (for signature validation, if callers want it).
    #[serde(default)]
    pub jwks_uri: Option<String>,
    /// Supported grant types; used to verify token-exchange is enabled.
    #[serde(default, rename = "grant_types_supported")]
    pub grant_types: Vec<String>,
}

/// Subset of the MCP Protected-Resource Metadata (PRM, RFC 9728).
#[derive(Debug, Clone, Deserialize)]
pub struct PrmMetadata {
    /// The Authorization Server's issuer URL.
    #[serde(rename = "authorization_servers")]
    pub authorization_servers: Vec<String>,
    /// The resource identifier for this MCP server.
    #[serde(default)]
    pub resource: Option<String>,
}

/// Errors from discovery/login helpers.
#[derive(Debug, thiserror::Error)]
pub enum IdpLoginError {
    #[error("http error: {0}")]
    Http(String),
    #[error("discovery returned {status}: {body}")]
    Discovery { status: u16, body: String },
    #[error("malformed metadata: {0}")]
    Malformed(String),
    #[error("token-exchange grant not supported by IdP")]
    TokenExchangeUnsupported,
}

/// Fetch the IdP's OIDC discovery document from its issuer URL.
///
/// `issuer_url` is typically something like `https://accounts.google.com`
/// or the company-specific identity URL. The well-known suffix is
/// appended automatically.
pub async fn discover_idp(client: &Client, issuer_url: &str) -> Result<IdpMetadata, IdpLoginError> {
    let trimmed = issuer_url.trim_end_matches('/');
    let url = format!("{trimmed}{OIDC_WELL_KNOWN_SUFFIX}");
    debug!(url, "idp_login: discovering OIDC metadata");

    let response = client
        .get(&url)
        .timeout(DISCOVERY_TIMEOUT)
        .send()
        .await
        .map_err(|e| IdpLoginError::Http(redact_tokens(&e.to_string())))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| IdpLoginError::Http(redact_tokens(&e.to_string())))?;

    if !status.is_success() {
        return Err(IdpLoginError::Discovery {
            status: status.as_u16(),
            body: redact_tokens(&body),
        });
    }

    let metadata: IdpMetadata = serde_json::from_str(&body)
        .map_err(|e| IdpLoginError::Malformed(format!("invalid discovery JSON: {e}")))?;

    if metadata.token_endpoint.is_empty() {
        return Err(IdpLoginError::Malformed(
            "token_endpoint missing from discovery metadata".into(),
        ));
    }
    Ok(metadata)
}

/// Fetch the Protected-Resource Metadata (RFC 9728) for an MCP server.
///
/// `resource_url` is the MCP server's base URL; PRM lives at
/// `{resource_url}/.well-known/oauth-protected-resource`.
pub async fn discover_prm(
    client: &Client,
    resource_url: &str,
) -> Result<PrmMetadata, IdpLoginError> {
    let trimmed = resource_url.trim_end_matches('/');
    let url = format!("{trimmed}/.well-known/oauth-protected-resource");
    debug!(url, "idp_login: discovering PRM");

    let response = client
        .get(&url)
        .timeout(DISCOVERY_TIMEOUT)
        .send()
        .await
        .map_err(|e| IdpLoginError::Http(redact_tokens(&e.to_string())))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| IdpLoginError::Http(redact_tokens(&e.to_string())))?;

    if !status.is_success() {
        return Err(IdpLoginError::Discovery {
            status: status.as_u16(),
            body: redact_tokens(&body),
        });
    }

    let metadata: PrmMetadata = serde_json::from_str(&body)
        .map_err(|e| IdpLoginError::Malformed(format!("invalid PRM JSON: {e}")))?;

    if metadata.authorization_servers.is_empty() {
        return Err(IdpLoginError::Malformed(
            "PRM has no authorization_servers".into(),
        ));
    }
    Ok(metadata)
}

/// Verify that the IdP advertises the token-exchange grant required for
/// XAA. Some enterprise IdPs support the full OIDC surface but omit
/// token-exchange; call this after `discover_idp` to fail fast.
pub fn ensure_token_exchange_supported(metadata: &IdpMetadata) -> Result<(), IdpLoginError> {
    if metadata.grant_types.is_empty() {
        // Some IdPs don't advertise grant_types_supported even though
        // they accept token-exchange. Be lenient and let the actual
        // exchange fail with a useful error rather than preemptively
        // blocking.
        return Ok(());
    }
    if metadata
        .grant_types
        .iter()
        .any(|g| g == super::xaa::TOKEN_EXCHANGE_GRANT)
    {
        Ok(())
    } else {
        Err(IdpLoginError::TokenExchangeUnsupported)
    }
}

#[cfg(test)]
#[path = "xaa_idp_login.test.rs"]
mod tests;
