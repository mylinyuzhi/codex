use super::*;

#[test]
fn test_embed_options() {
    let options = EmbedOptions::new("text-embedding-3-small", "Hello").with_dimensions(512);

    assert!(options.model.is_string());
    assert_eq!(options.dimensions, Some(512));
}

#[test]
fn test_embed_many_options() {
    let options = EmbedManyOptions::new(
        "text-embedding-3-small",
        vec!["Hello".to_string(), "World".to_string()],
    );

    assert_eq!(options.values.len(), 2);
}
