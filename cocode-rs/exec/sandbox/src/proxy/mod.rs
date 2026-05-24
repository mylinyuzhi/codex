//! Network proxy for sandboxed command network filtering.
//!
//! Provides HTTP CONNECT and SOCKS5 proxy servers that enforce
//! domain-based allow/deny filtering for sandboxed commands.
//! On Linux, includes socat-based bridges for proxy access across
//! network namespaces created by bubblewrap `--unshare-net`.

pub mod bridge;
mod filter;
mod server;

pub use bridge::BridgeManager;
pub use bridge::BridgePorts;
pub use filter::DomainFilter;
pub use server::ProxyServer;
