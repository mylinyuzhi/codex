# coco-utils-common

Small cross-crate helpers with zero internal deps.

## Key Modules
| Module | Exports |
|--------|---------|
| `coco_home` | `find_coco_home`, `COCODE_HOME_ENV` — resolves `~/.coco` / override |
| `elapsed` | `format_duration`, `format_elapsed` — human-readable durations |
| `format_env_display` | `format_env_display` — redacted env-var printing |
| `fuzzy_match` | `fuzzy_match`, `fuzzy_indices` — lightweight fuzzy scoring |
| `logging` | `LoggingConfig`, `TimezoneConfig`, `ConfigurableTimer`, `build_env_filter` — `tracing-subscriber` bootstrap |
