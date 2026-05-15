//! Team memory sync subsystem (HTTP-backed).
//!
//! TS: `services/teamMemorySync/{index,types,secretScanner,
//! teamMemSecretGuard,watcher}.ts`.
//!
//! HTTP push/pull pipeline + watcher live in `service.rs`. Per-agent
//! snapshot IO lives at [`coco_memory::agent_memory_snapshot`] (separate
//! module — the snapshot mechanism is per-agent, not per-team).

pub mod secret_scanner;
pub mod service;
pub mod types;
pub mod watcher;

pub use secret_scanner::scan_for_secrets;
pub use service::PushEntry;
pub use service::apply_pulled_content;
pub use service::compute_content_hash;
pub use service::endpoint;
pub use service::pull;
pub use service::push;
pub use types::SkippedSecretFile;
pub use types::SyncState;
pub use types::TeamMemoryContent;
pub use types::TeamMemoryData;
pub use types::TeamMemoryHashesResult;
pub use types::TeamMemorySyncFetchResult;
pub use types::TeamMemorySyncPushResult;
pub use types::TeamMemorySyncUploadResult;
pub use types::{MAX_FILE_SIZE_BYTES, MAX_PUT_BODY_BYTES, SYNC_TIMEOUT_MS};
pub use watcher::WatcherConfig;
pub use watcher::spawn_watcher;
