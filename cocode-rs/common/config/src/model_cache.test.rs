use super::*;

#[test]
fn test_model_cache_new() {
    let cache = ModelCache::new();
    assert!(cache.cache.is_empty());
}

#[test]
fn test_model_cache_insert_and_get() {
    let mut cache = ModelCache::new();
    let spec = ModelSpec::new("openai", "gpt-4");
    let info = ModelInfo {
        slug: "gpt-4".to_string(),
        display_name: Some("GPT-4".to_string()),
        context_window: Some(8192),
        ..Default::default()
    };

    cache.insert(spec.clone(), info);
    assert_eq!(cache.cache.len(), 1);
    assert!(cache.get(&spec).is_some());
    assert_eq!(cache.get(&spec).unwrap().slug, "gpt-4");
}

#[test]
fn test_model_cache_multiple_entries() {
    let mut cache = ModelCache::new();
    let spec1 = ModelSpec::new("openai", "gpt-4");
    let spec2 = ModelSpec::new("openai", "gpt-3.5");
    let spec3 = ModelSpec::new("anthropic", "claude-3");

    let info = ModelInfo {
        slug: "test".to_string(),
        ..Default::default()
    };

    cache.insert(spec1.clone(), info.clone());
    cache.insert(spec2.clone(), info.clone());
    cache.insert(spec3.clone(), info);

    assert_eq!(cache.cache.len(), 3);
    assert!(cache.get(&spec1).is_some());
    assert!(cache.get(&spec2).is_some());
    assert!(cache.get(&spec3).is_some());
}

#[test]
fn test_model_cache_clear() {
    let mut cache = ModelCache::new();
    let spec = ModelSpec::new("openai", "gpt-4");
    let info = ModelInfo {
        slug: "gpt-4".to_string(),
        ..Default::default()
    };

    cache.insert(spec, info);
    assert_eq!(cache.cache.len(), 1);

    cache.cache.clear();
    assert!(cache.cache.is_empty());
}

#[test]
fn test_model_cache_into_inner() {
    let mut cache = ModelCache::new();
    let spec = ModelSpec::new("openai", "gpt-4");
    let info = ModelInfo {
        slug: "gpt-4".to_string(),
        ..Default::default()
    };

    cache.insert(spec, info);
    let inner = cache.into_inner();
    assert_eq!(inner.len(), 1);
}
