use super::*;

#[test]
fn test_call_options_new() {
    let prompt = vec![];
    let options = LanguageModelV4CallOptions::new(prompt);
    assert_eq!(options.prompt.len(), 0);
    assert!(options.max_output_tokens.is_none());
    assert!(options.temperature.is_none());
}

#[test]
fn test_call_options_with_max_output_tokens() {
    let options = LanguageModelV4CallOptions::new(vec![]).with_max_output_tokens(1024);
    assert_eq!(options.max_output_tokens, Some(1024));
}

#[test]
fn test_call_options_with_temperature() {
    let options = LanguageModelV4CallOptions::new(vec![]).with_temperature(0.7);
    assert_eq!(options.temperature, Some(0.7));
}

#[test]
fn test_call_options_with_top_p() {
    let options = LanguageModelV4CallOptions::new(vec![]).with_top_p(0.9);
    assert_eq!(options.top_p, Some(0.9));
}

#[test]
fn test_call_options_with_stop_sequences() {
    let options = LanguageModelV4CallOptions::new(vec![])
        .with_stop_sequences(vec!["STOP".to_string(), "END".to_string()]);
    assert_eq!(
        options.stop_sequences,
        Some(vec!["STOP".to_string(), "END".to_string()])
    );
}

#[test]
fn test_call_options_builder_chain() {
    let options = LanguageModelV4CallOptions::new(vec![])
        .with_max_output_tokens(2048)
        .with_temperature(0.5);
    assert_eq!(options.max_output_tokens, Some(2048));
    assert_eq!(options.temperature, Some(0.5));
}
