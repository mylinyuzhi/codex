use super::*;

#[test]
fn test_default_animation() {
    let anim = Animation::default();
    let frame = anim.current_frame();
    assert!(!frame.is_empty());
}

#[test]
fn test_variant_count() {
    assert!(Animation::variant_count() >= 4);
}

#[test]
fn test_next_variant_wraps() {
    let mut anim = Animation::new(0);
    let count = Animation::variant_count();
    for _ in 0..count {
        anim.next_variant();
    }
    // Should wrap back to 0
    assert_eq!(anim.variant_idx, 0);
}

#[test]
fn test_clamped_variant() {
    let anim = Animation::new(999);
    let frame = anim.current_frame();
    assert!(!frame.is_empty());
}

#[test]
fn test_frame_is_non_empty() {
    for variant in 0..Animation::variant_count() {
        let anim = Animation::new(variant);
        assert!(
            !anim.current_frame().is_empty(),
            "Variant {variant} produced empty frame"
        );
    }
}
