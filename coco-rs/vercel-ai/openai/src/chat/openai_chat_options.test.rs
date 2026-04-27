use super::*;
use serde_json::json;
use std::collections::HashMap;
use vercel_ai_provider::ProviderOptions;

/// `raw` is verbatim — every key (typed-known and unknown) appears
/// in the returned map. Patching this into the wire body lets the
/// user's `extra_body` win over typed body writes at the same key.
#[test]
fn raw_map_includes_every_key_verbatim() {
    let mut inner = HashMap::new();
    inner.insert("user".into(), json!("uid")); // typed-known
    inner.insert("myCustom".into(), json!(true)); // unknown
    let mut outer = HashMap::new();
    outer.insert("openai".into(), inner);
    let po = Some(ProviderOptions(outer));

    let (typed, raw) = extract_openai_options(&po);
    assert_eq!(typed.user.as_deref(), Some("uid"));
    // Both typed-known and unknown keys appear — no filter.
    assert!(raw.contains_key("user"));
    assert!(raw.contains_key("myCustom"));
}
