use super::*;

#[test]
fn test_default_options_are_one_shot_cache_safe() {
    // The default shape is the cache-safe one used by `/btw` /
    // promptSuggestion / sessionMemory: 1 turn, skip transcript,
    // skip cache write, no effort override.
    let opts = ForkedAgentOptions::default();
    assert_eq!(opts.max_turns, Some(1));
    assert!(opts.skip_transcript);
    assert!(opts.skip_cache_write);
    assert!(
        opts.effort.is_none(),
        "effort override busts cache; default must be None"
    );
    assert_eq!(opts.query_source, "fork");
}

#[test]
fn test_one_shot_options_overrides_query_source() {
    let opts = one_shot_options("/btw");
    assert_eq!(opts.query_source, "/btw");
    assert_eq!(opts.max_turns, Some(1));
}
