use super::*;
use crate::messages::Message;

#[derive(Debug)]
struct TestRequestHook {
    name: String,
    priority: i32,
}

impl TestRequestHook {
    fn new(name: &str, priority: i32) -> Self {
        Self {
            name: name.to_string(),
            priority,
        }
    }
}

#[async_trait]
impl RequestHook for TestRequestHook {
    async fn on_request(
        &self,
        request: &mut GenerateRequest,
        _context: &mut HookContext,
    ) -> Result<(), HyperError> {
        request.temperature = Some(0.5);
        Ok(())
    }

    fn priority(&self) -> i32 {
        self.priority
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[test]
fn test_hook_context_builder() {
    let context = HookContext::with_provider("openai", "gpt-4o").conversation_id("conv_123");

    assert_eq!(context.provider, "openai");
    assert_eq!(context.model_id, "gpt-4o");
    assert_eq!(context.conversation_id, Some("conv_123".to_string()));
}

#[test]
fn test_hook_context_metadata() {
    let mut context = HookContext::new();
    context.set_metadata("key", serde_json::json!("value"));

    assert_eq!(
        context.get_metadata("key"),
        Some(&serde_json::json!("value"))
    );
    assert_eq!(context.get_metadata("nonexistent"), None);
}

#[tokio::test]
async fn test_request_hook() {
    let hook = TestRequestHook::new("test", 50);
    let mut request = GenerateRequest::new(vec![Message::user("Hello")]);
    let mut context = HookContext::new();

    hook.on_request(&mut request, &mut context).await.unwrap();
    assert_eq!(request.temperature, Some(0.5));
    assert_eq!(hook.priority(), 50);
    assert_eq!(hook.name(), "test");
}
