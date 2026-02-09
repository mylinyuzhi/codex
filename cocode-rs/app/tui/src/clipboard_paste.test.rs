use super::*;

#[test]
fn test_media_type_from_path() {
    assert_eq!(
        media_type_from_path(Path::new("/tmp/photo.jpg")),
        Some("image/jpeg")
    );
    assert_eq!(
        media_type_from_path(Path::new("/tmp/photo.JPEG")),
        Some("image/jpeg")
    );
    assert_eq!(
        media_type_from_path(Path::new("/tmp/screenshot.png")),
        Some("image/png")
    );
    assert_eq!(
        media_type_from_path(Path::new("/tmp/anim.gif")),
        Some("image/gif")
    );
    assert_eq!(
        media_type_from_path(Path::new("/tmp/modern.webp")),
        Some("image/webp")
    );
    // Unsupported extensions
    assert_eq!(media_type_from_path(Path::new("/tmp/doc.pdf")), None);
    assert_eq!(media_type_from_path(Path::new("/tmp/noext")), None);
}
