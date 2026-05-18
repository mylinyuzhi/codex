//! Pure view model for unified diff rendering.
//!
//! The widget layer owns ratatui spans and theme colors. This module owns
//! parsing, row classification, and old/new line-number progression so chat
//! cells and full-screen diff modals share one source-backed model.

/// Parsed line numbers from a unified diff hunk header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HunkHeader {
    pub(crate) old_start: i32,
    pub(crate) new_start: i32,
    pub(crate) label: String,
}

/// A single source-backed diff row before styling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiffLineView {
    FileHeader {
        marker: &'static str,
        path: String,
    },
    Hunk {
        old_start: i32,
        new_start: i32,
        label: String,
    },
    RawHunk {
        text: String,
    },
    Context {
        old_line: i32,
        new_line: i32,
        content: String,
    },
    Removed {
        old_line: i32,
        content: String,
        compare_to: Option<String>,
    },
    Added {
        new_line: i32,
        content: String,
        compare_to: Option<String>,
    },
}

enum DiffChunk<'a> {
    Paired { old: &'a str, new: &'a str },
    Removed(&'a str),
    Added(&'a str),
    Context(&'a str),
    Hunk(&'a str),
    FileHeader(&'a str),
}

/// Parse a unified diff `@@` line into old/new start positions.
pub(crate) fn parse_hunk_header(line: &str) -> Option<HunkHeader> {
    // Format: @@ -old_start[,old_count] +new_start[,new_count] @@ [label]
    let stripped = line.strip_prefix("@@ ")?;
    let end_idx = stripped.find(" @@")?;
    let range_part = &stripped[..end_idx];
    let label = stripped.get(end_idx + 3..).unwrap_or("").trim().to_string();

    let mut parts = range_part.split_whitespace();

    let old_range = parts.next()?.strip_prefix('-')?;
    let old_start: i32 = old_range
        .split(',')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let new_range = parts.next()?.strip_prefix('+')?;
    let new_start: i32 = new_range
        .split(',')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    Some(HunkHeader {
        old_start,
        new_start,
        label,
    })
}

pub(crate) fn diff_line_views(diff_text: &str) -> Vec<DiffLineView> {
    let raw_lines: Vec<&str> = diff_text.lines().collect();
    let chunks = classify_diff_lines(&raw_lines);

    let mut old_line: i32 = 1;
    let mut new_line: i32 = 1;
    let mut rows = Vec::new();

    for chunk in chunks {
        match chunk {
            DiffChunk::FileHeader(text) => {
                let (marker, path) = if let Some(rest) = text.strip_prefix("--- ") {
                    ("─", rest)
                } else if let Some(rest) = text.strip_prefix("+++ ") {
                    ("+", rest)
                } else {
                    ("", text)
                };
                rows.push(DiffLineView::FileHeader {
                    marker,
                    path: path.to_string(),
                });
            }
            DiffChunk::Hunk(text) => {
                if let Some(hdr) = parse_hunk_header(text) {
                    old_line = hdr.old_start;
                    new_line = hdr.new_start;
                    rows.push(DiffLineView::Hunk {
                        old_start: hdr.old_start,
                        new_start: hdr.new_start,
                        label: hdr.label,
                    });
                } else {
                    rows.push(DiffLineView::RawHunk {
                        text: text.to_string(),
                    });
                }
            }
            DiffChunk::Context(text) => {
                rows.push(DiffLineView::Context {
                    old_line,
                    new_line,
                    content: text.strip_prefix(' ').unwrap_or(text).to_string(),
                });
                old_line += 1;
                new_line += 1;
            }
            DiffChunk::Removed(text) => {
                rows.push(DiffLineView::Removed {
                    old_line,
                    content: text.strip_prefix('-').unwrap_or(text).to_string(),
                    compare_to: None,
                });
                old_line += 1;
            }
            DiffChunk::Added(text) => {
                rows.push(DiffLineView::Added {
                    new_line,
                    content: text.strip_prefix('+').unwrap_or(text).to_string(),
                    compare_to: None,
                });
                new_line += 1;
            }
            DiffChunk::Paired { old, new } => {
                let old_content = old.strip_prefix('-').unwrap_or(old);
                let new_content = new.strip_prefix('+').unwrap_or(new);
                rows.push(DiffLineView::Removed {
                    old_line,
                    content: old_content.to_string(),
                    compare_to: Some(new_content.to_string()),
                });
                old_line += 1;
                rows.push(DiffLineView::Added {
                    new_line,
                    content: new_content.to_string(),
                    compare_to: Some(old_content.to_string()),
                });
                new_line += 1;
            }
        }
    }

    rows
}

fn classify_diff_lines<'a>(lines: &'a [&'a str]) -> Vec<DiffChunk<'a>> {
    let mut chunks = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        if line.starts_with("--- ") || line.starts_with("+++ ") {
            chunks.push(DiffChunk::FileHeader(line));
            i += 1;
        } else if line.starts_with("@@") {
            chunks.push(DiffChunk::Hunk(line));
            i += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            let rm_start = i;
            while i < lines.len() && lines[i].starts_with('-') && !lines[i].starts_with("---") {
                i += 1;
            }
            let add_start = i;
            while i < lines.len() && lines[i].starts_with('+') && !lines[i].starts_with("+++") {
                i += 1;
            }
            let removed = &lines[rm_start..add_start];
            let added = &lines[add_start..i];

            let pairs = removed.len().min(added.len());
            for j in 0..pairs {
                chunks.push(DiffChunk::Paired {
                    old: removed[j],
                    new: added[j],
                });
            }
            for line in &removed[pairs..] {
                chunks.push(DiffChunk::Removed(line));
            }
            for line in &added[pairs..] {
                chunks.push(DiffChunk::Added(line));
            }
        } else if line.starts_with('+') && !line.starts_with("+++") {
            chunks.push(DiffChunk::Added(line));
            i += 1;
        } else {
            chunks.push(DiffChunk::Context(line));
            i += 1;
        }
    }

    chunks
}

#[cfg(test)]
#[path = "diff.test.rs"]
mod tests;
