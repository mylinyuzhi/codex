//! Retrieval CLI - Testing tool for the retrieval system.
//!
//! Provides interactive commands for testing indexing and search capabilities.

use std::io::BufRead;
use std::io::Write;
use std::io::{self};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use clap::Subcommand;

use codex_retrieval::FileWatcher;
use codex_retrieval::IndexManager;
use codex_retrieval::RebuildMode;
use codex_retrieval::RetrievalConfig;
use codex_retrieval::RetrievalService;
use codex_retrieval::SnippetStorageExt;
use codex_retrieval::SqliteStore;
use codex_retrieval::SymbolQuery;

/// Extract workspace name from a directory path.
fn workspace_name(workdir: &Path) -> &str {
    workdir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("default")
}

/// Create default features for BM25-only search.
fn bm25_features() -> codex_retrieval::RetrievalFeatures {
    codex_retrieval::RetrievalFeatures {
        code_search: true,
        query_rewrite: true,
        vector_search: false,
    }
}

/// Create features for hybrid search (BM25 + vector if available).
fn hybrid_features() -> codex_retrieval::RetrievalFeatures {
    codex_retrieval::RetrievalFeatures {
        code_search: true,
        query_rewrite: true,
        vector_search: true,
    }
}

#[derive(Parser)]
#[command(name = "retrieval-cli")]
#[command(about = "Testing tool for the retrieval system")]
struct Cli {
    /// Working directory to index/search
    #[arg(default_value = ".")]
    workdir: PathBuf,

    /// Path to config file (default: {workdir}/.codex/retrieval.toml or ~/.codex/retrieval.toml)
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Run a single command and exit (instead of REPL mode)
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show index status
    Status,

    /// Build/rebuild the index
    Build {
        /// Clean all existing data before rebuilding
        #[arg(long)]
        clean: bool,
    },

    /// Watch for file changes and auto-index
    Watch,

    /// Hybrid search (BM25 + vector + snippet)
    Search {
        /// Search query
        query: String,
        /// Maximum results
        #[arg(short, long, default_value = "10")]
        limit: i32,
    },

    /// BM25 full-text search only
    Bm25 {
        /// Search query
        query: String,
        /// Maximum results
        #[arg(short, long, default_value = "10")]
        limit: i32,
    },

    /// Vector similarity search only
    Vector {
        /// Search query
        query: String,
        /// Maximum results
        #[arg(short, long, default_value = "10")]
        limit: i32,
    },

    /// Symbol-based search (functions, classes, etc.)
    Snippet {
        /// Search query (e.g., "fn:handle" or "type:struct name:Config")
        query: String,
        /// Maximum results
        #[arg(short, long, default_value = "10")]
        limit: i32,
    },

    /// Show current configuration
    Config,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("codex_retrieval=info".parse()?),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    // Canonicalize workdir
    let workdir = cli.workdir.canonicalize().unwrap_or(cli.workdir.clone());

    // Load config from specified file or default locations
    let config = if let Some(config_path) = &cli.config {
        if !config_path.exists() {
            anyhow::bail!("Config file not found: {}", config_path.display());
        }
        RetrievalConfig::from_file(config_path)?
    } else {
        RetrievalConfig::load(&workdir)?
    };

    if !config.enabled {
        println!("Retrieval is not enabled.");
        if cli.config.is_some() {
            println!("Set 'enabled = true' in your config file.");
        } else {
            println!(
                "Create a config file at: {}/.codex/retrieval.toml",
                workdir.display()
            );
            println!("\nExample config:");
            println!("[retrieval]");
            println!("enabled = true");
        }
        return Ok(());
    }

    // Show which config is being used
    if let Some(config_path) = &cli.config {
        eprintln!("Using config: {}", config_path.display());
    }

    match cli.command {
        Some(cmd) => run_command(cmd, &workdir, &config).await?,
        None => run_repl(&workdir, &config, cli.config.as_ref()).await?,
    }

    Ok(())
}

async fn run_command(
    cmd: Command,
    workdir: &PathBuf,
    config: &RetrievalConfig,
) -> anyhow::Result<()> {
    match cmd {
        Command::Status => cmd_status(workdir, config).await?,
        Command::Build { clean } => cmd_build(workdir, config, clean).await?,
        Command::Watch => cmd_watch(workdir, config).await?,
        Command::Search { query, limit } => cmd_search(config, &query, limit).await?,
        Command::Bm25 { query, limit } => cmd_bm25(config, &query, limit).await?,
        Command::Vector { query, limit } => cmd_vector(config, &query, limit).await?,
        Command::Snippet { query, limit } => cmd_snippet(workdir, config, &query, limit).await?,
        Command::Config => cmd_config(config)?,
    }
    Ok(())
}

async fn run_repl(
    workdir: &PathBuf,
    config: &RetrievalConfig,
    config_path: Option<&PathBuf>,
) -> anyhow::Result<()> {
    println!("Retrieval CLI v0.1");
    if let Some(path) = config_path {
        println!("Config: {}", path.display());
    } else {
        println!(
            "Config: {}/.codex/retrieval.toml (or ~/.codex/retrieval.toml)",
            workdir.display()
        );
    }
    println!("Data: {}", config.data_dir.display());
    println!(
        "\nCommands: status, build [--clean], watch, search <query>, bm25 <query>, vector <query>, snippet <query>, config, quit"
    );
    println!();

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("> ");
        stdout.flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break; // EOF
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        let cmd = parts.first().unwrap_or(&"");

        let result = match *cmd {
            "quit" | "exit" | "q" => break,
            "status" => cmd_status(workdir, config).await,
            "build" => {
                let clean = parts.get(1).map(|s| *s == "--clean").unwrap_or(false);
                cmd_build(workdir, config, clean).await
            }
            "watch" => cmd_watch(workdir, config).await,
            "search" => {
                let query = parts[1..].join(" ");
                if query.is_empty() {
                    println!("Usage: search <query>");
                    continue;
                }
                cmd_search(config, &query, 10).await
            }
            "bm25" => {
                let query = parts[1..].join(" ");
                if query.is_empty() {
                    println!("Usage: bm25 <query>");
                    continue;
                }
                cmd_bm25(config, &query, 10).await
            }
            "vector" => {
                let query = parts[1..].join(" ");
                if query.is_empty() {
                    println!("Usage: vector <query>");
                    continue;
                }
                cmd_vector(config, &query, 10).await
            }
            "snippet" => {
                let query = parts[1..].join(" ");
                if query.is_empty() {
                    println!("Usage: snippet <query>");
                    continue;
                }
                cmd_snippet(workdir, config, &query, 10).await
            }
            "config" => cmd_config(config),
            "help" | "?" => {
                println!("Commands:");
                println!("  status         - Show index status");
                println!("  build [--clean] - Build index (--clean for full rebuild)");
                println!("  watch          - Watch for file changes");
                println!("  search <query> - Hybrid search");
                println!("  bm25 <query>   - BM25 full-text search");
                println!("  vector <query> - Vector similarity search");
                println!("  snippet <query> - Symbol-based search");
                println!("  config         - Show configuration");
                println!("  quit           - Exit");
                continue;
            }
            _ => {
                println!(
                    "Unknown command: {}. Type 'help' for available commands.",
                    cmd
                );
                continue;
            }
        };

        if let Err(e) = result {
            println!("Error: {e}");
        }
    }

    Ok(())
}

async fn cmd_status(workdir: &PathBuf, config: &RetrievalConfig) -> anyhow::Result<()> {
    use codex_retrieval::storage::SqliteStore;

    let db_path = config.data_dir.join("retrieval.db");
    if !db_path.exists() {
        println!("Index not found. Run 'build' to create it.");
        return Ok(());
    }

    let store = Arc::new(SqliteStore::open(&db_path)?);
    let manager = IndexManager::new(config.clone(), store);

    let workspace = workspace_name(workdir);
    let stats = manager.get_stats(workspace).await?;

    println!("Workspace: {}", workspace);
    println!("Files indexed: {}", stats.file_count);
    println!("Total chunks: {}", stats.chunk_count);
    if let Some(ts) = stats.last_indexed {
        let dt = chrono::DateTime::from_timestamp(ts, 0)
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "unknown".to_string());
        println!("Last indexed: {}", dt);
    } else {
        println!("Last indexed: never");
    }
    println!("Watch enabled: {}", config.indexing.watch_enabled);

    Ok(())
}

async fn cmd_build(workdir: &PathBuf, config: &RetrievalConfig, clean: bool) -> anyhow::Result<()> {
    use codex_retrieval::storage::SqliteStore;

    // Ensure data directory exists
    std::fs::create_dir_all(&config.data_dir)?;

    let db_path = config.data_dir.join("retrieval.db");
    let store = Arc::new(SqliteStore::open(&db_path)?);
    let mut manager = IndexManager::new(config.clone(), store);

    let workspace = workspace_name(workdir);

    let mode = if clean {
        println!("[Clean] Deleting old index...");
        RebuildMode::Clean
    } else {
        println!("[Incremental] Scanning for changes...");
        RebuildMode::Incremental
    };

    let mut rx = manager.rebuild(workspace, workdir, mode).await?;

    // Process progress updates
    while let Some(progress) = rx.recv().await {
        match progress.status {
            codex_retrieval::indexing::IndexStatus::Loading => {
                println!("{}", progress.description);
            }
            codex_retrieval::indexing::IndexStatus::Indexing => {
                let pct = (progress.progress * 100.0) as i32;
                println!("[{:3}%] {}", pct, progress.description);
            }
            codex_retrieval::indexing::IndexStatus::Done => {
                println!("Done: {}", progress.description);
            }
            codex_retrieval::indexing::IndexStatus::Failed => {
                println!("Failed: {}", progress.description);
            }
            _ => {}
        }
    }

    Ok(())
}

async fn cmd_watch(workdir: &PathBuf, config: &RetrievalConfig) -> anyhow::Result<()> {
    use codex_retrieval::storage::SqliteStore;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    println!("[Watch] Watching for changes (Ctrl+C to stop)...");

    let debounce_ms = config.indexing.watch_debounce_ms.max(0) as u64;
    let watcher = FileWatcher::new(workdir, debounce_ms)?;

    // Ensure data directory exists
    std::fs::create_dir_all(&config.data_dir)?;

    let db_path = config.data_dir.join("retrieval.db");
    let store = Arc::new(SqliteStore::open(&db_path)?);
    let mut manager = IndexManager::new(config.clone(), store);

    let workspace = workspace_name(workdir);

    // Set up signal handler for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    tokio::spawn(async move {
        if let Ok(()) = tokio::signal::ctrl_c().await {
            r.store(false, Ordering::SeqCst);
        }
    });

    while running.load(Ordering::SeqCst) {
        if let Some(events) = watcher.recv_timeout(Duration::from_millis(500)) {
            let count = events.len();
            for event in &events {
                let kind = match event.kind {
                    codex_retrieval::WatchEventKind::Created => "created",
                    codex_retrieval::WatchEventKind::Modified => "modified",
                    codex_retrieval::WatchEventKind::Deleted => "deleted",
                };
                println!("[Change] {} {}", event.path.display(), kind);
            }

            if count > 0 {
                println!("[Indexing] {} file(s) changed, re-indexing...", count);
                let mut rx = manager
                    .rebuild(workspace, workdir, RebuildMode::Incremental)
                    .await?;

                // Drain progress updates
                while let Some(progress) = rx.recv().await {
                    if progress.status == codex_retrieval::indexing::IndexStatus::Done {
                        println!("[Done] {}", progress.description);
                        break;
                    }
                }
            }
        }
    }

    println!("\n[Watch] Stopped watching.");
    Ok(())
}

async fn cmd_search(config: &RetrievalConfig, query: &str, limit: i32) -> anyhow::Result<()> {
    let service = RetrievalService::new(config.clone(), hybrid_features()).await?;
    let results = service.search_with_limit(query, Some(limit)).await?;

    println!("[Hybrid] Found {} results:\n", results.len());

    for (i, result) in results.iter().enumerate() {
        println!(
            "{}. {}:{}-{} (score: {:.3}, type: {:?})",
            i + 1,
            result.chunk.filepath,
            result.chunk.start_line,
            result.chunk.end_line,
            result.score,
            result.score_type
        );
        // Show first 2 lines of content
        let lines: Vec<&str> = result.chunk.content.lines().take(2).collect();
        for line in lines {
            println!("   {}", line.trim());
        }
        println!();
    }

    Ok(())
}

async fn cmd_bm25(config: &RetrievalConfig, query: &str, limit: i32) -> anyhow::Result<()> {
    let service = RetrievalService::new(config.clone(), bm25_features()).await?;
    let results = service.search_bm25(query, limit).await?;

    println!("[BM25] Found {} results:\n", results.len());

    for (i, result) in results.iter().enumerate() {
        println!(
            "{}. {}:{}-{} (score: {:.3})",
            i + 1,
            result.chunk.filepath,
            result.chunk.start_line,
            result.chunk.end_line,
            result.score
        );
        let lines: Vec<&str> = result.chunk.content.lines().take(2).collect();
        for line in lines {
            println!("   {}", line.trim());
        }
        println!();
    }

    Ok(())
}

async fn cmd_vector(config: &RetrievalConfig, query: &str, limit: i32) -> anyhow::Result<()> {
    let service = RetrievalService::new(config.clone(), hybrid_features()).await?;

    if !service.has_vector_search() {
        println!("[Vector] Vector search not available (embeddings not configured)");
        return Ok(());
    }

    let results = service.search_vector(query, limit).await?;

    println!("[Vector] Found {} results:\n", results.len());

    for (i, result) in results.iter().enumerate() {
        println!(
            "{}. {}:{}-{} (score: {:.3})",
            i + 1,
            result.chunk.filepath,
            result.chunk.start_line,
            result.chunk.end_line,
            result.score
        );
        let lines: Vec<&str> = result.chunk.content.lines().take(2).collect();
        for line in lines {
            println!("   {}", line.trim());
        }
        println!();
    }

    Ok(())
}

async fn cmd_snippet(
    workdir: &PathBuf,
    config: &RetrievalConfig,
    query: &str,
    limit: i32,
) -> anyhow::Result<()> {
    let db_path = config.data_dir.join("retrieval.db");

    if !db_path.exists() {
        println!("[Snippet] Index not found. Run 'build' first.");
        return Ok(());
    }

    let store = Arc::new(SqliteStore::open(&db_path)?);
    let snippet_store = SnippetStorageExt::new(store);

    let workspace = workspace_name(workdir);

    // Parse symbol query (e.g., "type:function name:handle")
    let symbol_query = SymbolQuery::parse(query);

    let results = snippet_store
        .search_fts(workspace, &symbol_query, limit)
        .await?;

    println!("[Snippet] Found {} symbols:\n", results.len());

    for (i, snippet) in results.iter().enumerate() {
        println!(
            "{}. {} {} ({}:{}-{})",
            i + 1,
            snippet.syntax_type,
            snippet.name,
            snippet.filepath,
            snippet.start_line,
            snippet.end_line
        );
        if let Some(sig) = &snippet.signature {
            println!("   {}", sig);
        }
    }

    Ok(())
}

fn cmd_config(config: &RetrievalConfig) -> anyhow::Result<()> {
    println!("Configuration:");
    println!("  enabled: {}", config.enabled);
    println!("  data_dir: {}", config.data_dir.display());
    println!();
    println!("Indexing:");
    println!("  max_file_size_mb: {}", config.indexing.max_file_size_mb);
    println!("  batch_size: {}", config.indexing.batch_size);
    println!("  watch_enabled: {}", config.indexing.watch_enabled);
    println!("  watch_debounce_ms: {}", config.indexing.watch_debounce_ms);
    println!();
    println!("Chunking:");
    println!("  max_tokens: {}", config.chunking.max_tokens);
    println!("  overlap_tokens: {}", config.chunking.overlap_tokens);
    println!();
    println!("Search:");
    println!("  n_final: {}", config.search.n_final);
    println!("  bm25_weight: {}", config.search.bm25_weight);
    println!("  vector_weight: {}", config.search.vector_weight);
    println!("  snippet_weight: {}", config.search.snippet_weight);
    println!();
    println!(
        "Embedding: {}",
        if config.embedding.is_some() {
            "configured"
        } else {
            "not configured"
        }
    );
    println!(
        "Query Rewrite: {}",
        if config.query_rewrite.is_some() {
            "configured"
        } else {
            "not configured"
        }
    );

    Ok(())
}
