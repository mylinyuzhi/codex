//! `/clear` — clear conversation history and optionally plan state.
//!
//! Returns a status message describing what was cleared. Actual state
//! clearing happens in the TUI layer; this handler only reports intent.

use std::pin::Pin;

/// Async handler for `/clear [all|history]`.
///
/// Checks `~/.cocode/sessions/` to provide context on preserved history,
/// then returns a message describing the clear operation.
pub fn handler(
    args: String,
) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let subcommand = args.trim().to_string();

        let sessions_dir = dirs::home_dir()
            .map(|h| h.join(".cocode").join("sessions"))
            .unwrap_or_default();

        let session_count = count_session_files(&sessions_dir).await;

        match subcommand.as_str() {
            "" => {
                let mut out = String::from(
                    "Conversation cleared. Starting fresh.\nHistory preserved for /resume.",
                );
                if session_count > 0 {
                    out.push_str(&format!(
                        "\n\n{session_count} session(s) available via /session list."
                    ));
                }
                Ok(out)
            }
            "all" => Ok("Conversation and plan state cleared.".to_string()),
            "history" => Ok("Message history cleared. Tools and state preserved.".to_string()),
            other => Ok(format!(
                "Unknown clear subcommand: {other}\n\n\
                 Usage:\n\
                 /clear           Clear conversation, preserve session for /resume\n\
                 /clear all       Clear conversation and plan state\n\
                 /clear history   Clear message history only"
            )),
        }
    })
}

/// Count JSON session files in the sessions directory.
async fn count_session_files(sessions_dir: &std::path::Path) -> i64 {
    if !sessions_dir.exists() {
        return 0;
    }

    let Ok(mut entries) = tokio::fs::read_dir(sessions_dir).await else {
        return 0;
    };

    let mut count: i64 = 0;
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
#[path = "clear.test.rs"]
mod tests;
