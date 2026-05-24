use super::*;

#[test]
fn test_find_pricing_sonnet() {
    let p = find_pricing("claude-sonnet-4-20250514");
    assert_eq!(p.name, "claude-sonnet-4");
    assert!((p.input_per_m - 3.0).abs() < f64::EPSILON);
}

#[test]
fn test_find_pricing_opus() {
    let p = find_pricing("claude-opus-4-20250514");
    assert_eq!(p.name, "claude-opus-4");
    assert!((p.input_per_m - 15.0).abs() < f64::EPSILON);
}

#[test]
fn test_find_pricing_haiku() {
    let p = find_pricing("claude-haiku-3-20250307");
    assert_eq!(p.name, "claude-haiku-3");
}

#[test]
fn test_find_pricing_unknown_defaults_to_sonnet() {
    let p = find_pricing("gpt-4-turbo");
    assert_eq!(p.name, "claude-sonnet-4");
}

#[test]
fn test_format_num() {
    assert_eq!(format_num(0), "0");
    assert_eq!(format_num(1_234), "1,234");
    assert_eq!(format_num(100_000), "100,000");
}

#[tokio::test]
async fn test_cost_handler_empty_sessions() {
    let output = handler(String::new()).await.unwrap();
    assert!(output.contains("Session Cost"));
    // Should either show "No API usage" or actual data
    assert!(
        output.contains("No API usage") || output.contains("Total"),
        "unexpected: {output}"
    );
}

#[tokio::test]
async fn test_collect_usage_with_session() {
    let tmp = tempfile::tempdir().unwrap();
    let session = serde_json::json!({
        "turns": [
            {
                "model": "claude-sonnet-4-20250514",
                "usage": {
                    "input_tokens": 1500,
                    "output_tokens": 500,
                    "cache_read_input_tokens": 200,
                    "cache_creation_input_tokens": 100,
                }
            },
            {
                "model": "claude-sonnet-4-20250514",
                "usage": {
                    "input_tokens": 2000,
                    "output_tokens": 800,
                }
            },
            {
                "model": "claude-haiku-3-20250307",
                "usage": {
                    "input_tokens": 500,
                    "output_tokens": 100,
                }
            }
        ]
    });

    tokio::fs::write(
        tmp.path().join("session.json"),
        serde_json::to_string(&session).unwrap(),
    )
    .await
    .unwrap();

    let buckets = collect_usage(tmp.path()).await;
    assert_eq!(buckets.len(), 2, "should have 2 model buckets");

    let sonnet = buckets.iter().find(|b| b.model.contains("sonnet")).unwrap();
    assert_eq!(sonnet.input_tokens, 3500);
    assert_eq!(sonnet.output_tokens, 1300);
    assert_eq!(sonnet.cache_read_tokens, 200);
    assert_eq!(sonnet.api_requests, 2);

    let haiku = buckets.iter().find(|b| b.model.contains("haiku")).unwrap();
    assert_eq!(haiku.input_tokens, 500);
    assert_eq!(haiku.output_tokens, 100);
    assert_eq!(haiku.api_requests, 1);
}

#[tokio::test]
async fn test_collect_usage_nonexistent_dir() {
    let buckets = collect_usage(std::path::Path::new("/tmp/nonexistent_cost_dir_xyz")).await;
    assert!(buckets.is_empty());
}
