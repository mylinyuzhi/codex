use super::*;

#[test]
fn test_builtin_interceptors_registered() {
    let interceptor = get_interceptor("byted_model_hub");
    assert!(interceptor.is_some());
    assert_eq!(
        interceptor.as_ref().map(|i| i.name()),
        Some("byted_model_hub")
    );
}

#[test]
fn test_list_interceptors() {
    let interceptors = list_interceptors();
    assert!(interceptors.contains(&"byted_model_hub".to_string()));
}

#[test]
fn test_get_unknown_interceptor() {
    let interceptor = get_interceptor("unknown");
    assert!(interceptor.is_none());
}

#[test]
fn test_resolve_chain() {
    let chain = resolve_chain(&["byted_model_hub".to_string()]);
    assert_eq!(chain.len(), 1);
    assert_eq!(chain.names(), vec!["byted_model_hub"]);
}

#[test]
fn test_resolve_chain_with_unknown() {
    let chain = resolve_chain(&["byted_model_hub".to_string(), "unknown".to_string()]);
    // Only the known interceptor should be in the chain
    assert_eq!(chain.len(), 1);
}

#[test]
fn test_resolve_chain_empty() {
    let chain = resolve_chain(&[]);
    assert!(chain.is_empty());
}
