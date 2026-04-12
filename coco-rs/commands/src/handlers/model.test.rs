use super::*;

#[test]
fn test_resolve_model_alias() {
    let m = resolve_model("sonnet").unwrap();
    assert_eq!(m.alias, "sonnet");
    assert_eq!(m.full_id, "claude-sonnet-4-20250514");
}

#[test]
fn test_resolve_model_alias_case_insensitive() {
    let m = resolve_model("OPUS").unwrap();
    assert_eq!(m.alias, "opus");
}

#[test]
fn test_resolve_model_full_id() {
    let m = resolve_model("claude-haiku-3-20250307").unwrap();
    assert_eq!(m.alias, "haiku");
}

#[test]
fn test_resolve_model_prefix() {
    let m = resolve_model("claude-sonnet").unwrap();
    assert_eq!(m.alias, "sonnet");
}

#[test]
fn test_resolve_model_unknown() {
    assert!(resolve_model("gpt-4").is_none());
    assert!(resolve_model("llama").is_none());
}

#[test]
fn test_levenshtein_identical() {
    assert_eq!(levenshtein("sonnet", "sonnet"), 0);
}

#[test]
fn test_levenshtein_one_edit() {
    assert_eq!(levenshtein("sonnet", "sonnt"), 1);
    assert_eq!(levenshtein("sonnet", "sonnett"), 1);
}

#[test]
fn test_levenshtein_different() {
    assert!(levenshtein("sonnet", "haiku") > 3);
}

#[test]
fn test_list_models_output() {
    let output = list_models();
    assert!(output.contains("sonnet"));
    assert!(output.contains("opus"));
    assert!(output.contains("haiku"));
    assert!(output.contains("Available Models"));
    assert!(output.contains("$/M in"));
}

#[tokio::test]
async fn test_handler_no_args() {
    let output = handler(String::new()).await.unwrap();
    assert!(output.contains("Available Models"));
    assert!(output.contains("sonnet"));
}

#[tokio::test]
async fn test_handler_valid_model() {
    let output = handler("opus".to_string()).await.unwrap();
    assert!(output.contains("switched to"));
    assert!(output.contains("claude-opus-4-20250514"));
    assert!(output.contains("Pricing"));
}

#[tokio::test]
async fn test_handler_unknown_model() {
    let output = handler("gpt-4".to_string()).await.unwrap();
    assert!(output.contains("Unknown model"));
}

#[tokio::test]
async fn test_handler_custom_provider_model() {
    let output = handler("openai/gpt-4-turbo".to_string()).await.unwrap();
    assert!(output.contains("custom model"));
}
