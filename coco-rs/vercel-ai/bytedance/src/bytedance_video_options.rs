//! ByteDance video provider options.

use serde::Deserialize;

/// Provider-specific options for ByteDance video generation.
///
/// Deserialized from the `"bytedance"` namespace in `provider_options`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ByteDanceVideoProviderOptions {
    /// Whether to add a watermark to the generated video.
    pub watermark: Option<bool>,
    /// Whether to generate audio along with the video.
    pub generate_audio: Option<bool>,
    /// Whether the camera should remain fixed during generation.
    pub camera_fixed: Option<bool>,
    /// Whether to return the last frame of the video.
    pub return_last_frame: Option<bool>,
    /// Service tier: `"default"` or `"flex"`.
    pub service_tier: Option<String>,
    /// Whether to use draft mode (faster, lower quality).
    pub draft: Option<bool>,
    /// Base64-encoded last frame image for continuation.
    pub last_frame_image: Option<String>,
    /// Base64-encoded reference images for style guidance.
    pub reference_images: Option<Vec<String>>,
    /// Override poll interval in milliseconds.
    pub poll_interval_ms: Option<u64>,
    /// Override poll timeout in milliseconds.
    pub poll_timeout_ms: Option<u64>,
}

/// Keys handled by the provider options struct (not passed through to the API).
pub const HANDLED_PROVIDER_OPTIONS: &[&str] = &[
    "watermark",
    "generate_audio",
    "camera_fixed",
    "return_last_frame",
    "service_tier",
    "draft",
    "last_frame_image",
    "reference_images",
    "poll_interval_ms",
    "poll_timeout_ms",
];
