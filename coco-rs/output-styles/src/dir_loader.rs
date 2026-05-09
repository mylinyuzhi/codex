//! Markdown directory loader for output styles.
//!
//! TS source: `outputStyles/loadOutputStylesDir.ts`. Reads `.md` files
//! from one or more directories, parses YAML frontmatter, and emits
//! one [`OutputStyleConfig`] per file. The filename (stem) is the
//! default style name; `name`, `description`, and
//! `keep-coding-instructions` frontmatter fields override or supplement
//! the defaults.
//!
//! `force-for-plugin` is only meaningful on plugin styles; finding it
//! here logs a warning and ignores the value (TS parity with the
//! `logForDebugging` warn at `loadOutputStylesDir.ts:65-69`).
//!
//! TS uses a multi-source markdown loader (managed/user/project, with
//! priority + dedup). coco-rs threads the (dir, source) pair in from
//! the caller — paths are resolved at the CLI bootstrap layer where
//! `~/.coco/`, the project tree, and managed/policy locations are
//! known.

use std::path::Path;

use coco_frontmatter::Frontmatter;
use coco_frontmatter::FrontmatterValue;

use crate::catalog::OutputStyleConfig;
use crate::catalog::OutputStyleSource;
use crate::error::OutputStylesError;

/// Load every `.md` output-style file directly under `dir`.
///
/// Returns an empty `Vec` if `dir` doesn't exist or isn't readable —
/// matches TS `loadMarkdownFilesForSubdir` "fail open" semantics so a
/// missing config dir never breaks bootstrap. Per-file parse errors are
/// logged at `warn` level and the offending file is skipped.
///
/// `source` is attached to each loaded style and drives priority during
/// aggregation. Use [`OutputStyleSource::UserSettings`] for `~/.claude`,
/// [`OutputStyleSource::ProjectSettings`] for `<cwd>/.claude/...`, and
/// [`OutputStyleSource::PolicySettings`] for the managed location.
pub fn load_dir_styles(dir: &Path, source: OutputStyleSource) -> Vec<OutputStyleConfig> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        match load_single(&path, source) {
            Ok(style) => out.push(style),
            Err(e) => tracing::warn!(
                target: "coco_output_styles::dir_loader",
                path = %path.display(),
                error = %e,
                "skipping malformed output-style file"
            ),
        }
    }
    out
}

/// Load a single output-style markdown file. Public for tests.
pub fn load_single(
    path: &Path,
    source: OutputStyleSource,
) -> Result<OutputStyleConfig, OutputStylesError> {
    let raw = std::fs::read_to_string(path).map_err(|source| OutputStylesError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed = coco_frontmatter::parse(&raw);
    Ok(build_config_from_parsed(path, &parsed, source))
}

/// Build an [`OutputStyleConfig`] from already-parsed frontmatter +
/// content. Public so the plugin loader can reuse the same field
/// extraction without re-reading the file.
pub fn build_config_from_parsed(
    path: &Path,
    parsed: &Frontmatter,
    source: OutputStyleSource,
) -> OutputStyleConfig {
    let stem = filename_stem(path);

    // `name` fallback: filename stem (matches TS `loadOutputStylesDir.ts:42`).
    let name = parsed
        .data
        .get("name")
        .and_then(FrontmatterValue::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| stem.clone());

    let description = parsed
        .data
        .get("description")
        .and_then(FrontmatterValue::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| description_from_markdown(&parsed.content, &stem));

    // `keep-coding-instructions` accepts both bool and stringly-typed
    // bool — TS at `loadOutputStylesDir.ts:53-62` does the same dance.
    let keep_coding_instructions = parsed
        .data
        .get("keep-coding-instructions")
        .and_then(FrontmatterValue::as_bool);

    // `force-for-plugin` warning when present on non-plugin style —
    // matches TS `loadOutputStylesDir.ts:65-69`. Plugin loader handles
    // the real read.
    if !matches!(source, OutputStyleSource::Plugin) && parsed.data.contains_key("force-for-plugin")
    {
        tracing::warn!(
            target: "coco_output_styles::dir_loader",
            path = %path.display(),
            style = %name,
            "`force-for-plugin` only applies to plugin output styles; ignoring"
        );
    }

    OutputStyleConfig {
        name,
        description,
        prompt: parsed.content.trim().to_string(),
        source,
        keep_coding_instructions,
        force_for_plugin: None,
    }
}

fn filename_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// First non-empty line of the markdown content, with leading `#`s
/// stripped, capped at 100 chars. TS:
/// `markdownConfigLoader.ts::extractDescriptionFromMarkdown`.
fn description_from_markdown(content: &str, stem: &str) -> String {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let text = if let Some(stripped) = trimmed.strip_prefix('#') {
            // Strip any number of leading '#' plus the space after.
            let mut s = stripped;
            while let Some(rest) = s.strip_prefix('#') {
                s = rest;
            }
            s.trim_start().to_string()
        } else {
            trimmed.to_string()
        };
        return if text.chars().count() > 100 {
            let mut truncated: String = text.chars().take(97).collect();
            truncated.push_str("...");
            truncated
        } else {
            text
        };
    }
    format!("Custom {stem} output style")
}

#[cfg(test)]
#[path = "dir_loader.test.rs"]
mod tests;
