//! External editor integration.
//!
//! Opens the user's $EDITOR or $VISUAL with the current input content,
//! reads back the edited content when the editor exits.

use std::io;
use std::process::Command;

/// Result of an external editor session.
pub struct EditorResult {
    /// The edited content.
    pub content: String,
    /// Whether the content was modified.
    pub modified: bool,
}

/// Open the given text in an external editor.
///
/// Uses $VISUAL → $EDITOR → vi fallback. Writes content to a temp file,
/// launches the editor, reads back the result.
pub fn edit_in_external_editor(initial_content: &str) -> io::Result<EditorResult> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    // Write initial content to temp file
    let temp_dir = std::env::temp_dir();
    let temp_file = temp_dir.join(format!("coco-edit-{}.txt", std::process::id()));
    std::fs::write(&temp_file, initial_content)?;

    // Launch editor
    let status = Command::new(&editor).arg(&temp_file).status()?;

    if !status.success() {
        let _ = std::fs::remove_file(&temp_file);
        return Err(io::Error::other(format!(
            "Editor '{editor}' exited with status {status}"
        )));
    }

    // Read back content
    let content = std::fs::read_to_string(&temp_file)?;
    let _ = std::fs::remove_file(&temp_file);

    let modified = content != initial_content;

    Ok(EditorResult { content, modified })
}
