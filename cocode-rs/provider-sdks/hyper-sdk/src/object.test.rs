use super::*;

#[test]
fn test_object_request() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "age": { "type": "integer" }
        },
        "required": ["name", "age"]
    });

    let request = ObjectRequest::from_text("Generate a person", schema.clone())
        .schema_name("Person")
        .temperature(0.7)
        .max_tokens(100);

    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.schema, schema);
    assert_eq!(request.schema_name, Some("Person".to_string()));
    assert_eq!(request.temperature, Some(0.7));
}

#[test]
fn test_object_response_parse() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Person {
        name: String,
        age: i32,
    }

    let response = ObjectResponse::new(
        "resp_1",
        "gpt-4o",
        serde_json::json!({
            "name": "Alice",
            "age": 30
        }),
    );

    let person: Person = response.parse().unwrap();
    assert_eq!(
        person,
        Person {
            name: "Alice".to_string(),
            age: 30
        }
    );
}

#[test]
fn test_object_stream_events() {
    let started = ObjectStreamEvent::started("resp_1");
    assert!(matches!(started, ObjectStreamEvent::Started { id } if id == "resp_1"));

    let delta = ObjectStreamEvent::delta(r#"{"name":"#);
    assert!(matches!(delta, ObjectStreamEvent::Delta { delta } if delta == r#"{"name":"#));

    let done = ObjectStreamEvent::done("resp_1", serde_json::json!({"name": "Alice"}), None);
    assert!(matches!(done, ObjectStreamEvent::Done { .. }));
}
