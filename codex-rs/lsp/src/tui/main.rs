//! LSP Test TUI - A simple TUI for testing codex-lsp functionality.

mod app;
mod event;
mod ops;
mod ui;

use anyhow::Result;
use app::App;
use clap::Parser;
use codex_lsp::DiagnosticsStore;
use codex_lsp::LspServerManager;
use crossterm::execute;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use event::Event;
use ratatui::prelude::*;
use std::fs::OpenOptions;
use std::io::stdout;
use std::io::{self};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing_appender::non_blocking;
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::prelude::*;

#[derive(Parser, Debug)]
#[command(name = "lsp-tui")]
#[command(about = "TUI for testing codex-lsp functionality")]
struct Args {
    /// Project root directory (workspace)
    #[arg(default_value = ".")]
    workspace: PathBuf,

    /// Initial file to open (optional)
    #[arg(short, long)]
    file: Option<PathBuf>,
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<Event>(32);

    // Spawn input reader task
    let tx_input = tx.clone();
    tokio::spawn(async move {
        loop {
            if crossterm::event::poll(std::time::Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(event) = crossterm::event::read() {
                    match event {
                        crossterm::event::Event::Key(key) => {
                            if tx_input.send(Event::Key(key)).await.is_err() {
                                break;
                            }
                        }
                        crossterm::event::Event::Resize(w, h) => {
                            if tx_input.send(Event::Resize(w, h)).await.is_err() {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    // Spawn tick task for periodic updates
    let tx_tick = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
        loop {
            interval.tick().await;
            if tx_tick.send(Event::Tick).await.is_err() {
                break;
            }
        }
    });

    loop {
        // Draw UI
        terminal.draw(|f| ui::render(app, f))?;

        // Handle events
        if let Some(event) = rx.recv().await {
            app.handle_event(event, tx.clone()).await?;
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI args
    let args = Args::parse();

    // Canonicalize the workspace path
    let workspace = args.workspace.canonicalize().unwrap_or(args.workspace);

    // Initialize file-based logging to ~/.codex/log/lsp.log
    let log_dir = dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".codex")
        .join("log");
    std::fs::create_dir_all(&log_dir)?;

    let mut log_file_opts = OpenOptions::new();
    log_file_opts.create(true).append(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        log_file_opts.mode(0o600);
    }

    let log_file = log_file_opts.open(log_dir.join("lsp.log"))?;

    let (non_blocking_writer, _guard) = non_blocking(log_file);

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("codex_lsp=info"));

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking_writer)
        .with_target(true)
        .with_ansi(false)
        .with_filter(env_filter);

    tracing_subscriber::registry().with(file_layer).init();

    // Initialize LSP manager
    let diagnostics = Arc::new(DiagnosticsStore::new());
    let manager = Arc::new(LspServerManager::with_auto_config(
        Some(&workspace),
        diagnostics.clone(),
    ));

    // Setup terminal
    let mut terminal = setup_terminal()?;

    // Create app state
    let mut app = App::new(workspace, manager, diagnostics);

    // Set initial file if provided
    if let Some(file) = args.file {
        app.set_file(file);
    }

    // Run event loop
    let result = run_event_loop(&mut terminal, &mut app).await;

    // Cleanup
    restore_terminal(&mut terminal)?;

    result
}
