# coco-otel

OpenTelemetry tracing and metrics. HYBRID: cocode-rs L0-L1 base + TS L2-L5 enhancements.

## Source
Base copied from cocode-rs `common/otel`. Needs TS enhancements:
- L2: Span hierarchy (interaction → llm_request → tool → hook → user_input) from `src/utils/telemetry/sessionTracing.ts`
- L3: ~53 application events from `src/services/analytics/` (~4K LOC)
- L4: Business metrics (token/cost/LOC/session/active_time/PR/commit)
- L5: Custom exporters (BigQuery, 1P Event Logging, Perfetto, Beta tracing)
- L6: Operational control (sampling, killswitch) — deferred

## TS Source (for L2-L5 enhancements)
- `src/services/analytics/` (8 files, ~4K LOC)
- `src/utils/telemetry/` (9 files, ~4K LOC)
- `src/utils/debug.ts`
- `src/services/internalLogging.ts`
- `src/services/toolUseSummary/` (v2)

## Key Types
OtelManager, OtelProvider, MetricsClient, Timer, OtelSettings

## Dependencies
- coco-utils-absolute-path (for TLS cert paths)
- External: opentelemetry, tracing, thiserror, reqwest

Note: Uses thiserror (not snafu) for errors. Plan says common/ should use snafu — migrate when integrating L2-L5 TS enhancements.
