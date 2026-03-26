//! Entry point for the cocode app-server binary.
//!
//! Supports two transport modes:
//! - `stdio://` (default): NDJSON over stdin/stdout
//! - `ws://IP:PORT`: WebSocket with JSON-RPC 2.0

use clap::Parser;
use cocode_app_server::AppServerTransport;
use cocode_app_server::run_main_with_transport;

#[derive(Debug, Parser)]
#[command(name = "cocode-app-server")]
struct AppServerArgs {
    /// Transport endpoint URL: `stdio://` (default) or `ws://IP:PORT`.
    #[arg(
        long = "listen",
        value_name = "URL",
        default_value = AppServerTransport::DEFAULT_LISTEN_URL
    )]
    listen: AppServerTransport,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = AppServerArgs::parse();
    run_main_with_transport(args.listen).await
}
