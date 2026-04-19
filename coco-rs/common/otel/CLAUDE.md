# coco-otel

OpenTelemetry tracing and metrics: OTLP gRPC/HTTP export, base events, metrics client with RAII timers.

## Key Types

- `OtelManager` — top-level handle with metadata, session span, optional `MetricsClient`
- `OtelEventMetadata` — conversation_id, provider, model, auth_mode, app_version, terminal_type
- `OtelProvider` — composed logger / tracer / metrics exporters
- `MetricsClient`, `MetricsConfig`, `MetricsError`, `Timer` (RAII duration recording)
- `ToolDecisionSource` (Config / User)
- Modules: `config` (`OtelSettings`, `OtelExporter`, `OtelTlsConfig`), `otel_provider`, `events`, `traces`, `metrics`, `otlp` (exporter builders)

## Current Scope

- **Shipped**: export pipeline (OTLP gRPC/HTTP, TLS, Statsig, InMemory) + 7 base events (conversation, prompt, tool_decision, tool_result, api_request, sse_event, completion).
- **Not yet shipped**: higher-layer span hierarchy, business metrics, custom exporters (BigQuery / Perfetto / first-party event logger), operational controls (sampling / killswitch / PII safety / opt-out). See `docs/coco-rs/crate-coco-otel.md` for the full roadmap and TS reference-points when these land.

## Conventions

- Uses `thiserror` (not snafu).
- `Timer` auto-records duration on Drop (safer than manual start / end).
- Tag keys / values validated via `validate_tag_key` / `validate_tag_value` before emission.
- `metrics_use_metadata_tags` flag controls whether provider / model / auth_mode are auto-tagged on every metric.
