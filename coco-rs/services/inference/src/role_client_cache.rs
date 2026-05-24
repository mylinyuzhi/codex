//! Per-role `ApiClient` cache, shared across `SessionRuntime` and
//! ancillary subsystems (hook LLM, fork dispatcher, side query …).
//!
//! Why this lives here: building an `ApiClient` is `coco-inference`'s
//! responsibility (`model_factory::build_api_client`), and a cache that
//! holds one client per `ModelRole` is the natural complement. Putting
//! it in this crate lets `SessionRuntime` and any other consumer share
//! one cache `Arc`, so cache-break detector state stays consistent for
//! a given role regardless of which subsystem dispatched the call.
//!
//! ## Semantics
//!
//! - `Main` always resolves through `main_client` (no map lookup, no
//!   build). The cache map never holds `Main` — the field is the
//!   single source of truth.
//! - Other roles: spec is read off `runtime_config.model_roles`. When
//!   it equals Main's spec (the common case for unconfigured roles
//!   where `resolve_model_roles` inserts Main's fallback), the
//!   resolution short-circuits to `main_client.clone()` so the
//!   detector keeps a continuous baseline across plan-mode swaps.
//! - Otherwise `build_api_client` is invoked once and the result is
//!   memoised. Concurrent first-callers race on the write lock; the
//!   loser observes the winner's `Arc` via lost-update protection.
//!
//! ## Hot-reload (known gap)
//!
//! `runtime_config` is a snapshot. A `RuntimeReloader` config swap is
//! NOT propagated — users who edit `models.<role>` mid-session pay
//! for the change only after restart. This matches the pre-cache
//! behaviour; lifting the limitation is tracked as a follow-up
//! (subscribe to `RuntimeReloader::subscribe_changes` and drop the
//! cache when the relevant spec changes).

use std::collections::HashMap;
use std::sync::Arc;

use coco_config::RuntimeConfig;
use coco_types::ModelRole;
use tokio::sync::RwLock;

use crate::ApiClient;
use crate::InferenceError;
use crate::errors::inference_error::ModelRoleUnresolvedSnafu;
use crate::model_factory::build_api_client;

/// Lazy per-role `ApiClient` cache.
///
/// Hold `Arc<RoleClientCache>` from multiple subsystems to share one
/// `ApiClient` per role. Cheap to clone; the `RwLock` is contended
/// only on cold-path first builds.
pub struct RoleClientCache {
    runtime_config: Arc<RuntimeConfig>,
    main_client: Arc<ApiClient>,
    cache: RwLock<HashMap<ModelRole, Arc<ApiClient>>>,
}

impl RoleClientCache {
    /// Build an empty cache. `main_client` is captured as the
    /// canonical `ModelRole::Main` client; other roles are resolved
    /// lazily.
    pub fn new(runtime_config: Arc<RuntimeConfig>, main_client: Arc<ApiClient>) -> Self {
        Self {
            runtime_config,
            main_client,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Borrow the canonical Main client. Useful when a caller needs a
    /// guaranteed-resolved fallback without touching the cache.
    pub fn main_client(&self) -> Arc<ApiClient> {
        self.main_client.clone()
    }

    /// Resolve an `ApiClient` for `role`. See module doc for
    /// semantics. The only failure mode is "role is not configured in
    /// `RuntimeConfig.model_roles` and no Main fallback was wired" —
    /// `resolve_model_roles` is supposed to prevent this, so an `Err`
    /// here means the runtime config was constructed by a path that
    /// bypassed the normal layering.
    pub async fn resolve(&self, role: ModelRole) -> Result<Arc<ApiClient>, InferenceError> {
        {
            let g = self.cache.read().await;
            if let Some(c) = g.get(&role) {
                return Ok(c.clone());
            }
        }
        if role == ModelRole::Main {
            return Ok(self.main_client.clone());
        }
        let spec = self
            .runtime_config
            .model_roles
            .get(role)
            .cloned()
            .ok_or_else(|| {
                ModelRoleUnresolvedSnafu {
                    role: format!("{role:?}"),
                }
                .build()
            })?;
        if let Some(main_spec) = self.runtime_config.model_roles.get(ModelRole::Main)
            && spec == *main_spec
        {
            let mut g = self.cache.write().await;
            g.entry(role).or_insert_with(|| self.main_client.clone());
            return Ok(self.main_client.clone());
        }
        let retry: crate::RetryConfig = self.runtime_config.api.retry.clone().into();
        let built = build_api_client(&self.runtime_config, &spec, retry)?;
        let mut g = self.cache.write().await;
        if let Some(existing) = g.get(&role) {
            return Ok(existing.clone());
        }
        g.insert(role, built.clone());
        Ok(built)
    }
}

// No unit tests in this file. Constructing a `RoleClientCache` requires
// a real `RuntimeConfig` (50+ resolved fields, registry validation
// against a non-empty models catalog) plus a working `ApiClient` (real
// `LanguageModelV4` impl) — heavy mock infrastructure for the surface
// area here. The Main shortcut, spec-equality reuse, and build path
// are all exercised end-to-end by `app/cli::session_runtime::SessionRuntime`
// integration tests that go through `client_for_role`.
