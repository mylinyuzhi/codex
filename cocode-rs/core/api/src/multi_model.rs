//! Multi-model container for role-based model resolution.
//!
//! The [`MultiModel`] struct provides:
//! - Role-based lookup with fallback to Main
//! - Thread-safe caching via `Mutex`
//! - Lazy model creation via [`ModelFactory`] trait
//! - Cache invalidation when selections change
//! - Shareable via `Arc<MultiModel>`
//!
//! # Migration Note
//!
//! For new code, prefer using [`ModelResolver`](crate::ModelResolver) directly,
//! which takes selections as a parameter instead of storing them internally.
//! This makes `SessionState.current_selections` the single source of truth.
//!
//! # Example
//!
//! ```ignore
//! use cocode_api::multi_model::{MultiModel, ModelFactory};
//! use cocode_protocol::model::ModelRole;
//!
//! // Create with a factory
//! let multi_model = MultiModel::new(factory);
//!
//! // Set selections
//! multi_model.set_selection(ModelRole::Main, "anthropic/claude-opus-4");
//! multi_model.set_selection(ModelRole::Fast, "anthropic/claude-haiku");
//!
//! // Get model (creates on first access, caches)
//! let model = multi_model.get(ModelRole::Main)?;
//!
//! // Unset roles fall back to Main
//! let compact_model = multi_model.get(ModelRole::Compact)?;
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use cocode_protocol::ProviderType;
use cocode_protocol::model::ModelRole;
use cocode_protocol::model::ModelSpec;
use hyper_sdk::Model;
use tracing::debug;
use tracing::info;

use crate::model_resolver::ModelResolver;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur when working with MultiModel.
#[derive(Debug, thiserror::Error)]
pub enum MultiModelError {
    /// No model is configured for the requested role or the Main fallback.
    #[error("No model configured for role {role} or Main")]
    NoModelConfigured { role: ModelRole },

    /// The model factory failed to create a model.
    #[error("Failed to create model for role {role}: {source}")]
    FactoryError {
        role: ModelRole,
        #[source]
        source: anyhow::Error,
    },

    /// Internal lock was poisoned (should not happen in normal operation).
    #[error("Internal lock poisoned")]
    LockPoisoned,
}

impl MultiModelError {
    /// Check if this error indicates no model is configured.
    pub fn is_no_model_configured(&self) -> bool {
        matches!(self, Self::NoModelConfigured { .. })
    }
}

impl From<crate::model_resolver::ResolverError> for MultiModelError {
    fn from(err: crate::model_resolver::ResolverError) -> Self {
        use crate::model_resolver::ResolverError;
        match err {
            ResolverError::NoModelConfigured { role } => Self::NoModelConfigured { role },
            ResolverError::LockPoisoned => Self::LockPoisoned,
            other => Self::FactoryError {
                role: ModelRole::Main, // Default role for conversion
                source: anyhow::anyhow!("{}", other),
            },
        }
    }
}

// ============================================================================
// Factory Trait
// ============================================================================

/// Factory trait for model creation.
///
/// Implementations handle the actual creation of model instances from
/// selection keys (e.g., "anthropic/claude-opus-4").
pub trait ModelFactory: Send + Sync {
    /// Create a model from a selection key.
    ///
    /// # Arguments
    ///
    /// * `role` - The role requesting the model (for logging/context)
    /// * `key` - The selection key (e.g., "anthropic/claude-opus-4")
    ///
    /// # Returns
    ///
    /// A tuple of (model, provider_type) on success.
    fn create(&self, role: ModelRole, key: &str) -> anyhow::Result<(Arc<dyn Model>, ProviderType)>;
}

// ============================================================================
// MultiModel
// ============================================================================

/// Multi-model container for role-based model resolution.
///
/// Provides thread-safe caching and lazy creation of model instances
/// per role. When a role is not configured, it falls back to the Main role.
///
/// # Cache Semantics
///
/// - When a role has an explicit selection, a model is cached under that role.
/// - When a role falls back to Main, the Main cache entry is reused (no duplicate).
/// - Cache entries are keyed by (role, selection_key) for staleness detection.
pub struct MultiModel {
    inner: Mutex<MultiModelInner>,
    factory: Arc<dyn ModelFactory>,
}

struct MultiModelInner {
    /// Selection keys per role (e.g., "anthropic/claude-opus-4").
    selections: HashMap<ModelRole, String>,

    /// Cached model instances keyed by effective role.
    /// When role X falls back to Main, the Main entry is used directly.
    cache: HashMap<ModelRole, CachedEntry>,
}

struct CachedEntry {
    model: Arc<dyn Model>,
    provider_type: ProviderType,
    /// Key used to create this model (for cache invalidation).
    key: String,
}

impl MultiModel {
    /// Create a new MultiModel with the given factory.
    pub fn new(factory: impl ModelFactory + 'static) -> Self {
        Self {
            inner: Mutex::new(MultiModelInner {
                selections: HashMap::new(),
                cache: HashMap::new(),
            }),
            factory: Arc::new(factory),
        }
    }

    /// Create a MultiModel backed by a ModelResolver.
    ///
    /// This constructor uses `ModelResolver` for model creation, which provides
    /// provider caching and uses the selections passed via `set_selection()`.
    ///
    /// # Note
    ///
    /// For new code, consider using `ModelResolver` directly with
    /// `get_for_role(role, &selections)` to avoid dual selection storage.
    pub fn with_resolver(resolver: Arc<ModelResolver>) -> Self {
        let factory = ResolverModelFactory { resolver };
        Self::new(factory)
    }

    /// Create a MultiModel that always returns the same model.
    ///
    /// This is useful for backward compatibility when only a single model
    /// is needed.
    pub fn single(model: Arc<dyn Model>, provider_type: ProviderType) -> Self {
        let factory = SingleModelFactory::new(model.clone(), provider_type);
        let multi = Self::new(factory);

        // Pre-cache the main model
        {
            let mut inner = multi.inner.lock().expect("lock poisoned");
            inner
                .selections
                .insert(ModelRole::Main, "single".to_string());
            inner.cache.insert(
                ModelRole::Main,
                CachedEntry {
                    model,
                    provider_type,
                    key: "single".to_string(),
                },
            );
        }

        multi
    }

    /// Set selection for a role.
    ///
    /// If the key differs from the current selection, the cache for this role
    /// is automatically invalidated.
    pub fn set_selection(&self, role: ModelRole, key: impl Into<String>) {
        let key = key.into();
        let mut inner = self.inner.lock().expect("lock poisoned");

        // Check if key changed
        let changed = inner
            .selections
            .get(&role)
            .map(|k| k != &key)
            .unwrap_or(true);

        if changed {
            debug!(role = %role, key = %key, "Selection changed, invalidating cache");
            inner.cache.remove(&role);
        }

        inner.selections.insert(role, key);
    }

    /// Get the selection key for a role.
    ///
    /// Falls back to Main if the role is not configured.
    pub fn get_selection(&self, role: ModelRole) -> Option<String> {
        let inner = self.inner.lock().expect("lock poisoned");
        inner
            .selections
            .get(&role)
            .or_else(|| inner.selections.get(&ModelRole::Main))
            .cloned()
    }

    /// Get model for a role (creates on first access, caches).
    ///
    /// If the role has no selection, falls back to Main. Returns an error
    /// if neither the role nor Main has a selection.
    pub fn get(&self, role: ModelRole) -> Result<Arc<dyn Model>, MultiModelError> {
        self.get_with_provider(role).map(|(m, _)| m)
    }

    /// Get model with provider type for a role.
    ///
    /// If the role has no selection, falls back to Main. Returns an error
    /// if neither the role nor Main has a selection.
    ///
    /// # Cache Behavior
    ///
    /// When a role is not explicitly configured, it falls back to Main.
    /// In this case, the Main cache entry is reused directly instead of
    /// creating a duplicate entry for the unconfigured role.
    pub fn get_with_provider(
        &self,
        role: ModelRole,
    ) -> Result<(Arc<dyn Model>, ProviderType), MultiModelError> {
        // Phase 1: Quick check under lock
        let (effective_role, key) = {
            let inner = self
                .inner
                .lock()
                .map_err(|_| MultiModelError::LockPoisoned)?;

            // Determine effective role (with fallback to Main)
            let effective_role = if inner.selections.contains_key(&role) {
                role
            } else {
                ModelRole::Main
            };

            // Check cache for effective role
            if let Some(entry) = inner.cache.get(&effective_role) {
                if let Some(expected_key) = inner.selections.get(&effective_role) {
                    if entry.key == *expected_key {
                        debug!(
                            role = %role,
                            effective_role = %effective_role,
                            "Cache hit"
                        );
                        return Ok((entry.model.clone(), entry.provider_type));
                    }
                }
            }

            // Get selection key for creation
            let key = inner
                .selections
                .get(&effective_role)
                .cloned()
                .ok_or(MultiModelError::NoModelConfigured { role })?;

            (effective_role, key)
        }; // Lock released here

        // Phase 2: Create model outside lock (potentially slow operation)
        info!(
            role = %role,
            effective_role = %effective_role,
            key = %key,
            "Creating model for role"
        );
        let (model, provider_type) = self.factory.create(effective_role, &key).map_err(|e| {
            MultiModelError::FactoryError {
                role: effective_role,
                source: e,
            }
        })?;

        // Phase 3: Double-check and store under lock
        {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| MultiModelError::LockPoisoned)?;

            // Double-check: another thread might have created the model
            if let Some(entry) = inner.cache.get(&effective_role) {
                if let Some(expected_key) = inner.selections.get(&effective_role) {
                    if entry.key == *expected_key {
                        debug!(
                            role = %role,
                            effective_role = %effective_role,
                            "Another thread created the model, using existing"
                        );
                        return Ok((entry.model.clone(), entry.provider_type));
                    }
                }
            }

            // Store in cache under effective role
            inner.cache.insert(
                effective_role,
                CachedEntry {
                    model: model.clone(),
                    provider_type,
                    key,
                },
            );
        }

        Ok((model, provider_type))
    }

    /// Shorthand for `get(ModelRole::Main)`.
    pub fn main(&self) -> Result<Arc<dyn Model>, MultiModelError> {
        self.get(ModelRole::Main)
    }

    /// Invalidate cache for a role.
    pub fn invalidate(&self, role: ModelRole) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.cache.remove(&role).is_some() {
                debug!(role = %role, "Invalidated cached model");
            }
        }
    }

    /// Invalidate all cached models.
    pub fn invalidate_all(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.cache.clear();
            debug!("Invalidated all cached models");
        }
    }

    /// Check if a model is cached for a role.
    pub fn is_cached(&self, role: ModelRole) -> bool {
        self.inner
            .lock()
            .map(|inner| inner.cache.contains_key(&role))
            .unwrap_or(false)
    }

    /// Get the number of cached models.
    pub fn cache_size(&self) -> usize {
        self.inner
            .lock()
            .map(|inner| inner.cache.len())
            .unwrap_or(0)
    }

    /// Get all configured roles.
    pub fn configured_roles(&self) -> Vec<ModelRole> {
        self.inner
            .lock()
            .map(|inner| inner.selections.keys().copied().collect())
            .unwrap_or_default()
    }
}

impl std::fmt::Debug for MultiModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Use try_lock to avoid potential deadlock in debug output
        match self.inner.try_lock() {
            Ok(inner) => f
                .debug_struct("MultiModel")
                .field("selections", &inner.selections)
                .field("cache_size", &inner.cache.len())
                .finish(),
            Err(_) => f
                .debug_struct("MultiModel")
                .field("state", &"<locked>")
                .finish(),
        }
    }
}

// ============================================================================
// SingleModelFactory
// ============================================================================

/// Simple factory that always returns the same model.
///
/// Used for backward compatibility when only a single model is needed.
pub struct SingleModelFactory {
    model: Arc<dyn Model>,
    provider_type: ProviderType,
}

impl SingleModelFactory {
    /// Create a new SingleModelFactory.
    pub fn new(model: Arc<dyn Model>, provider_type: ProviderType) -> Self {
        Self {
            model,
            provider_type,
        }
    }
}

impl ModelFactory for SingleModelFactory {
    fn create(
        &self,
        _role: ModelRole,
        _key: &str,
    ) -> anyhow::Result<(Arc<dyn Model>, ProviderType)> {
        Ok((self.model.clone(), self.provider_type))
    }
}

// ============================================================================
// ResolverModelFactory
// ============================================================================

/// Factory that delegates to a [`ModelResolver`].
///
/// This bridges the `ModelFactory` trait with the `ModelResolver` implementation,
/// allowing `MultiModel` to use `ModelResolver` for provider and model caching.
///
/// The key is parsed as "provider/model" format and converted to a `ModelSpec`.
struct ResolverModelFactory {
    resolver: Arc<ModelResolver>,
}

impl ModelFactory for ResolverModelFactory {
    fn create(
        &self,
        _role: ModelRole,
        key: &str,
    ) -> anyhow::Result<(Arc<dyn Model>, ProviderType)> {
        // Parse key as "provider/model"
        let spec = parse_model_key(key)?;
        self.resolver
            .get(&spec)
            .map_err(|e| anyhow::anyhow!("{}", e))
    }
}

/// Parse a selection key into a ModelSpec.
///
/// Keys are in the format "provider/model" (e.g., "anthropic/claude-opus-4").
fn parse_model_key(key: &str) -> anyhow::Result<ModelSpec> {
    let parts: Vec<&str> = key.splitn(2, '/').collect();
    if parts.len() != 2 {
        anyhow::bail!(
            "Invalid selection key '{}': expected format 'provider/model'",
            key
        );
    }
    Ok(ModelSpec::new(parts[0], parts[1]))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use hyper_sdk::GenerateRequest;
    use hyper_sdk::GenerateResponse;
    use hyper_sdk::HyperError;
    use std::sync::Arc;
    use std::sync::atomic::AtomicI32;
    use std::sync::atomic::Ordering;
    use std::thread;

    /// Mock model for testing.
    #[derive(Debug)]
    struct MockModel {
        name: String,
    }

    #[async_trait]
    impl Model for MockModel {
        fn model_name(&self) -> &str {
            &self.name
        }

        fn provider(&self) -> &str {
            "mock"
        }

        async fn generate(
            &self,
            _request: GenerateRequest,
        ) -> Result<GenerateResponse, HyperError> {
            Err(HyperError::UnsupportedCapability("mock".to_string()))
        }
    }

    /// Mock factory for testing that tracks call count.
    struct MockFactory {
        call_count: AtomicI32,
    }

    impl MockFactory {
        fn new() -> Self {
            Self {
                call_count: AtomicI32::new(0),
            }
        }

        fn calls(&self) -> i32 {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    impl ModelFactory for MockFactory {
        fn create(
            &self,
            role: ModelRole,
            key: &str,
        ) -> anyhow::Result<(Arc<dyn Model>, ProviderType)> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let model = Arc::new(MockModel {
                name: format!("{}-{}", role, key),
            });
            Ok((model, ProviderType::Openai))
        }
    }

    #[test]
    fn test_multi_model_basic() {
        let factory = MockFactory::new();
        let multi = MultiModel::new(factory);

        multi.set_selection(ModelRole::Main, "anthropic/claude-opus-4");

        let model = multi.get(ModelRole::Main).unwrap();
        assert_eq!(model.model_name(), "main-anthropic/claude-opus-4");
    }

    #[test]
    fn test_multi_model_caching() {
        let factory = MockFactory::new();
        let multi = MultiModel::new(factory);

        multi.set_selection(ModelRole::Main, "anthropic/claude-opus-4");

        // First call creates model
        let _ = multi.get(ModelRole::Main).unwrap();
        assert!(multi.is_cached(ModelRole::Main));

        // Second call uses cache - factory should only be called once
        let _ = multi.get(ModelRole::Main).unwrap();
        assert_eq!(multi.cache_size(), 1);
    }

    #[test]
    fn test_multi_model_fallback_to_main() {
        // Use Arc to keep a reference after moving to MultiModel
        let factory = Arc::new(MockFactory::new());
        let wrapper = ArcMockFactory {
            inner: factory.clone(),
        };
        let multi = MultiModel::new(wrapper);

        // Only set Main
        multi.set_selection(ModelRole::Main, "anthropic/claude-opus-4");

        // First, get Main to cache it
        let main_model = multi.get(ModelRole::Main).unwrap();
        assert_eq!(factory.calls(), 1);

        // Fast should fall back to Main and reuse the same cache entry
        let fast_model = multi.get(ModelRole::Fast).unwrap();
        assert_eq!(factory.calls(), 1); // No new factory call!

        // Both should be the same model
        assert_eq!(main_model.model_name(), fast_model.model_name());

        // Only one cache entry (Main)
        assert_eq!(multi.cache_size(), 1);
        assert!(multi.is_cached(ModelRole::Main));
        assert!(!multi.is_cached(ModelRole::Fast)); // Fast is not cached separately
    }

    /// A factory wrapper that delegates to an Arc<MockFactory>.
    struct ArcMockFactory {
        inner: Arc<MockFactory>,
    }

    impl ModelFactory for ArcMockFactory {
        fn create(
            &self,
            role: ModelRole,
            key: &str,
        ) -> anyhow::Result<(Arc<dyn Model>, ProviderType)> {
            self.inner.create(role, key)
        }
    }

    #[test]
    fn test_multi_model_specific_role() {
        let factory = MockFactory::new();
        let multi = MultiModel::new(factory);

        multi.set_selection(ModelRole::Main, "anthropic/claude-opus-4");
        multi.set_selection(ModelRole::Fast, "anthropic/claude-haiku");

        let main = multi.get(ModelRole::Main).unwrap();
        let fast = multi.get(ModelRole::Fast).unwrap();

        assert_eq!(main.model_name(), "main-anthropic/claude-opus-4");
        assert_eq!(fast.model_name(), "fast-anthropic/claude-haiku");

        // Two separate cache entries
        assert_eq!(multi.cache_size(), 2);
    }

    #[test]
    fn test_multi_model_invalidate() {
        let factory = MockFactory::new();
        let multi = MultiModel::new(factory);

        multi.set_selection(ModelRole::Main, "anthropic/claude-opus-4");

        // Create and cache
        let _ = multi.get(ModelRole::Main).unwrap();
        assert!(multi.is_cached(ModelRole::Main));

        // Invalidate
        multi.invalidate(ModelRole::Main);
        assert!(!multi.is_cached(ModelRole::Main));
    }

    #[test]
    fn test_multi_model_selection_change_invalidates() {
        let factory = MockFactory::new();
        let multi = MultiModel::new(factory);

        multi.set_selection(ModelRole::Main, "anthropic/claude-opus-4");

        // Create and cache
        let _ = multi.get(ModelRole::Main).unwrap();
        assert!(multi.is_cached(ModelRole::Main));

        // Change selection (should invalidate)
        multi.set_selection(ModelRole::Main, "openai/gpt-5");
        assert!(!multi.is_cached(ModelRole::Main));

        // Get again (should create new model)
        let model = multi.get(ModelRole::Main).unwrap();
        assert_eq!(model.model_name(), "main-openai/gpt-5");
    }

    #[test]
    fn test_multi_model_no_selection_error() {
        let factory = MockFactory::new();
        let multi = MultiModel::new(factory);

        // No selection set
        let result = multi.get(ModelRole::Main);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.is_no_model_configured());
        assert!(err.to_string().contains("No model configured"));
    }

    #[test]
    fn test_single_model_factory() {
        let mock = Arc::new(MockModel {
            name: "test-model".to_string(),
        });
        let multi = MultiModel::single(mock, ProviderType::Anthropic);

        // Should return the same model for all roles
        let main = multi.get(ModelRole::Main).unwrap();
        let fast = multi.get(ModelRole::Fast).unwrap();

        assert_eq!(main.model_name(), "test-model");
        assert_eq!(fast.model_name(), "test-model");
    }

    #[test]
    fn test_configured_roles() {
        let factory = MockFactory::new();
        let multi = MultiModel::new(factory);

        multi.set_selection(ModelRole::Main, "m1");
        multi.set_selection(ModelRole::Fast, "m2");
        multi.set_selection(ModelRole::Compact, "m3");

        let roles = multi.configured_roles();
        assert_eq!(roles.len(), 3);
        assert!(roles.contains(&ModelRole::Main));
        assert!(roles.contains(&ModelRole::Fast));
        assert!(roles.contains(&ModelRole::Compact));
    }

    #[test]
    fn test_get_selection() {
        let factory = MockFactory::new();
        let multi = MultiModel::new(factory);

        multi.set_selection(ModelRole::Main, "main-model");
        multi.set_selection(ModelRole::Fast, "fast-model");

        assert_eq!(
            multi.get_selection(ModelRole::Main),
            Some("main-model".to_string())
        );
        assert_eq!(
            multi.get_selection(ModelRole::Fast),
            Some("fast-model".to_string())
        );

        // Unconfigured role should fall back to Main
        assert_eq!(
            multi.get_selection(ModelRole::Vision),
            Some("main-model".to_string())
        );
    }

    #[test]
    fn test_error_types() {
        let err = MultiModelError::NoModelConfigured {
            role: ModelRole::Fast,
        };
        assert!(err.is_no_model_configured());

        let err = MultiModelError::FactoryError {
            role: ModelRole::Main,
            source: anyhow::anyhow!("test error"),
        };
        assert!(!err.is_no_model_configured());
        assert!(err.to_string().contains("test error"));
    }

    #[test]
    fn test_concurrent_access() {
        let factory = Arc::new(MockFactory::new());
        let slow_factory = SlowMockFactory {
            inner: factory.clone(),
        };
        let multi = Arc::new(MultiModel::new(slow_factory));

        multi.set_selection(ModelRole::Main, "test-model");

        // Spawn multiple threads trying to get the model simultaneously
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let multi = multi.clone();
                thread::spawn(move || multi.get(ModelRole::Main).unwrap())
            })
            .collect();

        // Wait for all threads
        for handle in handles {
            let model = handle.join().unwrap();
            assert_eq!(model.model_name(), "main-test-model");
        }

        // Due to the double-checked locking pattern, the factory might be called
        // multiple times during the initial race (before any thread caches).
        // The key property is that the cache works after the initial contention,
        // and subsequent calls reuse the cached entry.
        // We just verify the cache now contains exactly one entry.
        assert_eq!(multi.cache_size(), 1, "Cache should have exactly one entry");

        // Verify subsequent calls use the cache (no more factory calls)
        let calls_before = factory.calls();
        let _ = multi.get(ModelRole::Main).unwrap();
        let calls_after = factory.calls();
        assert_eq!(
            calls_before, calls_after,
            "Factory should not be called again after caching"
        );
    }

    /// A factory that simulates slow model creation for concurrent testing.
    struct SlowMockFactory {
        inner: Arc<MockFactory>,
    }

    impl ModelFactory for SlowMockFactory {
        fn create(
            &self,
            role: ModelRole,
            key: &str,
        ) -> anyhow::Result<(Arc<dyn Model>, ProviderType)> {
            // Simulate slow creation
            thread::sleep(std::time::Duration::from_millis(10));
            self.inner.create(role, key)
        }
    }
}
