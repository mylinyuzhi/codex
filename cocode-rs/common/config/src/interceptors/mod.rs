//! HTTP interceptor registry for cocode-config.
//!
//! This module provides a global registry for HTTP interceptors, allowing
//! configuration files to reference interceptors by name.
//!
//! # Architecture
//!
//! ```text
//! providers.json: interceptors = ["byted_model_hub"]
//!   └── resolve_chain(names) → HttpInterceptorChain
//!       └── get_interceptor("byted_model_hub") → Arc<dyn HttpInterceptor>
//! ```
//!
//! # Built-in Interceptors
//!
//! - `byted_model_hub` - Adds session_id to "extra" header for ByteDance ModelHub
//!
//! # Example
//!
//! ```no_run
//! use cocode_config::interceptors::{
//!     get_interceptor, resolve_chain, list_interceptors,
//!     HttpRequest, HttpInterceptorContext,
//! };
//!
//! // Get an interceptor by name
//! if let Some(interceptor) = get_interceptor("byted_model_hub") {
//!     println!("Found: {}", interceptor.name());
//! }
//!
//! // Resolve a chain from config names
//! let mut chain = resolve_chain(&["byted_model_hub".to_string()]);
//! assert_eq!(chain.len(), 1);
//!
//! // Apply the chain to a request
//! let mut request = HttpRequest::post("https://api.example.com/v1/chat");
//! let ctx = HttpInterceptorContext::new().conversation_id("session-123");
//! chain.apply(&mut request, &ctx);
//!
//! // List all registered interceptors
//! let names = list_interceptors();
//! assert!(names.contains(&"byted_model_hub".to_string()));
//! ```

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::RwLock;

use serde_json::Value;

// ============================================================================
// HTTP Interceptor Types (defined locally, no hyper-sdk dependency)
// ============================================================================

/// HTTP request that can be modified by interceptors.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP method (GET, POST, etc.).
    pub method: http::Method,
    /// Full URL including query parameters.
    pub url: String,
    /// HTTP headers.
    pub headers: http::HeaderMap,
    /// Request body as JSON (for JSON APIs).
    pub body: Option<Value>,
}

impl HttpRequest {
    /// Create a new HTTP request with the given method and URL.
    pub fn new(method: http::Method, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: http::HeaderMap::new(),
            body: None,
        }
    }

    /// Create a POST request with the given URL.
    pub fn post(url: impl Into<String>) -> Self {
        Self::new(http::Method::POST, url)
    }

    /// Set the request body.
    pub fn with_body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    /// Add a header to the request.
    pub fn with_header(mut self, name: &'static str, value: &str) -> Self {
        if let Ok(header_value) = http::HeaderValue::from_str(value) {
            self.headers.insert(name, header_value);
        }
        self
    }
}

/// Context passed to HTTP interceptors.
#[derive(Debug, Clone, Default)]
pub struct HttpInterceptorContext {
    /// Conversation/session ID for tracking multi-turn conversations.
    pub conversation_id: Option<String>,
    /// Model being used.
    pub model: Option<String>,
    /// Provider name.
    pub provider_name: Option<String>,
    /// Unique request ID for this specific request.
    pub request_id: Option<String>,
    /// Custom metadata that can be used by interceptors.
    pub metadata: HashMap<String, Value>,
}

impl HttpInterceptorContext {
    /// Create a new empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a context with provider and model.
    pub fn with_provider(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider_name: Some(provider.into()),
            model: Some(model.into()),
            ..Default::default()
        }
    }

    /// Set the conversation ID.
    pub fn conversation_id(mut self, id: impl Into<String>) -> Self {
        self.conversation_id = Some(id.into());
        self
    }

    /// Set the request ID.
    pub fn request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    /// Set a metadata value.
    pub fn set_metadata(&mut self, key: impl Into<String>, value: Value) {
        self.metadata.insert(key.into(), value);
    }

    /// Get a metadata value.
    pub fn get_metadata(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }
}

/// Trait for HTTP request interceptors.
pub trait HttpInterceptor: Send + Sync + Debug {
    /// Unique name identifying this interceptor.
    fn name(&self) -> &str;

    /// Interceptor priority (lower = earlier execution).
    fn priority(&self) -> i32 {
        100
    }

    /// Modify the request.
    fn intercept(&self, request: &mut HttpRequest, ctx: &HttpInterceptorContext);
}

/// Priority-ordered chain of HTTP interceptors.
#[derive(Debug, Default, Clone)]
pub struct HttpInterceptorChain {
    interceptors: Vec<Arc<dyn HttpInterceptor>>,
    sorted: bool,
}

impl HttpInterceptorChain {
    /// Create a new empty interceptor chain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an interceptor to the chain.
    pub fn add(&mut self, interceptor: Arc<dyn HttpInterceptor>) -> &mut Self {
        self.interceptors.push(interceptor);
        self.sorted = false;
        self
    }

    fn ensure_sorted(&mut self) {
        if !self.sorted {
            self.interceptors.sort_by_key(|i| i.priority());
            self.sorted = true;
        }
    }

    /// Check if the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.interceptors.is_empty()
    }

    /// Get the number of interceptors in the chain.
    pub fn len(&self) -> usize {
        self.interceptors.len()
    }

    /// Apply all interceptors to the request in priority order.
    pub fn apply(&mut self, request: &mut HttpRequest, ctx: &HttpInterceptorContext) {
        self.ensure_sorted();
        for interceptor in &self.interceptors {
            interceptor.intercept(request, ctx);
        }
    }

    /// List the names of all interceptors in the chain.
    pub fn names(&self) -> Vec<&str> {
        self.interceptors.iter().map(|i| i.name()).collect()
    }
}

// ============================================================================
// Built-in Interceptors
// ============================================================================

/// Interceptor for ByteDance ModelHub session tracking.
///
/// Adds a `session_id` field to the "extra" header as JSON.
#[derive(Debug, Clone, Default)]
pub struct BytedModelHubInterceptor;

impl HttpInterceptor for BytedModelHubInterceptor {
    fn name(&self) -> &str {
        "byted_model_hub"
    }

    fn priority(&self) -> i32 {
        50
    }

    fn intercept(&self, request: &mut HttpRequest, ctx: &HttpInterceptorContext) {
        if let Some(session_id) = &ctx.conversation_id {
            let extra_json = serde_json::json!({
                "session_id": session_id
            });
            if let Ok(value) = http::HeaderValue::from_str(&extra_json.to_string()) {
                request.headers.insert("extra", value);
            }
        }
    }
}

// ============================================================================
// Global Registry
// ============================================================================

/// Thread-safe registry for HTTP interceptors.
#[derive(Debug, Default)]
struct InterceptorRegistry {
    interceptors: RwLock<HashMap<String, Arc<dyn HttpInterceptor>>>,
}

impl InterceptorRegistry {
    fn new() -> Self {
        Self {
            interceptors: RwLock::new(HashMap::new()),
        }
    }

    fn register(&self, interceptor: Arc<dyn HttpInterceptor>) {
        let name = interceptor.name().to_string();
        let mut interceptors = self.interceptors.write().unwrap_or_else(|e| {
            tracing::warn!("interceptor registry lock poisoned, recovering");
            e.into_inner()
        });
        interceptors.insert(name, interceptor);
    }

    fn get(&self, name: &str) -> Option<Arc<dyn HttpInterceptor>> {
        let interceptors = self.interceptors.read().unwrap_or_else(|e| {
            tracing::warn!("interceptor registry lock poisoned, recovering");
            e.into_inner()
        });
        interceptors.get(name).cloned()
    }

    fn list(&self) -> Vec<String> {
        let interceptors = self.interceptors.read().unwrap_or_else(|e| {
            tracing::warn!("interceptor registry lock poisoned, recovering");
            e.into_inner()
        });
        interceptors.keys().cloned().collect()
    }
}

/// Global interceptor registry with built-in interceptors pre-registered.
static INTERCEPTOR_REGISTRY: LazyLock<InterceptorRegistry> = LazyLock::new(|| {
    let registry = InterceptorRegistry::new();
    // Register built-in interceptors
    registry.register(Arc::new(BytedModelHubInterceptor));
    registry
});

/// Get an interceptor by name from the global registry.
///
/// Returns `None` if the interceptor is not found.
///
/// # Example
///
/// ```no_run
/// use cocode_config::interceptors::get_interceptor;
///
/// if let Some(interceptor) = get_interceptor("byted_model_hub") {
///     println!("Found interceptor: {}", interceptor.name());
/// }
/// ```
pub fn get_interceptor(name: &str) -> Option<Arc<dyn HttpInterceptor>> {
    INTERCEPTOR_REGISTRY.get(name)
}

/// Register a custom interceptor in the global registry.
///
/// If an interceptor with the same name already exists, it will be replaced.
///
/// # Example
///
/// ```no_run
/// use cocode_config::interceptors::{register_interceptor, HttpInterceptor, HttpInterceptorContext, HttpRequest};
/// use std::sync::Arc;
///
/// #[derive(Debug)]
/// struct MyInterceptor;
///
/// impl HttpInterceptor for MyInterceptor {
///     fn name(&self) -> &str { "my_interceptor" }
///     fn intercept(&self, _: &mut HttpRequest, _: &HttpInterceptorContext) {}
/// }
///
/// register_interceptor(Arc::new(MyInterceptor));
/// ```
pub fn register_interceptor(interceptor: Arc<dyn HttpInterceptor>) {
    INTERCEPTOR_REGISTRY.register(interceptor);
}

/// List all registered interceptor names.
///
/// # Example
///
/// ```no_run
/// use cocode_config::interceptors::list_interceptors;
///
/// let names = list_interceptors();
/// println!("Available interceptors: {:?}", names);
/// ```
pub fn list_interceptors() -> Vec<String> {
    INTERCEPTOR_REGISTRY.list()
}

/// Resolve a chain of interceptors from configuration names.
///
/// Unknown interceptor names are silently ignored (with a warning logged).
/// Returns an `HttpInterceptorChain` containing all found interceptors.
///
/// # Example
///
/// ```no_run
/// use cocode_config::interceptors::resolve_chain;
///
/// let chain = resolve_chain(&["byted_model_hub".to_string()]);
/// assert_eq!(chain.len(), 1);
/// ```
pub fn resolve_chain(names: &[String]) -> HttpInterceptorChain {
    let mut chain = HttpInterceptorChain::new();
    for name in names {
        if let Some(interceptor) = get_interceptor(name) {
            chain.add(interceptor);
        } else {
            tracing::warn!("Unknown HTTP interceptor: {name}");
        }
    }
    chain
}

/// Apply interceptors to a request.
///
/// This is a convenience function that resolves a chain and applies it
/// in a single call.
///
/// # Example
///
/// ```no_run
/// use cocode_config::interceptors::{apply_interceptors, HttpRequest, HttpInterceptorContext};
///
/// let mut request = HttpRequest::post("https://api.example.com/v1/chat");
/// let ctx = HttpInterceptorContext::new().conversation_id("session-123");
///
/// apply_interceptors(&mut request, &ctx, &["byted_model_hub".to_string()]);
/// ```
pub fn apply_interceptors(
    request: &mut HttpRequest,
    ctx: &HttpInterceptorContext,
    interceptor_names: &[String],
) {
    let mut chain = resolve_chain(interceptor_names);
    chain.apply(request, ctx);
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
