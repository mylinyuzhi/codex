//! Mention → Attachment resolution with FileReadState deduplication.
//!
//! TS: `processAtMentionedFiles()` in attachments.ts (line 1894) — resolves
//! @-mentioned files into Attachment objects, checking `readFileState` for
//! dedup (returns `AlreadyReadFileAttachment` if unchanged).

use std::path::Path;
use std::path::PathBuf;

use crate::attachment::AlreadyReadFileAttachment;
use crate::attachment::Attachment;
use crate::attachment::DirectoryAttachment;
use crate::attachment::FileReadOptions;
use crate::attachment::generate_file_attachment;
use crate::file_read_state::FileReadEntry;
use crate::file_read_state::FileReadState;
use crate::file_read_state::file_mtime_ms;
use crate::user_input::Mention;
use crate::user_input::MentionType;

/// Options for mention resolution.
pub struct MentionResolveOptions<'a> {
    /// Current working directory for relative path expansion.
    pub cwd: &'a Path,
    /// Maximum directory entries to list.
    pub max_dir_entries: i32,
}

impl Default for MentionResolveOptions<'_> {
    fn default() -> Self {
        Self {
            cwd: Path::new("."),
            max_dir_entries: 1000,
        }
    }
}

/// Resolve a list of mentions into attachments.
///
/// Checks `file_read_state` for dedup: if a file is cached and its mtime
/// hasn't changed, returns `AlreadyReadFile` instead of re-reading.
/// After reading a new file, updates `file_read_state` with its content and mtime.
pub async fn resolve_mentions(
    mentions: &[Mention],
    file_read_state: &mut FileReadState,
    options: &MentionResolveOptions<'_>,
) -> Vec<Attachment> {
    let mut attachments = Vec::new();

    for mention in mentions {
        match mention.mention_type {
            MentionType::FilePath => {
                if let Some(att) = resolve_file_mention(mention, file_read_state, options).await {
                    attachments.push(att);
                }
            }
            MentionType::Agent => {
                attachments.push(Attachment::AgentMention(
                    crate::attachment::AgentMentionAttachment {
                        agent_type: mention.text.clone(),
                    },
                ));
            }
            MentionType::Url | MentionType::Symbol => {
                // URL and symbol mentions not resolved to attachments yet.
            }
        }
    }

    attachments
}

/// Resolve a single file mention to an attachment.
async fn resolve_file_mention(
    mention: &Mention,
    file_read_state: &mut FileReadState,
    options: &MentionResolveOptions<'_>,
) -> Option<Attachment> {
    let raw_path = Path::new(&mention.text);
    let abs_path = if raw_path.is_absolute() {
        raw_path.to_path_buf()
    } else {
        options.cwd.join(raw_path)
    };

    let display_path = abs_path
        .strip_prefix(options.cwd)
        .unwrap_or(&abs_path)
        .to_string_lossy()
        .into_owned();

    // Check if path exists
    if !abs_path.exists() {
        return None;
    }

    // Directory handling
    if abs_path.is_dir() {
        return resolve_directory(&abs_path, &display_path, options.max_dir_entries);
    }

    // Dedup check: if file is in FileReadState and mtime hasn't changed,
    // return AlreadyReadFileAttachment.
    // TS: generateFileAttachment() line 3077-3115
    if let Some(entry) = file_read_state.peek(&abs_path) {
        if let Ok(disk_mtime) = file_mtime_ms(&abs_path).await {
            if entry.mtime_ms == disk_mtime {
                return Some(Attachment::AlreadyReadFile(AlreadyReadFileAttachment {
                    filename: abs_path.to_string_lossy().into_owned(),
                    display_path,
                }));
            }
        }
    }

    // Read the file via the existing attachment generator.
    let read_options = FileReadOptions {
        offset: mention.line_start,
        limit: mention.line_end.map(|end| {
            // Convert line range to limit: #L10-20 → offset=10, limit=11
            end - mention.line_start.unwrap_or(1) + 1
        }),
        ..Default::default()
    };

    let attachment = generate_file_attachment(&abs_path, options.cwd, &read_options)?;

    // Update FileReadState with new content and mtime.
    update_file_read_state(file_read_state, &abs_path, &attachment, &read_options).await;

    Some(attachment)
}

/// Update FileReadState after resolving a mention.
async fn update_file_read_state(
    state: &mut FileReadState,
    abs_path: &PathBuf,
    attachment: &Attachment,
    options: &FileReadOptions,
) {
    let content = match attachment {
        Attachment::File(f) => f.content.clone(),
        // Images and PDFs don't populate text content in FileReadState.
        _ => return,
    };

    if let Ok(mtime) = file_mtime_ms(abs_path).await {
        state.set(
            abs_path.clone(),
            FileReadEntry {
                content,
                mtime_ms: mtime,
                offset: options.offset,
                limit: options.limit,
            },
        );
    }
}

/// Resolve a directory mention: list entries up to max_entries.
fn resolve_directory(path: &Path, display_path: &str, max_entries: i32) -> Option<Attachment> {
    let entries = std::fs::read_dir(path).ok()?;
    let mut lines = Vec::new();
    let mut count = 0;

    for entry in entries {
        if count >= max_entries {
            lines.push(format!("... ({count}+ entries, truncated)"));
            break;
        }
        if let Ok(e) = entry {
            let name = e.file_name().to_string_lossy().into_owned();
            let suffix = if e.path().is_dir() { "/" } else { "" };
            lines.push(format!("{name}{suffix}"));
            count += 1;
        }
    }

    Some(Attachment::Directory(DirectoryAttachment {
        path: path.to_string_lossy().into_owned(),
        content: lines.join("\n"),
        display_path: display_path.to_string(),
    }))
}

#[cfg(test)]
#[path = "mention_resolver.test.rs"]
mod tests;
