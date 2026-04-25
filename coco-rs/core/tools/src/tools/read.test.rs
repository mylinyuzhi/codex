use crate::tools::read::ReadTool;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolUseContext;
use serde_json::json;

// ── R7-T25: read description content check ──
//
// Regression guard: the description must include the multimodal
// capabilities (images/PDF/notebooks), the 2000-line default, and
// the cat -n format hint. Without these the model won't discover
// the tool's full surface.
#[test]
fn test_read_description_mentions_multimodal_capabilities() {
    let desc = ReadTool.description(&serde_json::Value::Null, &DescriptionOptions::default());
    assert!(desc.contains("PNG"), "missing image format hint");
    assert!(desc.contains("PDF"), "missing PDF support hint");
    assert!(
        desc.contains("Jupyter notebook") || desc.contains(".ipynb"),
        "missing notebook hint"
    );
    assert!(desc.contains("2000 lines"), "missing 2000-line default");
    assert!(desc.contains("cat -n"), "missing cat -n format hint");
}

#[tokio::test]
async fn test_read_basic_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hello.txt");
    std::fs::write(&file, "line one\nline two\nline three\n").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let text = result.data["file"]["content"].as_str().unwrap();
    assert!(text.contains("1\tline one"));
    assert!(text.contains("2\tline two"));
    assert!(text.contains("3\tline three"));
}

/// TS `FileReadTool.ts:1020` treats `offset` as 1-based — the input
/// corresponds directly to the line number visible in editors. `offset: 10`
/// must start at line 10 (not line 11, which was the pre-fix 0-based
/// behavior). The rendered line numbers are 1-based too.
#[tokio::test]
async fn test_read_with_offset_limit() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("lines.txt");
    let content: String = (1..=100).map(|i| format!("line {i}\n")).collect();
    std::fs::write(&file, &content).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "offset": 10, "limit": 5}),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data["file"]["content"].as_str().unwrap();
    assert!(text.contains("10\tline 10"), "got: {text}");
    assert!(text.contains("14\tline 14"), "got: {text}");
    assert!(!text.contains("15\tline 15"), "got: {text}");
    assert!(text.contains("more lines not shown"));
}

/// TS special-cases `offset: 0` and `offset: 1` to both mean "start from
/// the first line" (`FileReadTool.ts:1020`). Regression guard.
#[tokio::test]
async fn test_read_offset_zero_and_one_equivalent() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("start.txt");
    std::fs::write(&file, "first\nsecond\nthird\n").unwrap();

    let ctx = ToolUseContext::test_default();
    for offset in [0_u64, 1] {
        let result = ReadTool
            .execute(
                json!({"file_path": file.to_str().unwrap(), "offset": offset, "limit": 1}),
                &ctx,
            )
            .await
            .unwrap();
        let text = result.data["file"]["content"].as_str().unwrap();
        assert!(
            text.contains("1\tfirst"),
            "offset={offset} should start at line 1; got: {text}"
        );
    }
}

#[tokio::test]
async fn test_read_nonexistent_file() {
    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": "/nonexistent/file.txt"}), &ctx)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn test_read_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("empty.txt");
    std::fs::File::create(&file).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let text = result.data["file"]["content"].as_str().unwrap();
    assert!(text.contains("empty"));
}

/// Helper — generate real image bytes in the requested format. Returns
/// a 1x1 pixel valid image that the `image` crate can decode and
/// round-trip. Replaces the old fake-byte fixtures which broke when
/// the D1 two-stage compression pipeline started actually decoding.
fn real_image_bytes(format: image::ImageFormat) -> Vec<u8> {
    use image::ColorType;
    use image::ImageEncoder;
    use image::codecs::gif::GifEncoder;
    use image::codecs::jpeg::JpegEncoder;
    use image::codecs::png::PngEncoder;
    use image::codecs::webp::WebPEncoder;
    // 1x1 RGBA image: one opaque red pixel.
    let pixel_rgba = [255u8, 0, 0, 255];
    let mut buf = Vec::new();
    match format {
        image::ImageFormat::Png => {
            PngEncoder::new(&mut buf)
                .write_image(&pixel_rgba, 1, 1, ColorType::Rgba8.into())
                .unwrap();
        }
        image::ImageFormat::Jpeg => {
            // JPEG needs RGB (no alpha).
            let pixel_rgb = [255u8, 0, 0];
            JpegEncoder::new_with_quality(&mut buf, 85)
                .write_image(&pixel_rgb, 1, 1, ColorType::Rgb8.into())
                .unwrap();
        }
        image::ImageFormat::Gif => {
            let frame =
                image::Frame::new(image::RgbaImage::from_raw(1, 1, pixel_rgba.to_vec()).unwrap());
            let mut encoder = GifEncoder::new(&mut buf);
            encoder.encode_frame(frame).unwrap();
        }
        image::ImageFormat::WebP => {
            WebPEncoder::new_lossless(&mut buf)
                .write_image(&pixel_rgba, 1, 1, ColorType::Rgba8.into())
                .unwrap();
        }
        _ => panic!("unsupported test format"),
    }
    buf
}

#[tokio::test]
async fn test_read_image_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    std::fs::write(&file, real_image_bytes(image::ImageFormat::Png)).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    // Post-D1: the image goes through resize+re-encode and returns as a
    // multimodal block with processed bytes.
    assert_eq!(result.data["type"], "image");
}

#[tokio::test]
async fn test_read_directory_error() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": dir.path().to_str().unwrap()}), &ctx)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("directory"));
}

#[tokio::test]
async fn test_read_binary_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");
    std::fs::write(&file, b"\x00\x01\x02\x03").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let text = result.data["file"]["content"].as_str().unwrap();
    assert!(text.contains("binary"));
}

// ---------------------------------------------------------------------------
// B2.1: base64 image encoding + device blocklist + offset-beyond-file
// ---------------------------------------------------------------------------

/// PNG/JPG/GIF/WEBP files get returned as a structured multimodal image
/// block: `{type: image, source: {type: base64, media_type, data}}`.
/// TS: `FileReadTool.ts:250-254, 396-397`. Images are processed through
/// the D1 resize+re-encode pipeline, so the returned media_type may
/// differ from the filename hint (e.g. WebP with alpha might be
/// downgraded to PNG).
#[tokio::test]
async fn test_read_png_returns_base64_block() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pixel.png");
    std::fs::write(&file, real_image_bytes(image::ImageFormat::Png)).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let data = &result.data;
    // TS-shaped image envelope: `{ type: 'image', file: { base64,
    // type, originalSize } }`. The old `source.type=base64` discriminator
    // was removed when the output schema migrated to TS-aligned shapes
    // — `file.type` now holds the MIME directly.
    assert_eq!(data["type"], "image");
    assert_eq!(data["file"]["type"], "image/png");
    let b64 = data["file"]["base64"].as_str().unwrap();
    assert!(!b64.is_empty(), "base64 data should not be empty");
    assert!(
        data["file"]["originalSize"].as_u64().is_some(),
        "originalSize should be populated"
    );
}

#[tokio::test]
async fn test_read_jpeg_returns_base64_block() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.jpg");
    std::fs::write(&file, real_image_bytes(image::ImageFormat::Jpeg)).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    assert_eq!(result.data["type"], "image");
    assert_eq!(result.data["file"]["type"], "image/jpeg");
}

/// `jpeg` extension also maps to image/jpeg (same as `jpg`).
#[tokio::test]
async fn test_read_jpeg_alt_extension() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.jpeg");
    std::fs::write(&file, real_image_bytes(image::ImageFormat::Jpeg)).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.data["file"]["type"], "image/jpeg");
}

#[tokio::test]
async fn test_read_webp_returns_base64_block() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("image.webp");
    std::fs::write(&file, real_image_bytes(image::ImageFormat::WebP)).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.data["file"]["type"], "image/webp");
}

#[tokio::test]
async fn test_read_gif_returns_base64_block() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("anim.gif");
    std::fs::write(&file, real_image_bytes(image::ImageFormat::Gif)).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();
    // GIF round-trips through coco-utils-image which re-encodes to PNG
    // for formats without a lossless path (GIF falls into this bucket
    // because the image crate can't encode-to-GIF without extra
    // features). The model still receives a valid image, just with
    // a different media_type.
    let media = result.data["file"]["type"].as_str().unwrap();
    assert!(
        media == "image/gif" || media == "image/png",
        "GIF should round-trip as image/gif or image/png, got {media}"
    );
}

/// Regression guard: images larger than `MAX_WIDTH × MAX_HEIGHT`
/// (2048 × 768 in coco-utils-image) must be resized by the pipeline,
/// proving the D1 two-stage compression is actually wired up. We
/// generate a 4096×1536 PNG — well over both dimension caps — and
/// verify the output base64 is smaller than the input raw bytes.
#[tokio::test]
async fn test_read_large_image_gets_resized() {
    use image::ColorType;
    use image::ImageEncoder;
    use image::codecs::png::PngEncoder;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("huge.png");

    // 4096×1536 all-red image → ~24MB of raw RGBA.
    let w = 4096u32;
    let h = 1536u32;
    let pixel_count = (w * h) as usize;
    let mut rgba = Vec::with_capacity(pixel_count * 4);
    for _ in 0..pixel_count {
        rgba.extend_from_slice(&[255u8, 0, 0, 255]);
    }
    let mut encoded = Vec::new();
    PngEncoder::new(&mut encoded)
        .write_image(&rgba, w, h, ColorType::Rgba8.into())
        .unwrap();
    let original_size = encoded.len();
    std::fs::write(&file, &encoded).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    // Sanity: still a valid image block.
    assert_eq!(result.data["type"], "image");
    // Extract the base64 and decode to verify it's smaller than the
    // original. We also check the decoded length (raw bytes after
    // base64 decode) because base64 inflates by ~4/3.
    let b64 = result.data["file"]["base64"].as_str().unwrap();
    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    assert!(
        decoded.len() < original_size,
        "resized image ({} bytes) should be smaller than original ({} bytes)",
        decoded.len(),
        original_size
    );

    // R7-T20: dimensions metadata. originalWidth/originalHeight should
    // reflect the source 4096×1536, displayWidth/displayHeight should
    // be the post-resize size (≤ MAX_WIDTH × MAX_HEIGHT = 2048×768).
    let dims = &result.data["file"]["dimensions"];
    assert_eq!(dims["originalWidth"], 4096);
    assert_eq!(dims["originalHeight"], 1536);
    let display_w = dims["displayWidth"].as_u64().unwrap();
    let display_h = dims["displayHeight"].as_u64().unwrap();
    assert!(
        display_w <= 2048 && display_h <= 768,
        "displayWidth/Height should be capped to MAX bounds, got {display_w}x{display_h}"
    );
    // Resized must be strictly smaller in at least one dimension.
    assert!(display_w < 4096 || display_h < 1536);
}

/// R7-T20: every successful Read must push the file path into
/// `ctx.nested_memory_attachment_triggers` so the next-turn message
/// builder can attach any nested CLAUDE.md memories from the file's
/// ancestry. TS `FileReadTool.ts:848,870,1038` does the same.
#[tokio::test]
async fn test_read_populates_nested_memory_triggers() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("note.md");
    std::fs::write(&file, "some content\n").unwrap();

    let ctx = ToolUseContext::test_default();
    let _ = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let triggers = ctx.nested_memory_attachment_triggers.read().await;
    let canonical = std::fs::canonicalize(&file).unwrap();
    let canonical_str = canonical.display().to_string();
    assert!(
        triggers.contains(&canonical_str),
        "expected {canonical_str} in nested_memory_attachment_triggers, got: {triggers:?}"
    );
}

/// Small image (under MAX_WIDTH × MAX_HEIGHT) should round-trip with
/// display dimensions equal to original dimensions.
#[tokio::test]
async fn test_read_small_image_dimensions_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("small.png");
    std::fs::write(&file, real_image_bytes(image::ImageFormat::Png)).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let dims = &result.data["file"]["dimensions"];
    // 1×1 fixture from real_image_bytes — both original and display
    // should report 1×1.
    assert_eq!(dims["originalWidth"], 1);
    assert_eq!(dims["originalHeight"], 1);
    assert_eq!(dims["displayWidth"], 1);
    assert_eq!(dims["displayHeight"], 1);
}

/// SVG is an intentionally-unsupported format (raster-only image crate
/// + Anthropic API doesn't accept it). SVGs get the placeholder response
/// like BMP/TIFF/ICO. Verified alignment: TS `FileReadTool.ts:188`
/// `IMAGE_EXTENSIONS` does NOT include SVG either.
#[tokio::test]
async fn test_read_svg_returns_placeholder_not_base64() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("logo.svg");
    std::fs::write(&file, b"<svg></svg>").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();
    // SVG is in PLACEHOLDER_IMAGE_EXTENSIONS, so we return a text
    // placeholder — not a multimodal image block.
    let text = result.data["file"]["content"].as_str().unwrap();
    assert!(text.contains("svg"));
    assert!(text.contains("not supported"));
}

/// BMP/ICO/TIFF still get a placeholder (not supported by Anthropic
/// multimodal API). These aren't errors — the tool tells the model the
/// file exists and hints at conversion.
#[tokio::test]
async fn test_read_bmp_returns_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("legacy.bmp");
    std::fs::write(&file, b"BM").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();
    let text = result.data["file"]["content"].as_str().unwrap();
    assert!(text.contains("bmp"));
    assert!(text.contains("not supported"));
}

/// Blocked device paths must be rejected with InvalidInput. These paths
/// never get `std::fs::read_to_string` called on them, so there's no risk
/// of hanging on /dev/stdin.
#[tokio::test]
async fn test_read_blocks_dev_zero() {
    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": "/dev/zero"}), &ctx)
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("device"), "error should mention device: {err}");
}

#[tokio::test]
async fn test_read_blocks_dev_stdin() {
    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": "/dev/stdin"}), &ctx)
        .await;
    assert!(result.is_err());
}

/// `/dev/null` is NOT blocked — it's a common sink and reading returns EOF
/// immediately. Opening it for read is harmless.
#[tokio::test]
async fn test_read_dev_null_is_not_blocked() {
    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": "/dev/null"}), &ctx)
        .await;
    // Either succeeds (treating as empty file) or fails with a different
    // reason — the key assertion is the error is NOT "cannot read device
    // file" which would only come from the blocklist.
    if let Err(e) = &result {
        assert!(
            !e.to_string().contains("Cannot read device file"),
            "/dev/null must not be blocklisted"
        );
    }
}

// ---------------------------------------------------------------------------
// B2.2: encoding detection
// ---------------------------------------------------------------------------

/// UTF-8 BOM files should be read correctly. TS: `readFileSyncWithMetadata`
/// strips the BOM. coco-file-encoding detects `Utf8WithBom` and decodes
/// the same content.
#[tokio::test]
async fn test_read_utf8_with_bom() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bom.txt");
    // UTF-8 BOM + "hello\nworld"
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"hello\nworld\n");
    std::fs::write(&file, &bytes).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let text = result.data["file"]["content"].as_str().unwrap();
    assert!(text.contains("hello"), "got: {text}");
    assert!(text.contains("world"));
    // BOM should not appear in the decoded content.
    assert!(!text.contains('\u{FEFF}'), "BOM must be stripped: {text}");
}

/// UTF-16LE files (with BOM `FF FE`) must decode correctly instead of
/// erroring on "invalid UTF-8".
#[tokio::test]
async fn test_read_utf16le_bom() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("utf16.txt");

    // UTF-16LE BOM + "hi\n" encoded as u16 little-endian pairs.
    let mut bytes = vec![0xFF, 0xFE]; // BOM
    for ch in "hi\n".chars() {
        let code = ch as u16;
        bytes.extend_from_slice(&code.to_le_bytes());
    }
    std::fs::write(&file, &bytes).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let text = result.data["file"]["content"].as_str().unwrap();
    assert!(text.contains("hi"), "should decode UTF-16LE: {text}");
}

// ---------------------------------------------------------------------------
// R6-T20: file-read permission gate on check_permissions
// ---------------------------------------------------------------------------

/// When `file_path` has no matching ignore pattern, check_permissions
/// returns Allow. This is the default (no env var set) scenario.
#[tokio::test]
async fn test_read_check_permissions_allows_default() {
    use coco_types::PermissionDecision;
    let decision = ReadTool
        .check_permissions(
            &json!({"file_path": "/tmp/ordinary.txt"}),
            &ToolUseContext::test_default(),
        )
        .await;
    assert!(matches!(decision, PermissionDecision::Allow { .. }));
}

// ---------------------------------------------------------------------------
// R6-T16: PDF extraction
// ---------------------------------------------------------------------------

use crate::tools::read::parse_page_range;

/// Single-page spec: `"3"` → `(3, 3)`.
#[test]
fn test_parse_page_range_single() {
    assert_eq!(parse_page_range("3", 10), Some((3, 3)));
    assert_eq!(parse_page_range("  5  ", 10), Some((5, 5)));
}

/// Dash range: `"1-5"` → `(1, 5)`.
#[test]
fn test_parse_page_range_dash() {
    assert_eq!(parse_page_range("1-5", 10), Some((1, 5)));
    assert_eq!(parse_page_range("2 - 4", 10), Some((2, 4)));
}

/// Over-range is clamped to the document length.
#[test]
fn test_parse_page_range_clamps_to_total() {
    assert_eq!(parse_page_range("1-100", 10), Some((1, 10)));
}

/// Invalid specs return `None`.
#[test]
fn test_parse_page_range_invalid() {
    assert_eq!(parse_page_range("0", 10), None, "page 0 is invalid");
    assert_eq!(parse_page_range("abc", 10), None, "non-numeric");
    assert_eq!(parse_page_range("5-1", 10), None, "end < start");
    assert_eq!(parse_page_range("", 10), None, "empty spec");
}

/// A malformed PDF file must surface as ExecutionFailed without panic.
#[tokio::test]
async fn test_read_malformed_pdf_errors_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("broken.pdf");
    // Not a valid PDF — just random bytes.
    std::fs::write(&file, b"not a real pdf").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await;

    assert!(result.is_err(), "malformed PDF should error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("PDF") || err.contains("pdf") || err.contains("extract"),
        "error should mention PDF extraction: {err}"
    );
}

/// R5-T12: files with very long lines must respect the byte cap.
/// TS `FileReadTool.ts` applies both line AND byte caps; coco-rs
/// previously only capped lines, so minified files could emit
/// megabytes of text. Regression guard.
#[tokio::test]
async fn test_read_byte_cap_on_long_lines() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("huge.json");
    // One 500K-char line — well under the 2000-line cap but way over
    // the 256K byte cap.
    let long_line: String = "x".repeat(500_000);
    std::fs::write(&file, &long_line).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let text = result.data["file"]["content"].as_str().unwrap();
    // The first line is emitted in full (the `!output.is_empty()` guard
    // avoids returning zero content on single-giant-line files), but
    // any subsequent content should be cut. Here there's only one line,
    // so the cap doesn't kick in — we just verify the output starts
    // with a line-number prefix and doesn't exceed a generous upper
    // bound when the file has multiple long lines.
    assert!(text.starts_with("1\t"), "output should start with line 1");
}

/// When multiple long lines are present, the byte cap stops emission
/// after the budget is exhausted.
#[tokio::test]
async fn test_read_byte_cap_multiple_long_lines() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("many.txt");
    // 10 lines × 50K chars each = 500K. Byte cap is 256K, so we
    // should see ~5-6 lines before truncation.
    let line: String = "y".repeat(50_000);
    let content: String = (0..10).map(|_| line.clone()).collect::<Vec<_>>().join("\n");
    std::fs::write(&file, &content).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let text = result.data["file"]["content"].as_str().unwrap();
    assert!(
        text.contains("byte limit"),
        "truncation footer should mention byte limit: {}",
        &text[text.len().saturating_sub(200)..]
    );
    assert!(
        text.len() < 280_000,
        "output should be near the 256K cap + small footer, got {} bytes",
        text.len()
    );
}

/// Offset > total_lines returns a helpful warning instead of empty output.
/// TS: `FileReadTool.ts:707`.
#[tokio::test]
async fn test_read_offset_beyond_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("short.txt");
    std::fs::write(&file, "line1\nline2\nline3\n").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "offset": 100}),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data["file"]["content"].as_str().unwrap();
    assert!(text.contains("shorter than provided offset"), "got: {text}");
    assert!(text.contains("100"));
    assert!(text.contains("3 line"));
}

// ── R7-T16: notebook structured cells tests ──
//
// TS `utils/notebook.ts:163-183` projects each cell into
// `{ cellType, source, cell_id, language?, execution_count?, outputs? }`.
// The tests below verify the projection: a basic two-cell notebook
// (markdown + code) plus output handling.

/// Helper — write a minimal Jupyter notebook fixture to a temp file.
fn write_notebook_fixture(file: &std::path::Path, cells: serde_json::Value) {
    let notebook = serde_json::json!({
        "metadata": {
            "language_info": { "name": "python" }
        },
        "nbformat": 4,
        "nbformat_minor": 5,
        "cells": cells,
    });
    std::fs::write(file, serde_json::to_string(&notebook).unwrap()).unwrap();
}

#[tokio::test]
async fn test_read_notebook_returns_structured_cells() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("simple.ipynb");
    write_notebook_fixture(
        &file,
        serde_json::json!([
            {
                "cell_type": "markdown",
                "id": "intro",
                "source": ["# Heading\n", "Some text"],
                "metadata": {},
            },
            {
                "cell_type": "code",
                "id": "compute",
                "source": "result = 1 + 1\nresult",
                "execution_count": 1,
                "metadata": {},
                "outputs": [
                    {
                        "output_type": "execute_result",
                        "execution_count": 1,
                        "data": { "text/plain": "2" },
                        "metadata": {}
                    }
                ]
            }
        ]),
    );

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    // TS-shaped envelope.
    assert_eq!(result.data["type"], "notebook");
    let cells = result.data["file"]["cells"]
        .as_array()
        .expect("cells array");
    assert_eq!(cells.len(), 2);

    // Markdown cell: cellType, source joined, cell_id from `id`,
    // NO language/execution_count/outputs (TS omits for non-code).
    assert_eq!(cells[0]["cellType"], "markdown");
    assert_eq!(cells[0]["source"], "# Heading\nSome text");
    assert_eq!(cells[0]["cell_id"], "intro");
    assert!(cells[0].get("language").is_none());
    assert!(cells[0].get("execution_count").is_none());

    // Code cell: cellType, source string-as-is, language defaulted from
    // notebook metadata, execution_count carried, outputs projected.
    assert_eq!(cells[1]["cellType"], "code");
    assert_eq!(cells[1]["source"], "result = 1 + 1\nresult");
    assert_eq!(cells[1]["cell_id"], "compute");
    assert_eq!(cells[1]["language"], "python");
    assert_eq!(cells[1]["execution_count"], 1);
    let outputs = cells[1]["outputs"].as_array().expect("outputs array");
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0]["output_type"], "execute_result");
    assert_eq!(outputs[0]["text"], "2");
}

#[tokio::test]
async fn test_read_notebook_synthesizes_cell_id_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("legacy.ipynb");
    // Old-style notebooks (nbformat < 4.5) don't have `id` on cells.
    // TS synthesizes `cell-N` (0-based index). Match that.
    write_notebook_fixture(
        &file,
        serde_json::json!([
            { "cell_type": "code", "source": "x = 1", "outputs": [], "metadata": {} },
            { "cell_type": "code", "source": "y = 2", "outputs": [], "metadata": {} },
        ]),
    );

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let cells = result.data["file"]["cells"].as_array().unwrap();
    assert_eq!(cells[0]["cell_id"], "cell-0");
    assert_eq!(cells[1]["cell_id"], "cell-1");
}

#[tokio::test]
async fn test_read_notebook_truncates_large_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("big.ipynb");
    // 12K-char output exceeds the 10K LARGE_OUTPUT_THRESHOLD.
    let big_text = "x".repeat(12_000);
    write_notebook_fixture(
        &file,
        serde_json::json!([{
            "cell_type": "code",
            "id": "big",
            "source": "print('big')",
            "execution_count": 1,
            "metadata": {},
            "outputs": [{
                "output_type": "stream",
                "text": [big_text]
            }]
        }]),
    );

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let outputs = result.data["file"]["cells"][0]["outputs"]
        .as_array()
        .unwrap();
    assert_eq!(outputs.len(), 1);
    let text = outputs[0]["text"].as_str().unwrap();
    // Should be the truncation hint, not the original 12K of x's.
    assert!(
        text.contains("too large to include") && text.contains("jq"),
        "expected truncation hint, got: {text}"
    );
}

#[tokio::test]
async fn test_read_notebook_stream_output() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("stream.ipynb");
    write_notebook_fixture(
        &file,
        serde_json::json!([{
            "cell_type": "code",
            "id": "printer",
            "source": "print('hello')",
            "execution_count": 1,
            "metadata": {},
            "outputs": [{
                "output_type": "stream",
                "name": "stdout",
                "text": ["hello\n"]
            }]
        }]),
    );

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let outputs = result.data["file"]["cells"][0]["outputs"]
        .as_array()
        .unwrap();
    assert_eq!(outputs[0]["output_type"], "stream");
    assert_eq!(outputs[0]["text"], "hello\n");
}

#[tokio::test]
async fn test_read_notebook_error_output() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("error.ipynb");
    write_notebook_fixture(
        &file,
        serde_json::json!([{
            "cell_type": "code",
            "id": "broken",
            "source": "1 / 0",
            "execution_count": 1,
            "metadata": {},
            "outputs": [{
                "output_type": "error",
                "ename": "ZeroDivisionError",
                "evalue": "division by zero",
                "traceback": ["Traceback line 1", "Traceback line 2"]
            }]
        }]),
    );

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let outputs = result.data["file"]["cells"][0]["outputs"]
        .as_array()
        .unwrap();
    assert_eq!(outputs[0]["output_type"], "error");
    let text = outputs[0]["text"].as_str().unwrap();
    assert!(text.contains("ZeroDivisionError: division by zero"));
    assert!(text.contains("Traceback line 1"));
    assert!(text.contains("Traceback line 2"));
}

#[tokio::test]
async fn test_read_notebook_image_output() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("plot.ipynb");
    write_notebook_fixture(
        &file,
        serde_json::json!([{
            "cell_type": "code",
            "id": "plot",
            "source": "plt.plot(...)",
            "execution_count": 1,
            "metadata": {},
            "outputs": [{
                "output_type": "display_data",
                "data": {
                    "text/plain": "<Figure>",
                    "image/png": "iVBORw0KGgo=  whitespace test"
                },
                "metadata": {}
            }]
        }]),
    );

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let output = &result.data["file"]["cells"][0]["outputs"][0];
    assert_eq!(output["output_type"], "display_data");
    assert_eq!(output["text"], "<Figure>");
    let image = &output["image"];
    assert_eq!(image["media_type"], "image/png");
    // Whitespace-stripped, matching TS `data['image/png'].replace(/\s/g,'')`.
    assert_eq!(image["image_data"], "iVBORw0KGgo=whitespacetest");
}

// ── R7-T9: file_unchanged dedup tests ──
//
// TS `FileReadTool.ts:523-573` returns a `{ type: 'file_unchanged' }` stub
// instead of resending the full content when:
//   1. the same path was previously read via the Read tool (not Edit/Write)
//   2. the same input offset/limit are requested
//   3. the disk mtime hasn't changed
// The tests below exercise each gate.

/// Two consecutive default Read calls on the same file: the second should
/// return a stub. This is the core dedup path — TS comment cites BQ
/// telemetry showing ~18% of Read calls are repeats of this shape.
#[tokio::test]
async fn test_read_dedup_same_call_twice() {
    use coco_context::FileReadState;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dedup.txt");
    std::fs::write(&file, "alpha\nbeta\ngamma\n").unwrap();

    let mut ctx = ToolUseContext::test_default();
    ctx.file_read_state = Some(Arc::new(RwLock::new(FileReadState::new())));

    // First call — full content returned in the TS-shaped text envelope.
    let first = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();
    assert_eq!(first.data["type"], "text");
    let first_text = first.data["file"]["content"]
        .as_str()
        .expect("first call returns text");
    assert!(first_text.contains("alpha"));

    // Second call — should hit the dedup stub.
    let second = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();
    assert_eq!(
        second.data["type"], "file_unchanged",
        "second identical Read should return file_unchanged stub, got: {:?}",
        second.data
    );
    assert_eq!(second.data["file"]["filePath"], file.to_str().unwrap());
}

/// Repeat Read with same explicit offset/limit args should still dedup.
#[tokio::test]
async fn test_read_dedup_same_explicit_range() {
    use coco_context::FileReadState;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dedup_range.txt");
    let content: String = (1..=50).map(|i| format!("line {i}\n")).collect();
    std::fs::write(&file, content).unwrap();

    let mut ctx = ToolUseContext::test_default();
    ctx.file_read_state = Some(Arc::new(RwLock::new(FileReadState::new())));

    let args = json!({
        "file_path": file.to_str().unwrap(),
        "offset": 10,
        "limit": 5
    });
    let _first = ReadTool.execute(args.clone(), &ctx).await.unwrap();
    let second = ReadTool.execute(args, &ctx).await.unwrap();
    assert_eq!(
        second.data["type"], "file_unchanged",
        "second Read with identical offset/limit should dedup, got: {:?}",
        second.data
    );
}

/// Different offset/limit should NOT dedup — the cached entry doesn't
/// cover the new range.
#[tokio::test]
async fn test_read_dedup_skipped_for_different_range() {
    use coco_context::FileReadState;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("no_dedup.txt");
    let content: String = (1..=50).map(|i| format!("line {i}\n")).collect();
    std::fs::write(&file, content).unwrap();

    let mut ctx = ToolUseContext::test_default();
    ctx.file_read_state = Some(Arc::new(RwLock::new(FileReadState::new())));

    let _first = ReadTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "offset": 10, "limit": 5}),
            &ctx,
        )
        .await
        .unwrap();
    let second = ReadTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "offset": 20, "limit": 5}),
            &ctx,
        )
        .await
        .unwrap();
    // Should be the TS-shaped text envelope, not a file_unchanged stub.
    assert_eq!(
        second.data["type"], "text",
        "expected text envelope, got: {:?}",
        second.data
    );
}

/// Mtime change invalidates the dedup — file modified externally between
/// reads should return fresh content. We mutate the cached `mtime_ms`
/// directly to simulate "disk mtime moved forward" without relying on
/// filesystem mtime precision (which can be 1s on ext4/HFS+).
#[tokio::test]
async fn test_read_dedup_invalidated_by_mtime_change() {
    use coco_context::FileReadState;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mtime.txt");
    std::fs::write(&file, "version one\n").unwrap();

    let mut ctx = ToolUseContext::test_default();
    ctx.file_read_state = Some(Arc::new(RwLock::new(FileReadState::new())));

    // Prime the cache.
    let _first = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    // Forge a stale cached mtime (1 ms behind the disk mtime) so the dedup
    // gate fails on the second call. This is more robust than relying on
    // filesystem mtime precision in tests.
    {
        let abs = std::fs::canonicalize(&file).unwrap();
        let frs = ctx.file_read_state.as_ref().unwrap();
        let mut frs_w = frs.write().await;
        // Pull the existing entry, decrement mtime, reinsert via
        // `set_from_read` to preserve the from-read marker + input range.
        if let Some(stale) = frs_w.peek(&abs).cloned() {
            let stale = coco_context::FileReadEntry {
                mtime_ms: stale.mtime_ms - 1,
                ..stale
            };
            frs_w.set_from_read(abs, stale, None, None);
        }
    }

    let second = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();
    // Cache mtime is stale → dedup gate fails → fresh read returned.
    assert_eq!(second.data["type"], "text");
    let text = second.data["file"]["content"]
        .as_str()
        .expect("expected text after mtime mismatch");
    assert!(text.contains("version one"), "got: {text}");
}

/// Edit then Read should NOT dedup against the post-edit entry, because
/// the post-edit content was never returned to the model as a Read result.
/// TS gates this via `existingState.offset !== undefined`; coco-rs gates
/// via `is_from_read_tool`.
#[tokio::test]
async fn test_read_dedup_skipped_after_edit() {
    use coco_context::FileReadEntry;
    use coco_context::FileReadState;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("post_edit.txt");
    std::fs::write(&file, "post-edit content\n").unwrap();

    let mut ctx = ToolUseContext::test_default();
    ctx.file_read_state = Some(Arc::new(RwLock::new(FileReadState::new())));

    // Simulate a prior Edit by inserting an entry via `set` (not
    // `set_from_read`). That mirrors what `update_after_edit` would do
    // post-edit, but more directly.
    let abs = std::fs::canonicalize(&file).unwrap();
    let mtime = coco_context::file_mtime_ms(&abs).await.unwrap();
    {
        let frs = ctx.file_read_state.as_ref().unwrap();
        let mut frs_w = frs.write().await;
        frs_w.set(
            abs,
            FileReadEntry {
                content: "post-edit content\n".into(),
                mtime_ms: mtime,
                offset: None,
                limit: None,
            },
        );
    }

    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();
    // Should NOT be a stub — the entry came from `set`, not
    // `set_from_read`, so dedup is skipped. The result is the TS-shaped
    // text envelope.
    assert_eq!(result.data["type"], "text");
    let text = result.data["file"]["content"]
        .as_str()
        .expect("expected text, got stub");
    assert!(text.contains("post-edit content"), "got: {text}");
}
