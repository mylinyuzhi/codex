//! IDE integration errors.

use snafu::Snafu;

/// IDE integration result type.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// IDE integration errors.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    /// Failed to read or parse a lockfile.
    #[snafu(display("lockfile error: {message}"))]
    Lockfile { message: String },

    /// Failed to connect to IDE MCP server.
    #[snafu(display("connection failed: {message}"))]
    Connection { message: String },

    /// IDE MCP tool call failed.
    #[snafu(display("tool call '{tool}' failed: {message}"))]
    ToolCall { tool: String, message: String },

    /// IDE disconnected unexpectedly.
    #[snafu(display("IDE disconnected"))]
    Disconnected,

    /// IO error.
    #[snafu(display("IO error: {source}"))]
    Io { source: std::io::Error },
}
