//! `/context` — show context window usage with token estimation.
//!
//! Reads the current session, estimates token usage per category
//! (system prompt, tools, memory, messages, free), and renders a table.

use std::pin::Pin;

/// Default context window size (tokens).
const DEFAULT_CONTEXT_WINDOW: i64 = 200_000;

/// Estimated fixed overhead for system prompt + tool definitions.
const SYSTEM_PROMPT_TOKENS: i64 = 2_500;
const TOOL_DEFINITIONS_TOKENS: i64 = 8_000;
const RESERVED_OUTPUT_TOKENS: i64 = 16_000;

/// Async handler for `/context`.
///
/// Estimates the context window usage by reading the session file and
/// computing approximate token counts for each budget category.
pub fn handler(
    _args: String,
) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let sessions_dir = dirs::home_dir()
            .map(|h| h.join(".cocode").join("sessions"))
            .unwrap_or_default();

        let (message_tokens, memory_tokens, message_count) = estimate_usage(&sessions_dir).await;

        let context_window = DEFAULT_CONTEXT_WINDOW;
        let used = SYSTEM_PROMPT_TOKENS + TOOL_DEFINITIONS_TOKENS + memory_tokens + message_tokens;
        let free = (context_window - used - RESERVED_OUTPUT_TOKENS).max(0);
        let used_pct = (used as f64 / context_window as f64 * 100.0) as i64;

        let mut out = String::from("## Context Window Usage\n\n");

        // Table header
        out.push_str("| Category            | Tokens    | Pct    |\n");
        out.push_str("|---------------------|-----------|--------|\n");

        // Rows
        let rows = [
            ("System prompt", SYSTEM_PROMPT_TOKENS),
            ("Tool definitions", TOOL_DEFINITIONS_TOKENS),
            ("Memory files", memory_tokens),
            ("Messages", message_tokens),
            ("Reserved (output)", RESERVED_OUTPUT_TOKENS),
            ("Free", free),
        ];

        for (label, tokens) in &rows {
            let pct = *tokens as f64 / context_window as f64 * 100.0;
            out.push_str(&format!(
                "| {:<19} | {:>9} | {:>5.1}% |\n",
                label,
                format_tokens(*tokens),
                pct,
            ));
        }

        out.push_str(&format!(
            "\n**Total used:** {} / {} ({used_pct}%)\n",
            format_tokens(used),
            format_tokens(context_window),
        ));
        out.push_str(&format!("**Messages in history:** {message_count}\n"));

        // Advice
        if used_pct > 80 {
            out.push_str("\nContext is getting full. Consider running /compact to free space.");
        } else if used_pct > 50 {
            out.push_str("\nContext usage is moderate. Compaction available via /compact.");
        } else {
            out.push_str("\nPlenty of context space available.");
        }

        Ok(out)
    })
}

/// Read the newest session and estimate message + memory tokens.
async fn estimate_usage(sessions_dir: &std::path::Path) -> (i64, i64, i64) {
    if !sessions_dir.exists() {
        return (0, 0, 0);
    }

    let Ok(mut entries) = tokio::fs::read_dir(sessions_dir).await else {
        return (0, 0, 0);
    };

    let mut newest: Option<(std::path::PathBuf, u64)> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Ok(meta) = entry.metadata().await {
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            if newest.as_ref().is_none_or(|n| modified > n.1) {
                newest = Some((path, modified));
            }
        }
    }

    let Some((path, _)) = newest else {
        return (0, 0, 0);
    };

    let Ok(content) = tokio::fs::read_to_string(&path).await else {
        return (0, 0, 0);
    };

    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
        return (0, 0, 0);
    };

    // Estimate message tokens
    let messages = parsed
        .get("messages")
        .or_else(|| parsed.get("message_history"))
        .and_then(|v| v.as_array());

    let message_count = messages.map_or(0, |m| m.len() as i64);

    let message_tokens = messages.map_or(0, |msgs| {
        msgs.iter()
            .map(|m| {
                let content = m.get("content").and_then(|v| v.as_str()).unwrap_or("");
                // ~4 chars per token + overhead per message
                (content.len() as i64 / 4) + 10
            })
            .sum::<i64>()
    });

    // Check for memory files content in the session
    let memory_tokens = parsed
        .get("memory_content")
        .and_then(|v| v.as_str())
        .map_or(1_200, |s| s.len() as i64 / 4); // default 1200 if not in session

    (message_tokens, memory_tokens, message_count)
}

/// Format a token count with thousands separators.
fn format_tokens(n: i64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

#[cfg(test)]
#[path = "context.test.rs"]
mod tests;
