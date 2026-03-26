use cocode_api::ToolCall;
use cocode_protocol::INTERRUPTED_BY_USER;
use cocode_protocol::INTERRUPTED_FOR_TOOL_USE;

use super::build_interrupt_tool_results_xml;

fn make_tool_call(id: &str, name: &str) -> ToolCall {
    ToolCall::new(id, name, serde_json::Value::Null)
}

#[test]
fn test_build_interrupt_xml_single_tool() {
    let tc = make_tool_call("call_abc", "Bash");
    let refs = vec![&tc];
    let xml = build_interrupt_tool_results_xml(&refs, INTERRUPTED_FOR_TOOL_USE);

    assert!(xml.contains("<tool_result tool_use_id=\"call_abc\" name=\"Bash\">"));
    assert!(xml.contains(INTERRUPTED_FOR_TOOL_USE));
    assert!(xml.contains("</tool_result>"));
    // Guidance text appears after the XML block
    assert!(xml.ends_with(INTERRUPTED_FOR_TOOL_USE));
}

#[test]
fn test_build_interrupt_xml_multiple_tools() {
    let tc1 = make_tool_call("call_1", "Read");
    let tc2 = make_tool_call("call_2", "Write");
    let tc3 = make_tool_call("call_3", "Bash");
    let refs = vec![&tc1, &tc2, &tc3];
    let xml = build_interrupt_tool_results_xml(&refs, INTERRUPTED_FOR_TOOL_USE);

    // Each tool gets its own <tool_result> block
    assert!(xml.contains("tool_use_id=\"call_1\" name=\"Read\""));
    assert!(xml.contains("tool_use_id=\"call_2\" name=\"Write\""));
    assert!(xml.contains("tool_use_id=\"call_3\" name=\"Bash\""));

    // Blocks are separated by double newlines
    let block_count = xml.matches("<tool_result").count();
    assert_eq!(block_count, 3);
}

#[test]
fn test_build_interrupt_xml_for_tool_use() {
    let tc = make_tool_call("call_x", "Edit");
    let refs = vec![&tc];
    let xml = build_interrupt_tool_results_xml(&refs, INTERRUPTED_FOR_TOOL_USE);

    assert!(xml.contains(INTERRUPTED_FOR_TOOL_USE));
    assert!(!xml.contains(INTERRUPTED_BY_USER));
}

#[test]
fn test_build_interrupt_xml_without_tool_use() {
    let tc = make_tool_call("call_y", "Read");
    let refs = vec![&tc];
    let xml = build_interrupt_tool_results_xml(&refs, INTERRUPTED_BY_USER);

    assert!(xml.contains(INTERRUPTED_BY_USER));
    assert!(!xml.contains(INTERRUPTED_FOR_TOOL_USE));
}

#[test]
fn test_build_interrupt_xml_empty_calls_returns_guidance_only() {
    let refs: Vec<&ToolCall> = vec![];
    let xml = build_interrupt_tool_results_xml(&refs, INTERRUPTED_FOR_TOOL_USE);

    // No <tool_result> blocks when no interrupted calls
    assert!(!xml.contains("<tool_result"));
    // Guidance text still present
    assert!(xml.contains(INTERRUPTED_FOR_TOOL_USE));
}
