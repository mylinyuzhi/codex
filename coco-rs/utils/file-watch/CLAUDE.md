# coco-file-watch

Throttled, event-coalescing file watcher over the `notify` crate.

## Key Types
| Type | Purpose |
|------|---------|
| `FileWatcher<E>` | Generic watcher emitting typed domain events `E` via `broadcast::Sender` |
| `FileWatcherBuilder<E>` | Configures `throttle_interval` (default 1s) + `channel_capacity` (default 128); `build(classify, merge)` or `build_noop` |
| `ThrottledPaths` | Stand-alone coalescer: accumulate paths, emit at most once per interval |
| `RecursiveMode` | Re-exported from `notify` |

`classify: Fn(&notify::Event) -> Option<E>` maps raw events to domain events; `merge: Fn(E, E) -> E` coalesces bursts during the throttle window.
