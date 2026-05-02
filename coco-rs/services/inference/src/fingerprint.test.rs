use super::*;
use coco_config::PartialProviderClientOptions;
use coco_config::PartialProviderConfig;
use coco_config::ProviderConfig;
use coco_config::RedactedSecret;
use coco_types::ProviderApi;
use pretty_assertions::assert_eq;

fn provider_cfg(name: &str, base_url: &str, api_key: Option<RedactedSecret>) -> ProviderConfig {
    let partial = PartialProviderConfig {
        api: Some(ProviderApi::Openai),
        env_key: Some("OPENAI_API_KEY".into()),
        api_key,
        base_url: Some(base_url.into()),
        ..Default::default()
    };
    ProviderConfig::from_partial(name, &partial).unwrap()
}

#[test]
fn fingerprint_unchanged_for_identical_config() {
    let cfg = provider_cfg("openai", "https://api.openai.com/v1", None);
    let a = ProviderClientFingerprint::compute(&cfg, "gpt-5");
    let b = ProviderClientFingerprint::compute(&cfg, "gpt-5");
    assert_eq!(a, b);
}

#[test]
fn fingerprint_changes_when_base_url_changes() {
    let cfg_a = provider_cfg("openai", "https://api.openai.com/v1", None);
    let cfg_b = provider_cfg("openai", "https://corp-proxy/v1", None);
    let a = ProviderClientFingerprint::compute(&cfg_a, "gpt-5");
    let b = ProviderClientFingerprint::compute(&cfg_b, "gpt-5");
    assert_ne!(a, b);
}

#[test]
fn fingerprint_changes_when_api_model_name_changes() {
    let cfg = provider_cfg("openai", "https://api.openai.com/v1", None);
    let a = ProviderClientFingerprint::compute(&cfg, "gpt-5");
    let b = ProviderClientFingerprint::compute(&cfg, "gpt-5-2025-04");
    assert_ne!(a, b);
}

#[test]
fn fingerprint_changes_when_secret_rotates() {
    let cfg_a = provider_cfg(
        "openai",
        "https://api.openai.com/v1",
        Some(RedactedSecret::new("sk-original")),
    );
    let cfg_b = provider_cfg(
        "openai",
        "https://api.openai.com/v1",
        Some(RedactedSecret::new("sk-rotated")),
    );
    let a = ProviderClientFingerprint::compute(&cfg_a, "gpt-5");
    let b = ProviderClientFingerprint::compute(&cfg_b, "gpt-5");
    assert_ne!(a.api_key_origin_digest, b.api_key_origin_digest);
}

#[test]
fn fingerprint_to_snapshot_carries_identity_fields() {
    // The thin DTO that crosses into `coco-types::agent_ipc` keeps
    // every identity-distinguishing field but drops the digests.
    let cfg = provider_cfg("openai-prod", "https://api.openai.com/v1", None);
    let fp = ProviderClientFingerprint::compute(&cfg, "gpt-5");
    let snap = fp.to_snapshot();
    assert_eq!(snap.provider, "openai-prod");
    assert_eq!(snap.api, ProviderApi::Openai);
    assert_eq!(snap.api_model_name, "gpt-5");
    assert_eq!(snap.base_url, "https://api.openai.com/v1");
    // Openai → wire_api is set; other APIs would be None.
    assert!(snap.wire_api.is_some(), "Openai must populate wire_api");
}

#[test]
fn fingerprint_to_snapshot_drops_anthropic_wire_api() {
    // Anthropic's `wire_api` is inert (always Chat); the fingerprint
    // intentionally stores `None` there so toggling the value doesn't
    // force a rebuild. The DTO must mirror that.
    let partial = PartialProviderConfig {
        api: Some(ProviderApi::Anthropic),
        env_key: Some("ANTHROPIC_API_KEY".into()),
        base_url: Some("https://api.anthropic.com".into()),
        ..Default::default()
    };
    let cfg = ProviderConfig::from_partial("anthropic", &partial).unwrap();
    let fp = ProviderClientFingerprint::compute(&cfg, "claude-opus-4-7");
    let snap = fp.to_snapshot();
    assert_eq!(snap.api, ProviderApi::Anthropic);
    assert_eq!(snap.wire_api, None);
}

#[test]
fn fingerprint_changes_when_client_options_headers_differ() {
    let mut cfg_a = provider_cfg("openai", "https://api.openai.com/v1", None);
    let mut cfg_b = provider_cfg("openai", "https://api.openai.com/v1", None);
    let overlay = PartialProviderClientOptions {
        headers: Some(
            [("X-Tenant".to_string(), "team-a".to_string())]
                .into_iter()
                .collect(),
        ),
        ..Default::default()
    };
    cfg_a.client_options.merge_partial(&overlay);
    let overlay_b = PartialProviderClientOptions {
        headers: Some(
            [("X-Tenant".to_string(), "team-b".to_string())]
                .into_iter()
                .collect(),
        ),
        ..Default::default()
    };
    cfg_b.client_options.merge_partial(&overlay_b);
    let a = ProviderClientFingerprint::compute(&cfg_a, "gpt-5");
    let b = ProviderClientFingerprint::compute(&cfg_b, "gpt-5");
    assert_ne!(a.client_options_digest, b.client_options_digest);
}
