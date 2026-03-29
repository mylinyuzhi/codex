#[test]
fn test_parse_server_command_single() {
    let parts: Vec<&str> = "node".split_whitespace().collect();
    let (program, args) = parts.split_first().expect("should have program");
    assert_eq!(*program, "node");
    assert!(args.is_empty());
}

#[test]
fn test_parse_server_command_with_args() {
    let parts: Vec<&str> = "node server.js --port 3000".split_whitespace().collect();
    let (program, args) = parts.split_first().expect("should have program");
    assert_eq!(*program, "node");
    assert_eq!(args, &["server.js", "--port", "3000"]);
}
