use super::*;

#[test]
fn test_parse_unquoted_file_mentions() {
    let mentions = parse_file_mentions("Check @file.txt and @src/main.rs");
    assert_eq!(mentions.len(), 2);
    assert_eq!(mentions[0].raw_path, "file.txt");
    assert_eq!(mentions[1].raw_path, "src/main.rs");
    assert!(!mentions[0].is_quoted);
}

#[test]
fn test_parse_quoted_file_mentions() {
    let mentions = parse_file_mentions(r#"Check @"path with spaces/file.txt""#);
    assert_eq!(mentions.len(), 1);
    assert_eq!(mentions[0].raw_path, "path with spaces/file.txt");
    assert!(mentions[0].is_quoted);
}

#[test]
fn test_parse_file_with_line_range() {
    let mentions = parse_file_mentions("Check @file.txt:10-20");
    assert_eq!(mentions.len(), 1);
    assert_eq!(mentions[0].raw_path, "file.txt");
    assert_eq!(mentions[0].line_start, Some(10));
    assert_eq!(mentions[0].line_end, Some(20));
}

#[test]
fn test_parse_file_with_line_start_only() {
    let mentions = parse_file_mentions("Check @file.txt:42");
    assert_eq!(mentions.len(), 1);
    assert_eq!(mentions[0].raw_path, "file.txt");
    assert_eq!(mentions[0].line_start, Some(42));
    assert_eq!(mentions[0].line_end, None); // None means "to EOF"
}

#[test]
fn test_parse_agent_mentions() {
    let mentions = parse_agent_mentions("Use @agent-search to find files");
    assert_eq!(mentions.len(), 1);
    assert_eq!(mentions[0].agent_type, "search");
}

#[test]
fn test_parse_multiple_agent_mentions() {
    let mentions = parse_agent_mentions("Use @agent-search and @agent-edit");
    assert_eq!(mentions.len(), 2);
    assert_eq!(mentions[0].agent_type, "search");
    assert_eq!(mentions[1].agent_type, "edit");
}

#[test]
fn test_parse_mentions_combined() {
    let result = parse_mentions("Check @file.txt and use @agent-search");
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].raw_path, "file.txt");
    assert_eq!(result.agents.len(), 1);
    assert_eq!(result.agents[0].agent_type, "search");
}

#[test]
fn test_parse_mentions_deduplication() {
    let result = parse_mentions("Check @file.txt and @file.txt again");
    assert_eq!(result.files.len(), 1);
}

#[test]
fn test_file_mention_resolve() {
    let mention = FileMention {
        raw_path: "src/main.rs".to_string(),
        line_start: None,
        line_end: None,
        is_quoted: false,
    };
    let resolved = mention.resolve(Path::new("/project"));
    assert_eq!(resolved, PathBuf::from("/project/src/main.rs"));
}

#[test]
fn test_file_mention_resolve_absolute() {
    let mention = FileMention {
        raw_path: "/absolute/path.rs".to_string(),
        line_start: None,
        line_end: None,
        is_quoted: false,
    };
    let resolved = mention.resolve(Path::new("/project"));
    assert_eq!(resolved, PathBuf::from("/absolute/path.rs"));
}

#[test]
fn test_parse_line_range() {
    let regex = line_range_regex();
    assert_eq!(
        parse_line_range_with_regex("file.txt:10-20", &regex),
        ("file.txt".to_string(), Some(10), Some(20))
    );
    assert_eq!(
        parse_line_range_with_regex("file.txt:42", &regex),
        ("file.txt".to_string(), Some(42), None)
    );
    assert_eq!(
        parse_line_range_with_regex("file.txt", &regex),
        ("file.txt".to_string(), None, None)
    );
}

#[test]
fn test_agent_mentions_not_in_file_mentions() {
    let result = parse_mentions("@agent-search @file.txt");
    assert_eq!(result.files.len(), 1);
    assert_eq!(result.files[0].raw_path, "file.txt");
    assert_eq!(result.agents.len(), 1);
    assert_eq!(result.agents[0].agent_type, "search");
}