use super::*;

#[test]
fn test_generate_client_call_id() {
    let call_id = generate_client_call_id("get_weather", 0);
    assert!(call_id.starts_with("cligen@get_weather#0@"));
    assert!(is_enhanced_call_id(&call_id));
    assert!(is_client_generated_call_id(&call_id));
    assert_eq!(
        parse_function_name_from_call_id(&call_id),
        Some("get_weather")
    );
    assert_eq!(parse_call_index(&call_id), Some(0));
    assert_eq!(extract_original_call_id(&call_id), None);
}

#[test]
fn test_generate_client_call_id_with_index() {
    let call_id_0 = generate_client_call_id("get_weather", 0);
    let call_id_1 = generate_client_call_id("get_weather", 1);
    let call_id_5 = generate_client_call_id("get_weather", 5);

    assert!(call_id_0.starts_with("cligen@get_weather#0@"));
    assert!(call_id_1.starts_with("cligen@get_weather#1@"));
    assert!(call_id_5.starts_with("cligen@get_weather#5@"));

    assert_eq!(parse_call_index(&call_id_0), Some(0));
    assert_eq!(parse_call_index(&call_id_1), Some(1));
    assert_eq!(parse_call_index(&call_id_5), Some(5));

    // All should have the same function name
    assert_eq!(
        parse_function_name_from_call_id(&call_id_0),
        Some("get_weather")
    );
    assert_eq!(
        parse_function_name_from_call_id(&call_id_1),
        Some("get_weather")
    );
    assert_eq!(
        parse_function_name_from_call_id(&call_id_5),
        Some("get_weather")
    );
}

#[test]
fn test_enhance_server_call_id() {
    let call_id = enhance_server_call_id("call_abc123", "search_files");
    assert_eq!(call_id, "srvgen@search_files@call_abc123");
    assert!(is_enhanced_call_id(&call_id));
    assert!(!is_client_generated_call_id(&call_id));
    assert_eq!(
        parse_function_name_from_call_id(&call_id),
        Some("search_files")
    );
    assert_eq!(extract_original_call_id(&call_id), Some("call_abc123"));
    assert_eq!(parse_call_index(&call_id), None);
}

#[test]
fn test_non_enhanced_call_id() {
    let call_id = "some_random_call_id";
    assert!(!is_enhanced_call_id(call_id));
    assert!(!is_client_generated_call_id(call_id));
    assert_eq!(parse_function_name_from_call_id(call_id), None);
    assert_eq!(extract_original_call_id(call_id), None);
    assert_eq!(parse_call_index(call_id), None);
}

#[test]
fn test_function_name_with_underscores() {
    // Function names with underscores should work correctly
    let client_id = generate_client_call_id("read_file_contents", 0);
    assert_eq!(
        parse_function_name_from_call_id(&client_id),
        Some("read_file_contents")
    );
    assert_eq!(parse_call_index(&client_id), Some(0));

    let server_id = enhance_server_call_id("srv_123", "write_to_database");
    assert_eq!(
        parse_function_name_from_call_id(&server_id),
        Some("write_to_database")
    );
    assert_eq!(extract_original_call_id(&server_id), Some("srv_123"));
}

#[test]
fn test_uuid_uniqueness() {
    // Generate multiple IDs with same name/index - should be unique
    let id1 = generate_client_call_id("test", 0);
    let id2 = generate_client_call_id("test", 0);
    assert_ne!(id1, id2);
}
