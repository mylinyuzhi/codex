//! `/hooks` — show configured hook event handlers.
//!
//! Reads hook configuration from `.claude/settings.json` and
//! `~/.cocode/settings.json`, then displays all hooks grouped by event type.

use std::path::Path;
use std::pin::Pin;

/// A discovered hook entry from a settings file.
struct HookEntry {
    event: String,
    matcher: Option<String>,
    handler_type: String,
    source: String,
}

/// Human-readable description for each recognized hook event type.
const EVENT_DESCRIPTIONS: &[(&str, &str)] = &[
    ("PreToolUse", "Runs before a tool executes (can block)"),
    ("PostToolUse", "Runs after a tool completes"),
    ("Stop", "Runs when the agent turn ends"),
];

/// Async handler for `/hooks`.
pub fn handler(
    args: String,
) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>> {
    Box::pin(async move {
        let _ = args; // no subcommands for now
        list_hooks().await
    })
}

/// Gather and display hooks from all settings sources.
async fn list_hooks() -> anyhow::Result<String> {
    let mut hooks = Vec::new();

    load_hooks_from_file(
        Path::new(".claude/settings.json"),
        ".claude/settings.json",
        &mut hooks,
    )
    .await;

    if let Some(home) = dirs::home_dir() {
        let user_settings = home.join(".cocode").join("settings.json");
        load_hooks_from_file(&user_settings, "~/.cocode/settings.json", &mut hooks).await;
    }

    let mut out = String::from("## Configured Hooks\n\n");

    if hooks.is_empty() {
        out.push_str("No hooks configured.\n\n");
        out.push_str("Add hooks to .claude/settings.json or ~/.cocode/settings.json:\n\n");
        out.push_str("  {\n");
        out.push_str("    \"hooks\": {\n");
        out.push_str("      \"PreToolUse\": [\n");
        out.push_str("        {\n");
        out.push_str("          \"matcher\": \"Bash\",\n");
        out.push_str("          \"hooks\": [\n");
        out.push_str("            { \"type\": \"command\", \"command\": \"echo pre-bash\" }\n");
        out.push_str("          ]\n");
        out.push_str("        }\n");
        out.push_str("      ]\n");
        out.push_str("    }\n");
        out.push_str("  }\n\n");
        out.push_str("Hook event types:\n");
        for (event, desc) in EVENT_DESCRIPTIONS {
            out.push_str(&format!("  {event:<14} {desc}\n"));
        }
    } else {
        // Group hooks by event type, respecting the canonical ordering.
        let event_order: Vec<&str> = EVENT_DESCRIPTIONS.iter().map(|(e, _)| *e).collect();
        let mut remaining: Vec<&str> = hooks
            .iter()
            .map(|h| h.event.as_str())
            .filter(|e| !event_order.contains(e))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        remaining.sort_unstable();

        let mut all_events: Vec<&str> = event_order.clone();
        all_events.extend(remaining.iter().copied());

        out.push_str(&format!(
            "{} hook{} configured:\n",
            hooks.len(),
            if hooks.len() == 1 { "" } else { "s" }
        ));

        for event in all_events {
            let group: Vec<&HookEntry> = hooks.iter().filter(|h| h.event == event).collect();
            if group.is_empty() {
                continue;
            }

            let desc = EVENT_DESCRIPTIONS
                .iter()
                .find(|(e, _)| *e == event)
                .map(|(_, d)| *d)
                .unwrap_or("");

            out.push_str(&format!("\n### {event}"));
            if !desc.is_empty() {
                out.push_str(&format!("  — {desc}"));
            }
            out.push('\n');

            for hook in group {
                out.push_str(&format!("  [{}]  type: {}", hook.source, hook.handler_type));
                if let Some(matcher) = &hook.matcher {
                    out.push_str(&format!("  matcher: {matcher}"));
                }
                out.push('\n');
            }
        }
    }

    Ok(out)
}

/// Load hook entries from a JSON settings file.
async fn load_hooks_from_file(path: &Path, source_label: &str, hooks: &mut Vec<HookEntry>) {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return;
    };

    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
        return;
    };

    let Some(hooks_obj) = parsed.get("hooks").and_then(|v| v.as_object()) else {
        return;
    };

    for (event, entries) in hooks_obj {
        let Some(entries_arr) = entries.as_array() else {
            continue;
        };

        for entry in entries_arr {
            let matcher = entry
                .get("matcher")
                .and_then(|v| v.as_str())
                .map(String::from);

            // Each entry may have an inner "hooks" array with the actual handlers.
            if let Some(inner_hooks) = entry.get("hooks").and_then(|v| v.as_array()) {
                for inner in inner_hooks {
                    let handler_type = determine_handler_type(inner);
                    hooks.push(HookEntry {
                        event: event.clone(),
                        matcher: matcher.clone(),
                        handler_type,
                        source: source_label.to_string(),
                    });
                }
            } else {
                // Entry itself is a handler definition.
                let handler_type = determine_handler_type(entry);
                hooks.push(HookEntry {
                    event: event.clone(),
                    matcher,
                    handler_type,
                    source: source_label.to_string(),
                });
            }
        }
    }
}

/// Determine the handler type string from a hook definition object.
fn determine_handler_type(hook: &serde_json::Value) -> String {
    hook.get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string()
}

#[cfg(test)]
#[path = "hooks.test.rs"]
mod tests;
