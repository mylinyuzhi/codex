use super::*;
use pretty_assertions::assert_eq;

#[test]
fn deny_unknown_fields_catches_typo() {
    let json = r#"{"orgization_id": "org-1"}"#;
    let err = serde_json::from_str::<PartialProviderClientOptions>(json).unwrap_err();
    assert!(
        err.to_string().contains("orgization_id") || err.to_string().contains("unknown field"),
        "expected unknown-field error, got: {err}"
    );
}

#[test]
fn deserialise_typed_options() {
    let json = r#"{
        "headers": {"X-Corp-Tenant": "engineering"},
        "organization_id": "org-myown",
        "include_usage": true
    }"#;
    let opts: PartialProviderClientOptions = serde_json::from_str(json).unwrap();
    assert_eq!(
        opts.headers.as_ref().unwrap().get("X-Corp-Tenant"),
        Some(&"engineering".into())
    );
    assert_eq!(opts.organization_id.as_deref(), Some("org-myown"));
    assert_eq!(opts.include_usage, Some(true));
    assert_eq!(opts.full_url, None);
}

#[test]
fn debug_redacts_auth_token() {
    let opts = ProviderClientOptions {
        auth_token: Some(crate::secret::RedactedSecret::new("sk-bearer-token")),
        ..Default::default()
    };
    let rendered = format!("{opts:?}");
    assert!(!rendered.contains("sk-bearer-token"));
    assert!(rendered.contains("<redacted>"));
}

#[test]
fn merge_partial_overlays_each_some_field() {
    let mut base = ProviderClientOptions {
        headers: BTreeMap::from([("X-Base".into(), "1".into())]),
        organization_id: Some("org-base".into()),
        full_url: false,
        ..Default::default()
    };
    let overlay = PartialProviderClientOptions {
        headers: Some(BTreeMap::from([("X-Overlay".into(), "2".into())])),
        full_url: Some(true),
        ..Default::default()
    };
    base.merge_partial(&overlay);
    assert_eq!(base.headers.get("X-Base"), Some(&"1".to_string()));
    assert_eq!(base.headers.get("X-Overlay"), Some(&"2".to_string()));
    assert_eq!(base.organization_id.as_deref(), Some("org-base"));
    assert!(base.full_url);
}
