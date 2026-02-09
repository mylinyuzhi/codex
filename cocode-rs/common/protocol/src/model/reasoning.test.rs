use super::*;

#[test]
fn test_ord_comparison() {
    // Test Ord trait - variants are ordered from lowest to highest
    assert!(ReasoningEffort::None < ReasoningEffort::Minimal);
    assert!(ReasoningEffort::Minimal < ReasoningEffort::Low);
    assert!(ReasoningEffort::Low < ReasoningEffort::Medium);
    assert!(ReasoningEffort::Medium < ReasoningEffort::High);
    assert!(ReasoningEffort::High < ReasoningEffort::XHigh);

    // Direct comparison
    assert!(ReasoningEffort::High > ReasoningEffort::Low);
    assert!(ReasoningEffort::Medium == ReasoningEffort::Medium);
    assert!(ReasoningEffort::XHigh >= ReasoningEffort::High);
}

#[test]
fn test_nearest_effort() {
    let supported = vec![
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
    ];

    // Exact match
    assert_eq!(
        nearest_effort(ReasoningEffort::Medium, &supported),
        ReasoningEffort::Medium
    );

    // None -> Low (nearest)
    assert_eq!(
        nearest_effort(ReasoningEffort::None, &supported),
        ReasoningEffort::Low
    );

    // XHigh -> High (nearest)
    assert_eq!(
        nearest_effort(ReasoningEffort::XHigh, &supported),
        ReasoningEffort::High
    );
}

#[test]
fn test_default() {
    assert_eq!(ReasoningEffort::default(), ReasoningEffort::Medium);
}

#[test]
fn test_serde() {
    let effort = ReasoningEffort::High;
    let json = serde_json::to_string(&effort).expect("serialize");
    assert_eq!(json, "\"high\"");

    let parsed: ReasoningEffort = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, ReasoningEffort::High);
}

#[test]
fn test_reasoning_summary_default() {
    assert_eq!(ReasoningSummary::default(), ReasoningSummary::Auto);
}

#[test]
fn test_reasoning_summary_serde() {
    let summary = ReasoningSummary::Detailed;
    let json = serde_json::to_string(&summary).expect("serialize");
    assert_eq!(json, "\"detailed\"");

    let parsed: ReasoningSummary = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, ReasoningSummary::Detailed);
}

#[test]
fn test_reasoning_summary_display() {
    assert_eq!(ReasoningSummary::None.to_string(), "none");
    assert_eq!(ReasoningSummary::Auto.to_string(), "auto");
    assert_eq!(ReasoningSummary::Concise.to_string(), "concise");
    assert_eq!(ReasoningSummary::Detailed.to_string(), "detailed");
}
