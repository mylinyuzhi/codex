use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::LazyLock;

use crate::error::ImageProcessingError;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use cocode_utils_cache::BlockingLruCache;
use cocode_utils_cache::sha1_digest;
use image::ColorType;
use image::DynamicImage;
use image::GenericImageView;
use image::ImageEncoder;
use image::ImageFormat;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
/// Maximum width used when resizing images before uploading.
pub const MAX_WIDTH: u32 = 2048;
/// Maximum height used when resizing images before uploading.
pub const MAX_HEIGHT: u32 = 768;

pub mod error;

#[derive(Debug, Clone)]
pub struct EncodedImage {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub width: u32,
    pub height: u32,
}

impl EncodedImage {
    pub fn into_data_url(self) -> String {
        let encoded = BASE64_STANDARD.encode(&self.bytes);
        format!("data:{};base64,{}", self.mime, encoded)
    }
}

static IMAGE_CACHE: LazyLock<BlockingLruCache<[u8; 20], EncodedImage>> =
    LazyLock::new(|| BlockingLruCache::new(NonZeroUsize::new(32).unwrap_or(NonZeroUsize::MIN)));

pub fn load_and_resize_to_fit(path: &Path) -> Result<EncodedImage, ImageProcessingError> {
    let path_buf = path.to_path_buf();

    let file_bytes = read_file_bytes(path, &path_buf)?;

    let key = sha1_digest(&file_bytes);

    IMAGE_CACHE.get_or_try_insert_with(key, move || {
        let format = match image::guess_format(&file_bytes) {
            Ok(ImageFormat::Png) => Some(ImageFormat::Png),
            Ok(ImageFormat::Jpeg) => Some(ImageFormat::Jpeg),
            _ => None,
        };

        let dynamic = image::load_from_memory(&file_bytes).map_err(|source| {
            ImageProcessingError::Decode {
                path: path_buf.clone(),
                source,
            }
        })?;

        let (width, height) = dynamic.dimensions();

        let encoded = if width <= MAX_WIDTH && height <= MAX_HEIGHT {
            if let Some(format) = format {
                let mime = format_to_mime(format);
                EncodedImage {
                    bytes: file_bytes,
                    mime,
                    width,
                    height,
                }
            } else {
                let (bytes, output_format) = encode_image(&dynamic, ImageFormat::Png)?;
                let mime = format_to_mime(output_format);
                EncodedImage {
                    bytes,
                    mime,
                    width,
                    height,
                }
            }
        } else {
            let resized = dynamic.resize(MAX_WIDTH, MAX_HEIGHT, FilterType::Triangle);
            let target_format = format.unwrap_or(ImageFormat::Png);
            let (bytes, output_format) = encode_image(&resized, target_format)?;
            let mime = format_to_mime(output_format);
            EncodedImage {
                bytes,
                mime,
                width: resized.width(),
                height: resized.height(),
            }
        };

        Ok(encoded)
    })
}

fn read_file_bytes(path: &Path, path_for_error: &Path) -> Result<Vec<u8>, ImageProcessingError> {
    match tokio::runtime::Handle::try_current() {
        // If we're inside a Tokio runtime, avoid block_on (it panics on worker threads).
        // Use block_in_place and do a standard blocking read safely.
        Ok(_) => tokio::task::block_in_place(|| std::fs::read(path)).map_err(|source| {
            ImageProcessingError::Read {
                path: path_for_error.to_path_buf(),
                source,
            }
        }),
        // Outside a runtime, just read synchronously.
        Err(_) => std::fs::read(path).map_err(|source| ImageProcessingError::Read {
            path: path_for_error.to_path_buf(),
            source,
        }),
    }
}

fn encode_image(
    image: &DynamicImage,
    preferred_format: ImageFormat,
) -> Result<(Vec<u8>, ImageFormat), ImageProcessingError> {
    let target_format = match preferred_format {
        ImageFormat::Jpeg => ImageFormat::Jpeg,
        _ => ImageFormat::Png,
    };

    let mut buffer = Vec::new();

    match target_format {
        ImageFormat::Png => {
            let rgba = image.to_rgba8();
            let encoder = PngEncoder::new(&mut buffer);
            encoder
                .write_image(
                    rgba.as_raw(),
                    image.width(),
                    image.height(),
                    ColorType::Rgba8.into(),
                )
                .map_err(|source| ImageProcessingError::Encode {
                    format: target_format,
                    source,
                })?;
        }
        ImageFormat::Jpeg => {
            let mut encoder = JpegEncoder::new_with_quality(&mut buffer, 85);
            encoder
                .encode_image(image)
                .map_err(|source| ImageProcessingError::Encode {
                    format: target_format,
                    source,
                })?;
        }
        _ => unreachable!("unsupported target_format should have been handled earlier"),
    }

    Ok((buffer, target_format))
}

fn format_to_mime(format: ImageFormat) -> String {
    match format {
        ImageFormat::Jpeg => "image/jpeg".to_string(),
        _ => "image/png".to_string(),
    }
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
