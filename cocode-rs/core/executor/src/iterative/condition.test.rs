use super::*;

#[test]
fn test_count_condition() {
    let cond = IterationCondition::Count { max: 10 };
    match cond {
        IterationCondition::Count { max } => assert_eq!(max, 10),
        _ => panic!("expected Count"),
    }
}

#[test]
fn test_duration_condition() {
    let cond = IterationCondition::Duration { max_secs: 300 };
    match cond {
        IterationCondition::Duration { max_secs } => assert_eq!(max_secs, 300),
        _ => panic!("expected Duration"),
    }
}

#[test]
fn test_until_condition() {
    let cond = IterationCondition::Until {
        check: "all tests pass".to_string(),
    };
    match cond {
        IterationCondition::Until { check } => assert_eq!(check, "all tests pass"),
        _ => panic!("expected Until"),
    }
}

#[test]
fn test_condition_serde_roundtrip() {
    let conditions = vec![
        IterationCondition::Count { max: 5 },
        IterationCondition::Duration { max_secs: 60 },
        IterationCondition::Until {
            check: "done".to_string(),
        },
    ];
    for cond in &conditions {
        let json = serde_json::to_string(cond).expect("serialize");
        let back: IterationCondition = serde_json::from_str(&json).expect("deserialize");
        let json2 = serde_json::to_string(&back).expect("re-serialize");
        assert_eq!(json, json2);
    }
}
