use super::*;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

#[derive(Debug)]
struct TestInterceptor {
    name: String,
    priority: i32,
    call_order: Arc<AtomicI32>,
    expected_order: i32,
}

impl HttpInterceptor for TestInterceptor {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> i32 {
        self.priority
    }

    fn intercept(&self, _request: &mut HttpRequest, _ctx: &HttpInterceptorContext) {
        let order = self.call_order.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            order, self.expected_order,
            "interceptor {} called out of order",
            self.name
        );
    }
}

#[test]
fn test_chain_empty() {
    let chain = HttpInterceptorChain::new();
    assert!(chain.is_empty());
    assert_eq!(chain.len(), 0);
}

#[test]
fn test_chain_add() {
    let mut chain = HttpInterceptorChain::new();

    #[derive(Debug)]
    struct DummyInterceptor;
    impl HttpInterceptor for DummyInterceptor {
        fn name(&self) -> &str {
            "dummy"
        }
        fn intercept(&self, _: &mut HttpRequest, _: &HttpInterceptorContext) {}
    }

    chain.add(Arc::new(DummyInterceptor));
    assert!(!chain.is_empty());
    assert_eq!(chain.len(), 1);
    assert_eq!(chain.names(), vec!["dummy"]);
}

#[test]
fn test_chain_priority_order() {
    let call_order = Arc::new(AtomicI32::new(0));

    let mut chain = HttpInterceptorChain::new();
    // Add in reverse priority order to test sorting
    chain.add(Arc::new(TestInterceptor {
        name: "third".to_string(),
        priority: 300,
        call_order: call_order.clone(),
        expected_order: 2,
    }));
    chain.add(Arc::new(TestInterceptor {
        name: "first".to_string(),
        priority: 50,
        call_order: call_order.clone(),
        expected_order: 0,
    }));
    chain.add(Arc::new(TestInterceptor {
        name: "second".to_string(),
        priority: 100,
        call_order: call_order.clone(),
        expected_order: 1,
    }));

    let mut request = HttpRequest::post("https://example.com");
    let ctx = HttpInterceptorContext::new();
    chain.apply(&mut request, &ctx);

    // All three should have been called
    assert_eq!(call_order.load(Ordering::SeqCst), 3);
}

#[test]
fn test_chain_apply_modifies_request() {
    #[derive(Debug)]
    struct HeaderInterceptor;
    impl HttpInterceptor for HeaderInterceptor {
        fn name(&self) -> &str {
            "header"
        }
        fn intercept(&self, request: &mut HttpRequest, _: &HttpInterceptorContext) {
            let value = http::HeaderValue::from_static("test-value");
            request.headers.insert("X-Test", value);
        }
    }

    let mut chain = HttpInterceptorChain::new();
    chain.add(Arc::new(HeaderInterceptor));

    let mut request = HttpRequest::post("https://example.com");
    let ctx = HttpInterceptorContext::new();
    chain.apply(&mut request, &ctx);

    assert!(request.headers.contains_key("X-Test"));
}

#[test]
fn test_chain_sorted_lazily() {
    // This test verifies that sorting happens lazily (on first apply)
    // and the sorted order is cached for subsequent applies.
    let mut chain = HttpInterceptorChain::new();

    #[derive(Debug)]
    struct OrderTracker {
        name: String,
        priority: i32,
    }
    impl HttpInterceptor for OrderTracker {
        fn name(&self) -> &str {
            &self.name
        }
        fn priority(&self) -> i32 {
            self.priority
        }
        fn intercept(&self, _: &mut HttpRequest, _: &HttpInterceptorContext) {}
    }

    // Add in reverse priority order
    chain.add(Arc::new(OrderTracker {
        name: "high".to_string(),
        priority: 100,
    }));
    chain.add(Arc::new(OrderTracker {
        name: "low".to_string(),
        priority: 50,
    }));

    // Before apply, names() returns insertion order
    assert_eq!(chain.names(), vec!["high", "low"]);

    // Apply triggers sort
    let mut request = HttpRequest::post("https://example.com");
    let ctx = HttpInterceptorContext::new();
    chain.apply(&mut request, &ctx);

    // After apply, names() returns sorted order (by priority)
    assert_eq!(chain.names(), vec!["low", "high"]);
}
