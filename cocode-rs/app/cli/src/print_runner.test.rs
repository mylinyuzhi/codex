use super::OutputFormat;

#[test]
fn test_output_format_parse_text() {
    assert!(matches!(OutputFormat::parse("text"), OutputFormat::Text));
}

#[test]
fn test_output_format_parse_json() {
    assert!(matches!(OutputFormat::parse("json"), OutputFormat::Json));
}

#[test]
fn test_output_format_parse_stream_json() {
    assert!(matches!(
        OutputFormat::parse("stream-json"),
        OutputFormat::StreamJson
    ));
    assert!(matches!(
        OutputFormat::parse("streaming-json"),
        OutputFormat::StreamJson
    ));
}

#[test]
fn test_output_format_parse_unknown_defaults_to_text() {
    assert!(matches!(OutputFormat::parse("xml"), OutputFormat::Text));
    assert!(matches!(OutputFormat::parse(""), OutputFormat::Text));
}
