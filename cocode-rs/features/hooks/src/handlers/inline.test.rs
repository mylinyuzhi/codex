use super::*;
use crate::event::HookEventType;
use std::path::PathBuf;

#[test]
fn test_inline_handler() {
    let handler: InlineHandler = Box::new(|ctx| {
        if ctx.tool_name.as_deref() == Some("bash") {
            HookResult::Reject {
                reason: "bash is not allowed".to_string(),
            }
        } else {
            HookResult::Continue
        }
    });

    let ctx = HookContext::new(
        HookEventType::PreToolUse,
        "s1".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_tool_name("bash");
    let result = handler(&ctx);
    assert!(matches!(result, HookResult::Reject { .. }));

    let ctx2 = HookContext::new(
        HookEventType::PreToolUse,
        "s1".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_tool_name("read_file");
    let result2 = handler(&ctx2);
    assert!(matches!(result2, HookResult::Continue));
}
