//! TUI runner — orchestrates TUI ↔ QueryEngine ↔ FileHistory.
//!
//! TS equivalent: REPL.tsx is the orchestrator (React component owns QueryEngine,
//! messages, file history, and permission state). In Rust we use an explicit
//! async task (`run_agent_driver`) since ratatui is not a reactive framework.
//!
//! Architecture:
//! ```text
//! ┌─────────────┐  UserCommand   ┌────────────────┐  LLM / tools  ┌────────────┐
//! │  TUI App    │ ──────────────>│  agent_driver   │ ──────────────>│ QueryEngine│
//! │  (ratatui)  │ <──────────────│  (tokio task)   │ <──────────────│            │
//! └─────────────┘ ServerNotif.   └────────────────┘  QueryEvent    └────────────┘
//!                                       │
//!                                 FileHistoryState
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tracing::info;
use tracing::warn;

use coco_context::FileHistoryState;
use coco_context::attachment::Attachment;
use coco_inference::ApiClient;
use coco_inference::RetryConfig;
use coco_query::QueryEngine;
use coco_query::QueryEngineConfig;
use coco_query::QueryEvent;
use coco_tool::ToolRegistry;
use coco_tui::App;
use coco_tui::ServerNotification;
use coco_tui::UserCommand;
use coco_tui::app::create_channels;
use tokio_util::sync::CancellationToken;

use crate::Cli;

/// Run the interactive TUI mode.
///
/// TS: launchRepl() → <REPL /> (React/Ink component).
/// Rust: spawns agent_driver as background task, runs TUI in foreground.
pub async fn run_tui(cli: &Cli) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let settings = coco_config::settings::load_settings(&cwd, None)?;
    let permission_mode = settings.merged.permissions.default_mode.unwrap_or_default();

    // Model + client
    let (model, mode) = crate::create_model(cli.model.as_deref());
    let model_id = model.model_id().to_string();
    let client = Arc::new(ApiClient::new(model, RetryConfig::default()));

    // Tools
    let mut registry = ToolRegistry::new();
    coco_tools::register_all_tools(&mut registry);
    let tools = Arc::new(registry);

    // System prompt
    let system_prompt = crate::build_system_prompt(&cwd, &model_id);

    // Config home for file history
    let config_home = dirs::home_dir()
        .map(|h| h.join(".coco"))
        .unwrap_or_else(|| PathBuf::from("/tmp/.coco"));

    // Session ID
    let session_id = uuid::Uuid::new_v4().to_string();

    // File read state — session-level cache for @mention dedup and change detection.
    // TS: readFileState (FileStateCache) — shared across tools and mentions.
    let file_read_state = Arc::new(RwLock::new(coco_context::FileReadState::new()));

    // File history (if enabled)
    // TS: fileHistoryEnabled() in fileHistory.ts — enabled by default
    let file_history = if settings.merged.file_checkpointing_enabled {
        Some(Arc::new(RwLock::new(FileHistoryState::new())))
    } else {
        None
    };

    // Create channels
    let (command_tx, command_rx, notification_tx, notification_rx) = create_channels();

    // Create TUI app
    let mut app = App::new(command_tx, notification_rx)
        .map_err(|e| anyhow::anyhow!("Failed to create TUI: {e}"))?;

    // Wire file_history_enabled into TUI session state so the rewind
    // overlay knows whether to show code restore options.
    app.state_mut().session.file_history_enabled = file_history.is_some();

    // Build engine config
    let engine_config = QueryEngineConfig {
        model_name: model_id.clone(),
        permission_mode,
        context_window: 200_000,
        max_output_tokens: 16_384,
        max_turns: 30,
        max_tokens: cli.max_tokens,
        system_prompt: Some(system_prompt),
        session_id: session_id.clone(),
        ..Default::default()
    };

    // Spawn agent driver
    let driver_handle = tokio::spawn(run_agent_driver(
        command_rx,
        notification_tx,
        client,
        tools,
        engine_config,
        file_read_state,
        file_history.clone(),
        config_home,
        session_id,
    ));

    eprintln!("coco-rs TUI ({mode} mode) — model: {model_id}\n");

    // Run TUI (blocks until exit)
    let tui_result = app.run().await;

    // Wait for agent driver
    let _ = driver_handle.await;

    tui_result.map_err(|e| anyhow::anyhow!("TUI error: {e}"))
}

/// Agent driver — consumes UserCommands, drives QueryEngine, emits ServerNotifications.
///
/// TS: REPL.tsx's onSubmit → query() → onQueryEvent() loop.
/// Runs as a background tokio task alongside the TUI event loop.
#[allow(clippy::too_many_arguments)]
async fn run_agent_driver(
    mut command_rx: mpsc::Receiver<UserCommand>,
    event_tx: mpsc::Sender<ServerNotification>,
    client: Arc<ApiClient>,
    tools: Arc<ToolRegistry>,
    engine_config: QueryEngineConfig,
    file_read_state: Arc<RwLock<coco_context::FileReadState>>,
    file_history: Option<Arc<RwLock<FileHistoryState>>>,
    config_home: PathBuf,
    session_id: String,
) {
    info!("Agent driver started");

    let cancel = CancellationToken::new();

    while let Some(command) = command_rx.recv().await {
        match command {
            UserCommand::SubmitInput {
                content, images, ..
            } => {
                if content.is_empty() {
                    continue;
                }

                let _ = event_tx
                    .send(ServerNotification::TurnStarted { turn_number: 1 })
                    .await;

                // Resolve @mentions into attachments.
                let processed = coco_context::process_user_input(&content);
                let cwd = std::env::current_dir().unwrap_or_default();

                let mut frs = file_read_state.write().await;
                let file_attachments = coco_context::resolve_mentions(
                    &processed.mentions,
                    &mut frs,
                    &coco_context::MentionResolveOptions {
                        cwd: &cwd,
                        max_dir_entries: 1000,
                    },
                )
                .await;

                // Detect files changed on disk since last read.
                let changed_file_attachments = coco_context::detect_changed_files(&mut frs).await;
                drop(frs);

                // Build user message (text + pasted images) and separate
                // attachment messages (file contents, changes).
                let messages = build_turn_messages(
                    &content,
                    &images,
                    &file_attachments,
                    &changed_file_attachments,
                );

                // Build engine with file history + file read state for this turn
                let mut engine = QueryEngine::new(
                    engine_config.clone(),
                    client.clone(),
                    tools.clone(),
                    cancel.clone(),
                    /*hooks*/ None,
                );
                engine = engine.with_file_read_state(file_read_state.clone());
                if let Some(ref fh) = file_history {
                    engine = engine.with_file_history(fh.clone(), config_home.clone());
                }

                // Run with event streaming
                let (query_event_tx, mut query_event_rx) = mpsc::channel::<QueryEvent>(256);

                let event_tx_clone = event_tx.clone();
                let forward_handle = tokio::spawn(async move {
                    while let Some(qe) = query_event_rx.recv().await {
                        let notification = map_query_event(qe);
                        if let Some(n) = notification {
                            let _ = event_tx_clone.send(n).await;
                        }
                    }
                });

                match engine.run_with_messages(messages, query_event_tx).await {
                    Ok(result) => {
                        let _ = event_tx
                            .send(ServerNotification::TurnCompleted {
                                usage: coco_tui::state::TokenUsage {
                                    input_tokens: result.total_usage.input_tokens,
                                    output_tokens: result.total_usage.output_tokens,
                                    cache_read_tokens: 0,
                                    cache_creation_tokens: 0,
                                },
                            })
                            .await;
                    }
                    Err(e) => {
                        let _ = event_tx
                            .send(ServerNotification::TurnFailed {
                                error: e.to_string(),
                            })
                            .await;
                    }
                }

                let _ = forward_handle.await;
            }

            UserCommand::Rewind {
                message_id,
                restore_type,
            } => {
                handle_rewind(
                    &restore_type,
                    &message_id,
                    &file_history,
                    &config_home,
                    &session_id,
                    &event_tx,
                )
                .await;
            }

            UserCommand::RequestDiffStats { message_id } => {
                // Async diff stats computation.
                // TS: fileHistoryGetDiffStats() in MessageSelector useEffect.
                if let Some(ref fh) = file_history {
                    let fh = fh.read().await;
                    let has_changes = fh
                        .has_any_changes(&message_id, &config_home, &session_id)
                        .await;
                    let (files, ins, del) = match fh
                        .get_diff_stats(&message_id, &config_home, &session_id)
                        .await
                    {
                        Ok(stats) => (
                            stats.files_changed.len() as i32,
                            stats.insertions,
                            stats.deletions,
                        ),
                        Err(_) => (0, 0, 0),
                    };
                    let _ = event_tx
                        .send(ServerNotification::DiffStatsLoaded {
                            message_id,
                            files_changed: files,
                            insertions: ins,
                            deletions: del,
                            has_any_changes: has_changes,
                        })
                        .await;
                }
            }

            UserCommand::Interrupt => {
                cancel.cancel();
            }

            UserCommand::Shutdown => {
                let _ = event_tx
                    .send(ServerNotification::SessionEnded {
                        reason: "User shutdown".into(),
                    })
                    .await;
                break;
            }

            // Other commands: log and skip for now
            other => {
                info!(?other, "Unhandled UserCommand in agent driver");
            }
        }
    }

    info!("Agent driver stopped");
}

/// Handle a rewind command.
///
/// TS: REPL.tsx rewindConversationTo() + fileHistoryRewind()
/// - Code rewind: calls file_history.rewind() to restore files
/// - Conversation rewind: emits RewindCompleted so TUI truncates messages
/// - Both: does both
async fn handle_rewind(
    restore_type: &coco_tui::state::RestoreType,
    message_id: &str,
    file_history: &Option<Arc<RwLock<FileHistoryState>>>,
    config_home: &PathBuf,
    session_id: &str,
    event_tx: &mpsc::Sender<ServerNotification>,
) {
    use coco_tui::state::RestoreType;

    let mut files_changed = 0i32;

    // Code rewind (file restore)
    // TS: fileHistoryRewind() in REPL.tsx onRestoreCode prop
    if matches!(restore_type, RestoreType::Both | RestoreType::CodeOnly) {
        if let Some(fh) = file_history {
            let fh = fh.read().await;
            match fh.rewind(message_id, config_home, session_id).await {
                Ok(changed) => {
                    files_changed = changed.len() as i32;
                    info!(files_changed, message_id, "File history rewind completed");
                }
                Err(e) => {
                    warn!("File history rewind failed: {e}");
                    let _ = event_tx
                        .send(ServerNotification::Error {
                            message: format!("File rewind failed: {e}"),
                            retryable: false,
                        })
                        .await;
                    return;
                }
            }
        }
    }

    // Conversation rewind: emit RewindCompleted so TUI truncates messages,
    // restores permission mode, and repopulates input.
    // TS: rewindConversationTo() + restoreMessageSync() in REPL.tsx
    let should_truncate = matches!(
        restore_type,
        RestoreType::Both | RestoreType::ConversationOnly
    );

    let _ = event_tx
        .send(ServerNotification::RewindCompleted {
            target_message_id: if should_truncate {
                message_id.to_string()
            } else {
                String::new()
            },
            files_changed,
        })
        .await;
}

/// Map a QueryEvent to a ServerNotification.
/// Build the list of messages for a turn: user message + attachment messages.
///
/// TS architecture: user message first, then separate attachment messages
/// wrapped in `<system-reminder>` tags with `is_meta: true`.
///
/// - User message: text + pasted clipboard images (inline content parts)
/// - Attachment messages: file contents, directories, changed files (separate messages)
fn build_turn_messages(
    text: &str,
    images: &[coco_tui::ImageData],
    file_attachments: &[Attachment],
    changed_file_attachments: &[Attachment],
) -> Vec<coco_types::Message> {
    use vercel_ai_provider::UserContentPart;

    let mut messages = Vec::new();

    // 1. User message: text + clipboard images
    if images.is_empty() {
        messages.push(coco_messages::create_user_message(text));
    } else {
        let mut parts: Vec<UserContentPart> = vec![UserContentPart::text(text)];
        for img in images {
            parts.push(UserContentPart::image(img.bytes.clone(), &img.mime));
        }
        messages.push(coco_messages::create_user_message_with_parts(parts));
    }

    // 2. @mention attachment messages (separate, wrapped in system-reminder)
    for att in file_attachments {
        if let Some(msg) = attachment_to_message(att) {
            messages.push(msg);
        }
    }

    // 3. Changed file notification messages
    for att in changed_file_attachments {
        if let Some(msg) = changed_file_to_message(att) {
            messages.push(msg);
        }
    }

    messages
}

/// Convert a resolved @mention attachment into a system-reminder message.
///
/// TS: `normalizeAttachmentForAPI()` — wraps file content in synthetic
/// tool-use/tool-result pairs inside `<system-reminder>` tags.
fn attachment_to_message(att: &Attachment) -> Option<coco_types::Message> {
    let read_tool = coco_types::ToolName::Read.as_str();
    let bash_tool = coco_types::ToolName::Bash.as_str();

    match att {
        Attachment::File(f) => {
            let text = format!(
                "Called the {read_tool} tool with the following input: \
                 {{\"file_path\":\"{}\"}}\n\
                 Result of calling the {read_tool} tool:\n{}",
                f.filename, f.content
            );
            Some(coco_messages::wrapping::create_system_reminder_message(
                &text,
            ))
        }
        Attachment::Image(img) => {
            if let Some(b64) = &img.base64_data {
                use vercel_ai_provider::FilePart;
                use vercel_ai_provider::UserContentPart;
                let parts = vec![
                    UserContentPart::text(coco_messages::wrapping::wrap_in_system_reminder(
                        &format!(
                            "Called the {read_tool} tool with the following input: \
                             {{\"file_path\":\"{}\"}}",
                            img.filename
                        ),
                    )),
                    UserContentPart::File(FilePart::image_base64(b64, &img.media_type)),
                ];
                Some(coco_messages::create_user_message_with_parts(parts))
            } else {
                None
            }
        }
        Attachment::Directory(d) => {
            let text = format!(
                "Called the {bash_tool} tool with the following input: \
                 {{\"command\":\"ls {}\",\"description\":\"Lists files in {}\"}}\n\
                 Result of calling the {bash_tool} tool:\n{}",
                d.display_path, d.display_path, d.content
            );
            Some(coco_messages::wrapping::create_system_reminder_message(
                &text,
            ))
        }
        Attachment::AlreadyReadFile(_) | Attachment::AgentMention(_) => None,
        _ => None,
    }
}

/// Convert a changed-file attachment into a notification message.
///
/// TS: `normalizeAttachmentForAPI()` for `edited_text_file` type — sends a
/// note explaining the file was modified externally, with a diff snippet.
fn changed_file_to_message(att: &Attachment) -> Option<coco_types::Message> {
    match att {
        Attachment::File(f) => {
            let text = format!(
                "Note: {} was modified, either by the user or by a linter. \
                 This change was intentional, so make sure to take it into \
                 account as you proceed (ie. don't revert it unless the user \
                 asks you to). Don't tell the user this, since they are already \
                 aware. Here are the relevant changes (shown with line numbers):\n{}",
                f.display_path, f.content
            );
            Some(coco_messages::wrapping::create_system_reminder_message(
                &text,
            ))
        }
        _ => None,
    }
}

fn map_query_event(event: QueryEvent) -> Option<ServerNotification> {
    match event {
        QueryEvent::TextDelta { text } => Some(ServerNotification::TextDelta { delta: text }),
        QueryEvent::ReasoningDelta { text } => {
            Some(ServerNotification::ThinkingDelta { delta: text })
        }
        QueryEvent::ToolUseStart {
            tool_use_id,
            tool_name,
            ..
        } => Some(ServerNotification::ToolUseQueued {
            call_id: tool_use_id,
            name: tool_name,
            input_preview: String::new(),
        }),
        QueryEvent::ToolUseEnd {
            tool_use_id,
            is_error,
            ..
        } => Some(ServerNotification::ToolUseCompleted {
            call_id: tool_use_id,
            output: String::new(),
            is_error,
        }),
        QueryEvent::TurnStarted { turn } => {
            Some(ServerNotification::TurnStarted { turn_number: turn })
        }
        QueryEvent::TurnCompleted { .. } => None, // Handled at run() return
        QueryEvent::CompactionTriggered => Some(ServerNotification::ContextCompacted {
            removed_messages: 0,
            summary_tokens: 0,
        }),
        _ => None,
    }
}
