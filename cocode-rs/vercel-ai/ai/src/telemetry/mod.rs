//! Telemetry module (stub implementation).
//!
//! This module provides type definitions for telemetry configuration.
//! The actual OpenTelemetry integration is not implemented; this is a stub
//! that allows code to compile with telemetry types.

mod telemetry_settings;

pub use telemetry_settings::TelemetrySettings;

/// Get a no-op tracer.
///
/// This returns a unit value since we don't have actual OpenTelemetry integration.
pub fn get_tracer(_settings: Option<&TelemetrySettings>) {}

/// Record a span (no-op).
///
/// This is a stub that simply executes the provided function without tracing.
pub async fn record_span<T, F, Fut>(_name: &str, _tracer: (), f: F) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    f().await
}
