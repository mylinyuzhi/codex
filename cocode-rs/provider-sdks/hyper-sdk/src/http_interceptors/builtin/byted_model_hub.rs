//! ByteDance ModelHub interceptor.
//!
//! Adds session_id to the "extra" header as JSON for ByteDance ModelHub
//! session tracking.
//!
//! # Usage
//!
//! Configure in `providers.json`:
//! ```json
//! {
//!   "name": "byted-model-hub",
//!   "type": "openai",
//!   "base_url": "https://ark.cn-beijing.volces.com/api/v3",
//!   "interceptors": ["byted_model_hub"]
//! }
//! ```
//!
//! # Output
//!
//! When conversation_id is available, adds header:
//! ```text
//! extra: {"session_id": "<conversation_id>"}
//! ```

use crate::http_interceptors::HttpInterceptor;
use crate::http_interceptors::HttpInterceptorContext;
use crate::http_interceptors::HttpRequest;
use http::HeaderValue;

/// Interceptor for ByteDance ModelHub session tracking.
///
/// This interceptor adds a `session_id` field to the "extra" header as JSON.
/// ByteDance ModelHub uses this header for session tracking in multi-turn
/// conversations.
///
/// # Example
///
/// ```no_run
/// use hyper_sdk::http_interceptors::{
///     HttpInterceptorChain, HttpInterceptorContext, HttpRequest, BytedModelHubInterceptor
/// };
/// use std::sync::Arc;
///
/// let mut chain = HttpInterceptorChain::new();
/// chain.add(Arc::new(BytedModelHubInterceptor));
///
/// let mut request = HttpRequest::post("https://ark.cn-beijing.volces.com/api/v3/chat");
/// let ctx = HttpInterceptorContext::new().conversation_id("session-123");
///
/// chain.apply(&mut request, &ctx);
/// // Request now has header: extra: {"session_id": "session-123"}
/// ```
#[derive(Debug, Clone, Default)]
pub struct BytedModelHubInterceptor;

impl BytedModelHubInterceptor {
    /// Create a new BytedModelHubInterceptor.
    pub fn new() -> Self {
        Self
    }
}

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
            if let Ok(value) = HeaderValue::from_str(&extra_json.to_string()) {
                request.headers.insert("extra", value);
            }
        }
    }
}

#[cfg(test)]
#[path = "byted_model_hub.test.rs"]
mod tests;
