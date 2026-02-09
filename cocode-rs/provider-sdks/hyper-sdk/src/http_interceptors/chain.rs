//! HTTP interceptor chain for applying multiple interceptors in priority order.

use super::HttpInterceptor;
use super::HttpInterceptorContext;
use super::HttpRequest;
use std::sync::Arc;

/// Priority-ordered chain of HTTP interceptors.
///
/// Interceptors are applied in order of their priority (lower = earlier).
/// This allows composing multiple interceptors that each handle a specific concern.
///
/// # Example
///
/// ```no_run
/// use hyper_sdk::http_interceptors::{HttpInterceptorChain, BytedModelHubInterceptor};
/// use std::sync::Arc;
///
/// let mut chain = HttpInterceptorChain::new();
/// chain.add(Arc::new(BytedModelHubInterceptor));
/// ```
#[derive(Debug, Default, Clone)]
pub struct HttpInterceptorChain {
    interceptors: Vec<Arc<dyn HttpInterceptor>>,
    /// Whether the interceptors are sorted by priority.
    sorted: bool,
}

impl HttpInterceptorChain {
    /// Create a new empty interceptor chain.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an interceptor to the chain.
    ///
    /// Interceptors are automatically sorted by priority when applied.
    /// Returns `&mut Self` for chaining.
    pub fn add(&mut self, interceptor: Arc<dyn HttpInterceptor>) -> &mut Self {
        self.interceptors.push(interceptor);
        self.sorted = false;
        self
    }

    /// Add multiple interceptors to the chain.
    pub fn add_all(
        &mut self,
        interceptors: impl IntoIterator<Item = Arc<dyn HttpInterceptor>>,
    ) -> &mut Self {
        self.interceptors.extend(interceptors);
        self.sorted = false;
        self
    }

    /// Ensure interceptors are sorted by priority.
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
    ///
    /// Interceptors with lower priority values are applied first.
    /// The chain is sorted lazily on first apply for efficiency.
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

#[cfg(test)]
#[path = "chain.test.rs"]
mod tests;
