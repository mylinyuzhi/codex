use super::*;

#[test]
fn test_spinner_frame_returns_valid_braille() {
    let anim = Animation::new();
    let frame = anim.spinner_frame();
    assert!(BRAILLE_FRAMES.contains(&frame));
}

#[test]
fn test_spinner_frame_style_returns_valid_frame() {
    let anim = Animation::new();
    for style in [
        SpinnerStyle::Braille,
        SpinnerStyle::Dots,
        SpinnerStyle::Line,
        SpinnerStyle::Bounce,
        SpinnerStyle::Arrow,
    ] {
        let frame = anim.spinner_frame_style(style);
        assert!(
            style.frames().contains(&frame),
            "frame {frame:?} not found in {style:?} frames"
        );
    }
}

#[test]
fn test_shimmer_alpha_range() {
    let anim = Animation::new();
    for offset in -5..5 {
        let alpha = anim.shimmer_alpha(offset);
        assert!(
            (0.0..=1.0).contains(&alpha),
            "shimmer_alpha({offset}) = {alpha}, out of range"
        );
    }
}

#[test]
fn test_shimmer_char_returns_base_char() {
    let (ch, _style) = shimmer_char('X', /*elapsed_ms*/ 500, /*offset*/ 0);
    assert_eq!(ch, 'X');
}

#[test]
fn test_glimmer_style_does_not_panic() {
    // Just verify it produces a style without panicking for various times.
    for ms in [0, 100, 400, 800, 1600, 10_000] {
        let _ = glimmer_style(ms);
    }
}

#[test]
fn test_default_impl() {
    let anim = Animation::default();
    let frame = anim.spinner_frame();
    assert!(BRAILLE_FRAMES.contains(&frame));
}
