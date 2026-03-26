//! cocode app-server: WebSocket/stdio server for IDE and browser integration.
//!
//! Architecture:
//! - **Transport layer** (`transport.rs`): Axum WebSocket + stdio NDJSON
//! - **Processor** (`processor.rs`): JSON-RPC dispatch, session/turn lifecycle
//! - **Connection** (`connection.rs`): Per-connection state tracking
//! - **Shared modules**: `event_mapper`, `permission`, `session_builder`
//!   are reused by both the app-server and the CLI SDK mode.

mod connection;
mod error_code;
pub mod event_mapper;
pub mod permission;
pub mod processor;
pub mod session_builder;
mod session_factory;
mod transport;
mod turn_runner;

use connection::OUTBOUND_CHANNEL_CAPACITY;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing::warn;
use transport::TransportEvent;

pub use transport::AppServerTransport;

/// Shutdown phase for graceful drain.
enum ShutdownPhase {
    Running,
    Draining,
}

/// Run the app-server with the specified transport.
///
/// This is the main entry point for both the binary and library consumers.
pub async fn run_main_with_transport(transport: AppServerTransport) -> anyhow::Result<()> {
    init_logging();

    let (event_tx, mut event_rx) = mpsc::channel::<TransportEvent>(OUTBOUND_CHANNEL_CAPACITY);

    // Start transport
    let mut io_handles = Vec::<JoinHandle<()>>::new();
    let ws_shutdown = CancellationToken::new();
    let single_client = matches!(transport, AppServerTransport::Stdio);

    match transport {
        AppServerTransport::Stdio => {
            let handles = transport::start_stdio_transport(event_tx.clone()).await?;
            io_handles.extend(handles);
        }
        AppServerTransport::WebSocket { bind_address } => {
            let handle = transport::start_websocket_transport(
                bind_address,
                event_tx.clone(),
                ws_shutdown.clone(),
            )
            .await?;
            io_handles.push(handle);
        }
    }

    // Create config manager for session creation
    let config = cocode_config::ConfigManager::from_default()
        .map_err(|e| anyhow::anyhow!("failed to load config: {e}"))?;

    let mut proc = processor::Processor::new(config);
    let mut phase = ShutdownPhase::Running;

    loop {
        if matches!(phase, ShutdownPhase::Draining) && proc.active_session_count() == 0 {
            info!("Drain complete, shutting down");
            break;
        }

        tokio::select! {
            event = event_rx.recv() => {
                let Some(event) = event else { break };
                match event {
                    TransportEvent::ConnectionOpened {
                        connection_id,
                        writer,
                    } => {
                        let _span = tracing::info_span!("connection", %connection_id).entered();
                        proc.on_connection_opened(connection_id, writer);
                        info!("Connection opened");
                    }
                    TransportEvent::ConnectionClosed { connection_id } => {
                        proc.on_connection_closed(connection_id);
                        info!(%connection_id, "Connection closed");
                        if single_client && proc.connection_count() == 0 {
                            break;
                        }
                    }
                    TransportEvent::IncomingMessage {
                        connection_id,
                        message,
                    } => {
                        if !proc.has_connection(connection_id) {
                            warn!(%connection_id, "Message from unknown connection");
                            continue;
                        }
                        proc.handle_message(connection_id, message).await;
                    }
                }
            }
            _ = shutdown_signal(), if matches!(phase, ShutdownPhase::Running) => {
                info!("Received shutdown signal, entering drain mode");
                phase = ShutdownPhase::Draining;
                ws_shutdown.cancel();
                proc.initiate_drain();
            }
        }
    }

    // Shutdown
    ws_shutdown.cancel();
    drop(event_tx);
    for h in io_handles {
        let _ = h.await;
    }

    info!("App-server shut down");
    Ok(())
}

/// Wait for a shutdown signal (CTRL-C or SIGTERM on Unix).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::SignalKind;
        use tokio::signal::unix::signal;
        let mut term = signal(SignalKind::terminate()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = async {
                if let Some(ref mut s) = term { s.recv().await; }
                else { std::future::pending::<()>().await; }
            } => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Initialize stderr-only logging for the app-server.
fn init_logging() {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt;
    use tracing_subscriber::prelude::*;

    let _ = tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .compact()
                .with_filter(EnvFilter::from_default_env()),
        )
        .try_init();
}
