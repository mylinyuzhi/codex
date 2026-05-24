use super::*;
use crate::messages::Message;
use async_trait::async_trait;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;

#[derive(Debug)]
struct OrderTrackingHook {
    name: String,
    priority: i32,
    counter: Arc<AtomicI32>,
    recorded_order: Arc<std::sync::Mutex<Vec<i32>>>,
}

impl OrderTrackingHook {
    fn new(
        name: &str,
        priority: i32,
        counter: Arc<AtomicI32>,
        recorded_order: Arc<std::sync::Mutex<Vec<i32>>>,
    ) -> Self {
        Self {
            name: name.to_string(),
            priority,
            counter,
            recorded_order,
        }
    }
}

#[async_trait]
impl RequestHook for OrderTrackingHook {
    async fn on_request(
        &self,
        _request: &mut GenerateRequest,
        _context: &mut HookContext,
    ) -> Result<(), HyperError> {
        let order = self.counter.fetch_add(1, Ordering::SeqCst);
        self.recorded_order.lock().unwrap().push(order);
        Ok(())
    }

    fn priority(&self) -> i32 {
        self.priority
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug)]
struct ModifyTempHook {
    temp: f64,
}

#[async_trait]
impl RequestHook for ModifyTempHook {
    async fn on_request(
        &self,
        request: &mut GenerateRequest,
        _context: &mut HookContext,
    ) -> Result<(), HyperError> {
        request.temperature = Some(self.temp);
        Ok(())
    }

    fn priority(&self) -> i32 {
        100
    }

    fn name(&self) -> &str {
        "modify_temp"
    }
}

#[tokio::test]
async fn test_hook_chain_priority_order() {
    let counter = Arc::new(AtomicI32::new(0));
    let recorded = Arc::new(std::sync::Mutex::new(Vec::new()));

    let mut chain = HookChain::new();

    // Add hooks in reverse priority order
    chain.add_request_hook(Arc::new(OrderTrackingHook::new(
        "low_priority",
        200,
        counter.clone(),
        recorded.clone(),
    )));
    chain.add_request_hook(Arc::new(OrderTrackingHook::new(
        "high_priority",
        10,
        counter.clone(),
        recorded.clone(),
    )));
    chain.add_request_hook(Arc::new(OrderTrackingHook::new(
        "medium_priority",
        100,
        counter.clone(),
        recorded.clone(),
    )));

    let mut request = GenerateRequest::new(vec![Message::user("test")]);
    let mut context = HookContext::new();

    chain
        .run_request_hooks(&mut request, &mut context)
        .await
        .unwrap();

    // Hooks should have run in priority order (10, 100, 200)
    let order = recorded.lock().unwrap();
    assert_eq!(*order, vec![0, 1, 2]);
}

#[tokio::test]
async fn test_hook_chain_modifies_request() {
    let mut chain = HookChain::new();
    chain.add_request_hook(Arc::new(ModifyTempHook { temp: 0.42 }));

    let mut request = GenerateRequest::new(vec![Message::user("test")]);
    assert!(request.temperature.is_none());

    let mut context = HookContext::new();
    chain
        .run_request_hooks(&mut request, &mut context)
        .await
        .unwrap();

    assert_eq!(request.temperature, Some(0.42));
}

#[test]
fn test_hook_chain_counts() {
    let mut chain = HookChain::new();
    assert!(!chain.has_request_hooks());
    assert_eq!(chain.request_hook_count(), 0);

    chain.add_request_hook(Arc::new(ModifyTempHook { temp: 0.5 }));
    assert!(chain.has_request_hooks());
    assert_eq!(chain.request_hook_count(), 1);

    chain.clear();
    assert!(!chain.has_request_hooks());
}
