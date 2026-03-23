use super::*;

#[test]
fn test_is_light_dark_background() {
    assert!(!is_light((0, 0, 0))); // Black
    assert!(!is_light((30, 30, 30))); // Dark gray
    assert!(!is_light((85, 85, 85))); // Medium dark
}

#[test]
fn test_is_light_light_background() {
    assert!(is_light((255, 255, 255))); // White
    assert!(is_light((200, 200, 200))); // Light gray
    assert!(is_light((170, 170, 170))); // Medium light
}

#[test]
fn test_blend_fully_opaque() {
    let result = blend((255, 0, 0), (0, 0, 255), 1.0);
    assert_eq!(result, (255, 0, 0));
}

#[test]
fn test_blend_fully_transparent() {
    let result = blend((255, 0, 0), (0, 0, 255), 0.0);
    assert_eq!(result, (0, 0, 255));
}

#[test]
fn test_blend_half() {
    let result = blend((200, 100, 0), (0, 100, 200), 0.5);
    assert_eq!(result, (100, 100, 100));
}

#[test]
fn test_blend_clamps_alpha() {
    let result = blend((255, 0, 0), (0, 0, 255), 2.0);
    assert_eq!(result, (255, 0, 0)); // Clamped to 1.0

    let result = blend((255, 0, 0), (0, 0, 255), -1.0);
    assert_eq!(result, (0, 0, 255)); // Clamped to 0.0
}

#[test]
fn test_ansi_index_to_rgb_basic() {
    assert_eq!(ansi_index_to_rgb(0), Some((0, 0, 0))); // Black
    assert_eq!(ansi_index_to_rgb(15), Some((255, 255, 255))); // White
    assert_eq!(ansi_index_to_rgb(255), None); // Out of range
}
