//! Tracing subscriber bootstrap for the `coco` binary.
//!
//! Single entry point: [`install`]. Called once from `main()` after
//! `Cli::parse()` so subscriber config can honor flags AND env. Returns
//! a [`SubscriberHandle`] the binary must keep alive for the process
//! lifetime — dropping it flushes the non-blocking file appender.
//!
//! Resolution priority for the filter directive:
//!   `--log-level` > `COCO_LOG` > `RUST_LOG` > `coco_otel::subscriber::DEFAULT_FILTER`.
//!
//! `COCO_LOG_FORMAT`, `COCO_LOG_FILE`, `COCO_LOG_STDERR`,
//! `COCO_LOG_TIMEZONE` mirror their `--log-*` flag counterparts at
//! lower priority.

use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::Result;
use coco_config::EnvKey;
use coco_config::env;
use coco_config::global_config;
use coco_otel::subscriber::Format;
use coco_otel::subscriber::Mode;
use coco_otel::subscriber::SubscriberHandle;
use coco_otel::subscriber::SubscriberOpts;
use coco_otel::subscriber::TimezoneConfig;
use coco_otel::subscriber::init_subscriber;

use crate::Cli;
use crate::Commands;

/// Install the global tracing subscriber. Returns `Ok(None)` for short
/// subcommands that exit before any heavy work — they get
/// [`Mode::Skip`] so their stdout stays clean.
pub fn install(cli: &Cli) -> Result<Option<SubscriberHandle>> {
    let opts = subscriber_opts_from_cli(cli);
    let mode = opts.mode;
    let handle = init_subscriber(opts).map_err(|e| anyhow::anyhow!("{e}"))?;
    if let Some(h) = &handle {
        emit_ready_anchor(mode, h);
    }
    Ok(handle)
}

/// Build [`SubscriberOpts`] from CLI flags + `COCO_LOG*` env vars.
/// Pure assembly — does not register the global subscriber.
pub fn subscriber_opts_from_cli(cli: &Cli) -> SubscriberOpts {
    SubscriberOpts {
        mode: detect_mode(cli),
        level: resolve_level(cli),
        format: resolve_format(cli),
        file: resolve_file(cli),
        also_stderr: cli.log_stderr || env::is_env_truthy(EnvKey::CocoLogStderr),
        default_log_dir: global_config::config_home().join("logs"),
        default_file_prefix: "coco".to_string(),
        timezone: resolve_timezone(cli),
    }
}

/// Pick the run mode by inspecting the subcommand and stdin/stdout
/// terminal state. Same branching as `main()` so subscriber sinks
/// match the eventual run path.
pub fn detect_mode(cli: &Cli) -> Mode {
    if let Some(cmd) = &cli.command {
        return match cmd {
            Commands::Chat { .. } | Commands::Review { .. } => Mode::Headless,
            Commands::Sdk => Mode::Sdk,
            // Every other subcommand prints a short result and exits;
            // installing a subscriber would only add log noise.
            _ => Mode::Skip,
        };
    }

    let is_piped = !std::io::stdout().is_terminal();
    if cli.prompt.is_some() || is_piped {
        Mode::Headless
    } else {
        Mode::Tui
    }
}

fn resolve_level(cli: &Cli) -> Option<String> {
    if let Some(raw) = cli.log_level.as_deref() {
        return Some(expand_bare_level(raw));
    }
    if let Some(directive) = env::env_opt(EnvKey::CocoLog) {
        return Some(directive);
    }
    // RUST_LOG is the tracing-ecosystem convention name; reading it
    // directly is the established pattern in retrieval/src/bin/.
    if let Some(directive) = std::env::var("RUST_LOG").ok().filter(|s| !s.is_empty()) {
        return Some(directive);
    }
    None
}

fn resolve_format(cli: &Cli) -> Option<Format> {
    cli.log_format
        .as_deref()
        .and_then(Format::parse)
        .or_else(|| env::env_opt(EnvKey::CocoLogFormat).and_then(|s| Format::parse(&s)))
}

fn resolve_file(cli: &Cli) -> Option<PathBuf> {
    cli.log_file
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| env::env_opt(EnvKey::CocoLogFile).map(PathBuf::from))
}

/// Resolve the timezone for log timestamps. `--log-timezone` wins
/// over `COCO_LOG_TIMEZONE`; both accept `local | utc`
/// (case-insensitive). Unknown values fall through to
/// [`TimezoneConfig::default`] (`Local`).
fn resolve_timezone(cli: &Cli) -> TimezoneConfig {
    cli.log_timezone
        .as_deref()
        .and_then(parse_timezone)
        .or_else(|| {
            env::env_opt(EnvKey::CocoLogTimezone)
                .as_deref()
                .and_then(parse_timezone)
        })
        .unwrap_or_default()
}

fn parse_timezone(raw: &str) -> Option<TimezoneConfig> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "local" => Some(TimezoneConfig::Local),
        "utc" => Some(TimezoneConfig::Utc),
        _ => None,
    }
}

/// Bare levels (`debug`, `info`, …) expand to `coco=<lvl>,<lvl>` so
/// coco crates stay verbose without spamming third-party output.
/// Anything else is passed through as a full `EnvFilter` directive.
fn expand_bare_level(raw: &str) -> String {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "off" | "error" | "warn" | "info" | "debug" | "trace"
    ) {
        format!("coco={lower},{lower}")
    } else {
        raw.to_string()
    }
}

fn emit_ready_anchor(mode: Mode, handle: &SubscriberHandle) {
    match handle.log_path.as_ref() {
        Some(path) => tracing::info!(
            target: "coco_cli::startup",
            coco_version = env!("CARGO_PKG_VERSION"),
            mode = mode.as_str(),
            log_filter = %handle.effective_filter,
            log_format = handle.effective_format.as_str(),
            log_file = %path.display(),
            "subscriber ready"
        ),
        None => tracing::info!(
            target: "coco_cli::startup",
            coco_version = env!("CARGO_PKG_VERSION"),
            mode = mode.as_str(),
            log_filter = %handle.effective_filter,
            log_format = handle.effective_format.as_str(),
            "subscriber ready (file sink disabled)"
        ),
    }
}

#[cfg(test)]
#[path = "tracing_init.test.rs"]
mod tests;
