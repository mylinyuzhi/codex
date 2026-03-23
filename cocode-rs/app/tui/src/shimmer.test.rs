use super::*;

#[test]
fn test_shimmer_empty_string() {
    let spans = shimmer_spans("");
    assert!(spans.is_empty());
}

#[test]
fn test_shimmer_produces_one_span_per_char() {
    let spans = shimmer_spans("hello");
    assert_eq!(spans.len(), 5);
}

#[test]
fn test_shimmer_single_char() {
    let spans = shimmer_spans("x");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].content.as_ref(), "x");
}

#[test]
fn test_fallback_style_dim_for_low_intensity() {
    let style = fallback_style(0.0);
    assert!(style.add_modifier.contains(Modifier::DIM));
}

#[test]
fn test_fallback_style_bold_for_high_intensity() {
    let style = fallback_style(0.8);
    assert!(style.add_modifier.contains(Modifier::BOLD));
}
