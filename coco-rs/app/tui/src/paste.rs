//! Paste handling — bracketed paste mode and inline paste pills.
//!
//! When the user pastes content, it's wrapped as a paste pill `[Pasted text #N]`
//! and stored in a cache. The pill is displayed inline in the input while the
//! full content is attached when the message is submitted.

/// Check if text matches a paste pill pattern.
///
/// Valid patterns: `[Pasted text #N]`, `[Image #N]`
pub fn is_paste_pill(text: &str) -> bool {
    if !text.starts_with('[') || !text.ends_with(']') {
        return false;
    }
    let inner = &text[1..text.len() - 1];
    inner.starts_with("Pasted text #") || inner.starts_with("Image #")
}

/// Image data extracted from a paste pill.
#[derive(Debug, Clone)]
pub struct ImageData {
    pub bytes: Vec<u8>,
    pub mime: String,
}

/// Resolved input after expanding paste pills.
#[derive(Debug, Clone)]
pub struct ResolvedInput {
    /// Text with paste pills expanded (text pills → content, image pills → removed).
    pub text: String,
    /// Image data extracted from image pills.
    pub images: Vec<ImageData>,
}

/// Paste entry stored in the cache.
#[derive(Debug, Clone)]
pub struct PasteEntry {
    /// The paste pill label (e.g., "[Pasted text #1]").
    pub pill: String,
    /// The actual pasted content (text content or file path for images).
    pub content: String,
    /// Whether this is an image paste.
    pub is_image: bool,
    /// Raw image bytes (only present when `is_image` is true).
    pub image_bytes: Option<Vec<u8>>,
    /// MIME type for image entries.
    pub image_mime: Option<String>,
}

/// Paste manager that tracks paste pills.
#[derive(Debug, Default)]
pub struct PasteManager {
    entries: Vec<PasteEntry>,
}

impl PasteManager {
    /// Create a new paste manager.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add a text paste. Returns the pill label.
    pub fn add_text(&mut self, content: String) -> String {
        let n = self.entries.len() + 1;
        let pill = format!("[Pasted text #{n}]");
        self.entries.push(PasteEntry {
            pill: pill.clone(),
            content,
            is_image: false,
            image_bytes: None,
            image_mime: None,
        });
        pill
    }

    /// Add an image paste by file path. Returns the pill label.
    pub fn add_image(&mut self, path: String) -> String {
        let n = self.entries.len() + 1;
        let pill = format!("[Image #{n}]");
        self.entries.push(PasteEntry {
            pill: pill.clone(),
            content: path,
            is_image: true,
            image_bytes: None,
            image_mime: None,
        });
        pill
    }

    /// Add an image paste with raw bytes. Returns the pill label.
    pub fn add_image_data(&mut self, bytes: Vec<u8>, mime: String) -> String {
        let n = self.entries.len() + 1;
        let pill = format!("[Image #{n}]");
        self.entries.push(PasteEntry {
            pill: pill.clone(),
            content: String::new(),
            is_image: true,
            image_bytes: Some(bytes),
            image_mime: Some(mime),
        });
        pill
    }

    pub fn entries(&self) -> &[PasteEntry] {
        &self.entries
    }

    /// Resolve paste pills in the input, returning the expanded content.
    ///
    /// Simple string replacement — text pills become content, image pills become
    /// their file path (or empty string if bytes-only).
    pub fn resolve(&self, input: &str) -> String {
        let mut result = input.to_string();
        for entry in &self.entries {
            result = result.replace(&entry.pill, &entry.content);
        }
        result
    }

    /// Resolve paste pills, separating text expansions from image data.
    ///
    /// Text pills are expanded inline. Image pills are removed from text and
    /// their data is returned separately for API content-block assembly.
    pub fn resolve_structured(&self, input: &str) -> ResolvedInput {
        let mut text = input.to_string();
        let mut images = Vec::new();

        for entry in &self.entries {
            if entry.is_image {
                // Remove image pill and any single adjacent space.
                // Avoids split_whitespace() which destroys code indentation/formatting.
                text = text.replace(&format!("{} ", &entry.pill), "");
                text = text.replace(&format!(" {}", &entry.pill), "");
                text = text.replace(&entry.pill, "");
                // Collect image data if available
                if let (Some(bytes), Some(mime)) = (&entry.image_bytes, &entry.image_mime) {
                    images.push(ImageData {
                        bytes: bytes.clone(),
                        mime: mime.clone(),
                    });
                }
            } else {
                // Expand text pill inline
                text = text.replace(&entry.pill, &entry.content);
            }
        }

        let text = text.trim().to_string();

        ResolvedInput { text, images }
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
#[path = "paste.test.rs"]
mod tests;
