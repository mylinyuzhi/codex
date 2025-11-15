use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use async_trait::async_trait;
use globset::GlobBuilder;
use ignore::WalkBuilder;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

pub struct GlobHandler;

const DEFAULT_LIMIT: usize = 1000;
// 最大返回文件数：防止过大的limit消耗内存
const MAX_LIMIT: usize = 2000;
// 最近文件阈值：24小时内修改的文件优先显示，按修改时间降序排列
const RECENT_THRESHOLD_HOURS: i64 = 24;

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

/// 判断文件是否在最近阈值内（小于指定时间间隔）
fn is_recent_file(modified_time: Option<SystemTime>, now: SystemTime, threshold: Duration) -> bool {
    modified_time
        .and_then(|t| now.duration_since(t).ok())
        .is_some_and(|d| d < threshold)
}

#[derive(Deserialize)]
struct GlobArgs {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    case_sensitive: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileEntry {
    absolute_path: PathBuf,
    relative_path: String,
    modified_time: Option<SystemTime>,
}

// Implement Ord for FileEntry to work with BinaryHeap
// Priority: Recent files (newest first) > Old files (alphabetical)
impl Ord for FileEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // For BinaryHeap (max-heap), we want the "best" items at the top
        // But we'll use Reverse<FileEntry> so this defines "worst" ordering
        let now = SystemTime::now();
        let threshold = Duration::from_secs((RECENT_THRESHOLD_HOURS * 3600) as u64);

        let self_is_recent = is_recent_file(self.modified_time, now, threshold);
        let other_is_recent = is_recent_file(other.modified_time, now, threshold);

        match (self_is_recent, other_is_recent) {
            (true, true) => {
                // Both recent: newer is better (for min-heap, worse comes first)
                self.modified_time.cmp(&other.modified_time)
            }
            (true, false) => {
                // self is recent, other is not: self is better (should come later in min-heap)
                std::cmp::Ordering::Greater
            }
            (false, true) => {
                // other is recent, self is not: other is better
                std::cmp::Ordering::Less
            }
            (false, false) => {
                // Both old: alphabetically earlier is better, later is worse
                // Invert comparison so a.rs > z.rs (alphabetically earlier = greater priority)
                other.relative_path.cmp(&self.relative_path)
            }
        }
    }
}

impl PartialOrd for FileEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[async_trait]
impl ToolHandler for GlobHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation { payload, turn, .. } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "glob handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: GlobArgs = serde_json::from_str(&arguments).map_err(|err| {
            FunctionCallError::RespondToModel(format!(
                "failed to parse function arguments: {err:?}"
            ))
        })?;

        let pattern = args.pattern.trim();
        if pattern.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "pattern must not be empty".to_string(),
            ));
        }

        if args.limit == 0 {
            return Err(FunctionCallError::RespondToModel(
                "limit must be greater than zero".to_string(),
            ));
        }

        let limit = args.limit.min(MAX_LIMIT);
        let search_path = turn.resolve_path(args.path.clone());
        let case_sensitive = args.case_sensitive.unwrap_or(false);

        verify_path_exists(&search_path).await?;

        let matched_files =
            find_matching_files(pattern, &search_path, limit, case_sensitive).await?;

        format_output(matched_files, pattern, limit)
    }
}

async fn verify_path_exists(path: &Path) -> Result<(), FunctionCallError> {
    tokio::fs::metadata(path).await.map_err(|err| {
        FunctionCallError::RespondToModel(format!("unable to access `{}`: {err}", path.display()))
    })?;
    Ok(())
}

async fn find_matching_files(
    pattern: &str,
    search_path: &Path,
    limit: usize,
    case_sensitive: bool,
) -> Result<Vec<FileEntry>, FunctionCallError> {
    // Build glob matcher with proper case sensitivity support
    let glob = GlobBuilder::new(pattern)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|err| FunctionCallError::RespondToModel(format!("Invalid glob pattern: {err}")))?;

    let matcher = glob.compile_matcher();

    // Use a min-heap (via Reverse) to maintain top N files efficiently
    // We want to keep the "best" files, so we use Reverse to make it a max-heap of "good" files
    let mut top_files: BinaryHeap<Reverse<FileEntry>> = BinaryHeap::new();

    // Use ignore::WalkBuilder for gitignore support
    let walker = WalkBuilder::new(search_path)
        .hidden(false) // Include hidden files (but gitignore may filter them)
        .git_ignore(true) // Respect .gitignore
        .git_global(true) // Respect global gitignore
        .git_exclude(true) // Respect .git/info/exclude
        .ignore(true) // Respect .ignore files
        .parents(true) // Check parent directories for ignore files
        .follow_links(false) // Don't follow symlinks for security
        .require_git(false) // Don't require git to be present
        .build();

    for entry_result in walker {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(_) => continue, // Skip errors (permission denied, etc.)
        };

        // Skip directories
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            continue;
        }

        let path = entry.path();
        let relative_path = match path.strip_prefix(search_path) {
            Ok(rel) => rel,
            Err(_) => continue,
        };

        // Match against the glob pattern
        if !matcher.is_match(relative_path) {
            continue;
        }

        let modified_time = entry.metadata().ok().and_then(|m| m.modified().ok());

        let file_entry = FileEntry {
            absolute_path: path.to_path_buf(),
            relative_path: relative_path.to_string_lossy().to_string(),
            modified_time,
        };

        // Efficiently maintain top N files using heap
        if top_files.len() < limit {
            top_files.push(Reverse(file_entry));
        } else if let Some(worst) = top_files.peek() {
            // If this file is better than the worst file in our heap, replace it
            if file_entry > worst.0 {
                top_files.pop();
                top_files.push(Reverse(file_entry));
            }
        }
    }

    // Extract and sort final results
    let mut results: Vec<FileEntry> = top_files.into_iter().map(|Reverse(f)| f).collect();

    // Sort by our desired order (recent first, then alphabetical)
    sort_files_two_tier(&mut results);

    Ok(results)
}

fn sort_files_two_tier(files: &mut [FileEntry]) {
    let now = SystemTime::now();
    let threshold = Duration::from_secs((RECENT_THRESHOLD_HOURS * 3600) as u64);

    files.sort_by(|a, b| {
        let a_is_recent = is_recent_file(a.modified_time, now, threshold);
        let b_is_recent = is_recent_file(b.modified_time, now, threshold);

        match (a_is_recent, b_is_recent) {
            (true, true) => {
                // Both recent: newest first (reverse chronological)
                b.modified_time.cmp(&a.modified_time)
            }
            (true, false) => {
                // a is recent, b is not: a comes first
                std::cmp::Ordering::Less
            }
            (false, true) => {
                // b is recent, a is not: b comes first
                std::cmp::Ordering::Greater
            }
            (false, false) => {
                // Both old: alphabetical order
                a.relative_path.cmp(&b.relative_path)
            }
        }
    });
}

fn format_output(
    matched_files: Vec<FileEntry>,
    pattern: &str,
    limit: usize,
) -> Result<ToolOutput, FunctionCallError> {
    if matched_files.is_empty() {
        let message = format!("No files found matching pattern \"{pattern}\"");
        return Ok(ToolOutput::Function {
            content: message,
            content_items: None,
            success: Some(false),
        });
    }

    let mut output_lines = Vec::new();

    // Summary line
    let summary = format!(
        "Found {} file(s) matching \"{}\"",
        matched_files.len(),
        pattern
    );

    output_lines.push(summary);
    output_lines.push(String::new()); // Empty line

    // File paths (absolute)
    for file in &matched_files {
        output_lines.push(file.absolute_path.display().to_string());
    }

    // Truncation warning
    if matched_files.len() == limit {
        output_lines.push(String::new());
        output_lines.push(format!(
            "(Results limited to {}. Use a more specific pattern or increase limit to see more)",
            limit
        ));
    }

    Ok(ToolOutput::Function {
        content: output_lines.join("\n"),
        content_items: None,
        success: Some(true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // ============================================================================
    // P1: Core functionality tests
    // ============================================================================

    #[tokio::test]
    async fn test_recursive_glob() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        // Create nested structure
        fs::create_dir_all(dir.join("src/tools")).unwrap();
        fs::write(dir.join("main.rs"), "content").unwrap();
        fs::write(dir.join("src/lib.rs"), "content").unwrap();
        fs::write(dir.join("src/tools/glob.rs"), "content").unwrap();

        let files = find_matching_files("**/*.rs", dir, 100, false)
            .await
            .unwrap();

        assert_eq!(files.len(), 3);
        let paths: Vec<_> = files.iter().map(|f| &f.relative_path).collect();
        assert!(paths.contains(&&"main.rs".to_string()));
        assert!(paths.contains(&&"src/lib.rs".to_string()));
        assert!(paths.contains(&&"src/tools/glob.rs".to_string()));
    }

    #[tokio::test]
    async fn test_prefix_recursive_glob() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        fs::create_dir_all(dir.join("src/tools")).unwrap();
        fs::create_dir_all(dir.join("tests")).unwrap();
        fs::write(dir.join("main.rs"), "").unwrap();
        fs::write(dir.join("src/lib.rs"), "").unwrap();
        fs::write(dir.join("src/tools/glob.rs"), "").unwrap();
        fs::write(dir.join("tests/test.rs"), "").unwrap();

        let files = find_matching_files("src/**/*.rs", dir, 100, false)
            .await
            .unwrap();

        assert_eq!(files.len(), 2);
        let paths: Vec<_> = files.iter().map(|f| &f.relative_path).collect();
        assert!(paths.contains(&&"src/lib.rs".to_string()));
        assert!(paths.contains(&&"src/tools/glob.rs".to_string()));
        assert!(!paths.contains(&&"main.rs".to_string()));
        assert!(!paths.contains(&&"tests/test.rs".to_string()));
    }

    #[tokio::test]
    async fn test_gitignore_filtering() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        // Create .gitignore
        fs::write(dir.join(".gitignore"), "*.log\ntarget/\n").unwrap();

        fs::write(dir.join("test.rs"), "content").unwrap();
        fs::write(dir.join("debug.log"), "content").unwrap();
        fs::create_dir(dir.join("target")).unwrap();
        fs::write(dir.join("target/output.rs"), "content").unwrap();

        let files = find_matching_files("**/*", dir, 100, false).await.unwrap();

        // Should find test.rs and .gitignore, but NOT debug.log or target/output.rs
        let paths: Vec<_> = files.iter().map(|f| &f.relative_path).collect();
        assert!(paths.contains(&&"test.rs".to_string()));
        assert!(!paths.contains(&&"debug.log".to_string()));
        assert!(!paths.contains(&&"target/output.rs".to_string()));
    }

    #[tokio::test]
    async fn test_case_insensitive_by_default() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        fs::write(dir.join("Test.RS"), "content").unwrap();
        fs::write(dir.join("main.rs"), "content").unwrap();

        let files = find_matching_files("*.rs", dir, 10, false).await.unwrap();

        assert_eq!(files.len(), 2);
    }

    #[tokio::test]
    async fn test_case_sensitive() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        // Test case-sensitive pattern matching
        // Use distinct filenames to avoid filesystem case-sensitivity issues
        fs::write(dir.join("Test.txt"), "content").unwrap();
        fs::write(dir.join("test.rs"), "content").unwrap();
        fs::write(dir.join("main.rs"), "content").unwrap();

        // Case-sensitive pattern: should NOT match .txt files
        let files = find_matching_files("*.rs", dir, 10, true).await.unwrap();

        assert_eq!(files.len(), 2);
        let paths: Vec<_> = files.iter().map(|f| &f.relative_path).collect();
        assert!(paths.contains(&&"test.rs".to_string()));
        assert!(paths.contains(&&"main.rs".to_string()));
        assert!(!paths.iter().any(|p| p.ends_with(".txt")));
    }

    #[tokio::test]
    async fn test_simple_glob() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        fs::write(dir.join("test.rs"), "content").unwrap();
        fs::write(dir.join("test.txt"), "content").unwrap();
        fs::write(dir.join("main.rs"), "content").unwrap();

        let files = find_matching_files("*.rs", dir, 10, false).await.unwrap();

        assert_eq!(files.len(), 2);
        let names: Vec<_> = files.iter().map(|f| &f.relative_path).collect();
        assert!(names.contains(&&"test.rs".to_string()));
        assert!(names.contains(&&"main.rs".to_string()));
    }

    #[tokio::test]
    async fn test_respects_limit() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        for i in 0..10 {
            fs::write(dir.join(format!("file{i}.rs")), "content").unwrap();
        }

        let files = find_matching_files("*.rs", dir, 5, false).await.unwrap();

        assert_eq!(files.len(), 5);
    }

    // ============================================================================
    // P2: Two-tier sorting tests
    // ============================================================================

    #[test]
    fn test_sorts_recent_files_first() {
        let now = SystemTime::now();
        let one_hour_ago = now - Duration::from_secs(3600);
        let two_days_ago = now - Duration::from_secs(2 * 24 * 3600);

        let mut files = vec![
            FileEntry {
                absolute_path: PathBuf::from("/old/file1.rs"),
                relative_path: "old/file1.rs".to_string(),
                modified_time: Some(two_days_ago),
            },
            FileEntry {
                absolute_path: PathBuf::from("/recent/file2.rs"),
                relative_path: "recent/file2.rs".to_string(),
                modified_time: Some(one_hour_ago),
            },
            FileEntry {
                absolute_path: PathBuf::from("/old/file3.rs"),
                relative_path: "old/file3.rs".to_string(),
                modified_time: Some(two_days_ago),
            },
        ];

        sort_files_two_tier(&mut files);

        // Recent file should be first
        assert_eq!(files[0].relative_path, "recent/file2.rs");
        // Old files should be alphabetical
        assert_eq!(files[1].relative_path, "old/file1.rs");
        assert_eq!(files[2].relative_path, "old/file3.rs");
    }

    #[test]
    fn test_multiple_recent_files_sorted_by_mtime() {
        let now = SystemTime::now();
        let one_hour_ago = now - Duration::from_secs(3600);
        let five_hours_ago = now - Duration::from_secs(5 * 3600);
        let ten_hours_ago = now - Duration::from_secs(10 * 3600);

        let mut files = vec![
            FileEntry {
                absolute_path: PathBuf::from("/file1.rs"),
                relative_path: "file1.rs".to_string(),
                modified_time: Some(ten_hours_ago),
            },
            FileEntry {
                absolute_path: PathBuf::from("/file2.rs"),
                relative_path: "file2.rs".to_string(),
                modified_time: Some(one_hour_ago),
            },
            FileEntry {
                absolute_path: PathBuf::from("/file3.rs"),
                relative_path: "file3.rs".to_string(),
                modified_time: Some(five_hours_ago),
            },
        ];

        sort_files_two_tier(&mut files);

        // All recent, should be sorted newest first
        assert_eq!(files[0].relative_path, "file2.rs"); // 1 hour ago
        assert_eq!(files[1].relative_path, "file3.rs"); // 5 hours ago
        assert_eq!(files[2].relative_path, "file1.rs"); // 10 hours ago
    }

    #[test]
    fn test_timestamp_boundary_exactly_24_hours() {
        let now = SystemTime::now();
        let exactly_24h = now - Duration::from_secs(24 * 3600);
        let just_under_24h = now - Duration::from_secs(24 * 3600 - 60);
        let just_over_24h = now - Duration::from_secs(24 * 3600 + 60);

        let mut files = vec![
            FileEntry {
                absolute_path: PathBuf::from("/b_exactly.rs"),
                relative_path: "b_exactly.rs".to_string(),
                modified_time: Some(exactly_24h),
            },
            FileEntry {
                absolute_path: PathBuf::from("/a_under.rs"),
                relative_path: "a_under.rs".to_string(),
                modified_time: Some(just_under_24h),
            },
            FileEntry {
                absolute_path: PathBuf::from("/c_over.rs"),
                relative_path: "c_over.rs".to_string(),
                modified_time: Some(just_over_24h),
            },
        ];

        sort_files_two_tier(&mut files);

        // just_under is recent (comes first)
        assert_eq!(files[0].relative_path, "a_under.rs");
        // The other two are old, sorted alphabetically
        assert_eq!(files[1].relative_path, "b_exactly.rs");
        assert_eq!(files[2].relative_path, "c_over.rs");
    }

    #[test]
    fn test_files_without_mtime() {
        let now = SystemTime::now();
        let recent = now - Duration::from_secs(3600);

        let mut files = vec![
            FileEntry {
                absolute_path: PathBuf::from("/z_no_time.rs"),
                relative_path: "z_no_time.rs".to_string(),
                modified_time: None,
            },
            FileEntry {
                absolute_path: PathBuf::from("/a_recent.rs"),
                relative_path: "a_recent.rs".to_string(),
                modified_time: Some(recent),
            },
            FileEntry {
                absolute_path: PathBuf::from("/b_no_time.rs"),
                relative_path: "b_no_time.rs".to_string(),
                modified_time: None,
            },
        ];

        sort_files_two_tier(&mut files);

        // Recent file first
        assert_eq!(files[0].relative_path, "a_recent.rs");
        // Files without mtime are treated as old, sorted alphabetically
        assert_eq!(files[1].relative_path, "b_no_time.rs");
        assert_eq!(files[2].relative_path, "z_no_time.rs");
    }

    // ============================================================================
    // P1: Limit selection tests (heap behavior)
    // ============================================================================

    #[tokio::test]
    async fn test_limit_selects_alphabetically_first_old_files() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();
        let now = SystemTime::now();
        let old_time = now - Duration::from_secs(48 * 3600); // 2 days ago

        // Create 10 old files with names a.rs through j.rs
        for ch in 'a'..='j' {
            let path = dir.join(format!("{ch}.rs"));
            fs::write(&path, "content").unwrap();
            // Set modification time to 2 days ago
            filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(old_time))
                .unwrap();
        }

        // Request only 3 files
        let files = find_matching_files("*.rs", dir, 3, false).await.unwrap();

        // Should keep alphabetically first 3 old files
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].relative_path, "a.rs");
        assert_eq!(files[1].relative_path, "b.rs");
        assert_eq!(files[2].relative_path, "c.rs");
    }

    #[tokio::test]
    async fn test_limit_prioritizes_recent_over_old() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();
        let now = SystemTime::now();

        // Create 3 recent files (within 24h)
        for i in 0..3 {
            let path = dir.join(format!("recent{i}.rs"));
            fs::write(&path, "content").unwrap();
            let recent_time = now - Duration::from_secs(3600 * (i as u64 + 1)); // 1h, 2h, 3h ago
            filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(recent_time))
                .unwrap();
        }

        // Create 10 old files (>24h) named old_a.rs through old_j.rs
        let old_time = now - Duration::from_secs(48 * 3600);
        for ch in 'a'..='j' {
            let path = dir.join(format!("old_{ch}.rs"));
            fs::write(&path, "content").unwrap();
            filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(old_time))
                .unwrap();
        }

        // Request 5 files total
        let files = find_matching_files("*.rs", dir, 5, false).await.unwrap();

        // Should have: 3 recent (sorted by mtime, newest first) + 2 old (alphabetically first)
        assert_eq!(files.len(), 5);

        // First 3 should be recent files, sorted newest first
        assert_eq!(files[0].relative_path, "recent0.rs"); // 1h ago (newest)
        assert_eq!(files[1].relative_path, "recent1.rs"); // 2h ago
        assert_eq!(files[2].relative_path, "recent2.rs"); // 3h ago

        // Last 2 should be alphabetically first old files
        assert_eq!(files[3].relative_path, "old_a.rs");
        assert_eq!(files[4].relative_path, "old_b.rs");
    }

    // ============================================================================
    // P1: Error handling tests
    // ============================================================================

    #[tokio::test]
    async fn test_invalid_glob_pattern() {
        let temp = tempdir().expect("create temp dir");
        // Unclosed character class is invalid
        let result = find_matching_files("[abc", temp.path(), 10, false).await;

        assert!(
            matches!(result, Err(FunctionCallError::RespondToModel(_))),
            "Invalid glob pattern should be rejected"
        );
    }

    // ============================================================================
    // P2: Edge cases
    // ============================================================================

    #[tokio::test]
    async fn test_empty_directory() {
        let temp = tempdir().expect("create temp dir");
        let files = find_matching_files("*.rs", temp.path(), 100, false)
            .await
            .unwrap();
        assert_eq!(files.len(), 0);
    }

    #[tokio::test]
    async fn test_no_matches() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        fs::write(dir.join("test.txt"), "content").unwrap();
        fs::write(dir.join("main.js"), "content").unwrap();

        let files = find_matching_files("*.rs", dir, 100, false).await.unwrap();
        assert_eq!(files.len(), 0);
    }

    #[tokio::test]
    async fn test_nested_directories() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        fs::create_dir_all(dir.join("a/b/c/d")).unwrap();
        fs::write(dir.join("a/file1.rs"), "").unwrap();
        fs::write(dir.join("a/b/file2.rs"), "").unwrap();
        fs::write(dir.join("a/b/c/file3.rs"), "").unwrap();
        fs::write(dir.join("a/b/c/d/file4.rs"), "").unwrap();

        let files = find_matching_files("**/*.rs", dir, 100, false)
            .await
            .unwrap();

        assert_eq!(files.len(), 4);
    }

    #[tokio::test]
    async fn test_special_characters_in_pattern() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        fs::write(dir.join("test1.rs"), "").unwrap();
        fs::write(dir.join("test2.rs"), "").unwrap();
        fs::write(dir.join("test3.rs"), "").unwrap();

        // Test character set
        let files = find_matching_files("test[12].rs", dir, 100, false)
            .await
            .unwrap();

        assert_eq!(files.len(), 2);
        let paths: Vec<_> = files.iter().map(|f| &f.relative_path).collect();
        assert!(paths.contains(&&"test1.rs".to_string()));
        assert!(paths.contains(&&"test2.rs".to_string()));
        assert!(!paths.contains(&&"test3.rs".to_string()));
    }

    #[tokio::test]
    async fn test_question_mark_wildcard() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        fs::write(dir.join("test1.rs"), "").unwrap();
        fs::write(dir.join("test22.rs"), "").unwrap();
        fs::write(dir.join("test3.rs"), "").unwrap();

        // ? matches exactly one character
        let files = find_matching_files("test?.rs", dir, 100, false)
            .await
            .unwrap();

        assert_eq!(files.len(), 2);
        let paths: Vec<_> = files.iter().map(|f| &f.relative_path).collect();
        assert!(paths.contains(&&"test1.rs".to_string()));
        assert!(paths.contains(&&"test3.rs".to_string()));
        assert!(!paths.contains(&&"test22.rs".to_string()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_symlinks_not_followed() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        // Create a directory and a real file
        fs::create_dir(dir.join("realdir")).unwrap();
        fs::write(dir.join("realdir/file.rs"), "").unwrap();

        // Create symlink to the directory
        let link_path = dir.join("linkdir");
        symlink(dir.join("realdir"), &link_path).unwrap();

        let files = find_matching_files("**/*.rs", dir, 100, false)
            .await
            .unwrap();

        // With follow_links=false, should only find realdir/file.rs (not linkdir/file.rs)
        // The file should be found exactly once
        let paths: Vec<_> = files.iter().map(|f| &f.relative_path).collect();
        assert!(
            paths.contains(&&"realdir/file.rs".to_string()),
            "Should find realdir/file.rs"
        );
        // linkdir should not be traversed
        assert!(
            !paths.iter().any(|p| p.starts_with("linkdir")),
            "Should not traverse symlinked directory"
        );
    }

    #[tokio::test]
    async fn test_hidden_files_with_gitignore() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        // Don't ignore hidden files by default
        fs::write(dir.join(".hidden.rs"), "").unwrap();
        fs::write(dir.join("normal.rs"), "").unwrap();

        let files = find_matching_files("*.rs", dir, 100, false).await.unwrap();

        // Should find both
        assert_eq!(files.len(), 2);

        // Now create .gitignore to ignore hidden files
        fs::write(dir.join(".gitignore"), ".*\n").unwrap();

        let files2 = find_matching_files("*.rs", dir, 100, false).await.unwrap();

        // Should only find normal.rs (.hidden.rs is ignored)
        assert_eq!(files2.len(), 1);
        assert_eq!(files2[0].relative_path, "normal.rs");
    }

    #[tokio::test]
    async fn test_multiple_extensions() {
        let temp = tempdir().expect("create temp dir");
        let dir = temp.path();

        fs::write(dir.join("file.rs"), "").unwrap();
        fs::write(dir.join("file.toml"), "").unwrap();
        fs::write(dir.join("file.txt"), "").unwrap();

        // Test multiple patterns (brace expansion)
        let files = find_matching_files("*.{rs,toml}", dir, 100, false)
            .await
            .unwrap();

        assert_eq!(files.len(), 2);
        let paths: Vec<_> = files.iter().map(|f| &f.relative_path).collect();
        assert!(paths.contains(&&"file.rs".to_string()));
        assert!(paths.contains(&&"file.toml".to_string()));
        assert!(!paths.contains(&&"file.txt".to_string()));
    }

    // ============================================================================
    // Output format tests
    // ============================================================================

    #[test]
    fn test_format_output_success() {
        let files = vec![
            FileEntry {
                absolute_path: PathBuf::from("/tmp/test/file1.rs"),
                relative_path: "file1.rs".to_string(),
                modified_time: None,
            },
            FileEntry {
                absolute_path: PathBuf::from("/tmp/test/file2.rs"),
                relative_path: "file2.rs".to_string(),
                modified_time: None,
            },
        ];

        let result = format_output(files, "*.rs", 100).unwrap();

        match result {
            ToolOutput::Function {
                content,
                success: Some(true),
                ..
            } => {
                assert!(content.contains("Found 2 file(s) matching \"*.rs\""));
                assert!(content.contains("/tmp/test/file1.rs"));
                assert!(content.contains("/tmp/test/file2.rs"));
                assert!(!content.contains("Results limited to"));
            }
            _ => panic!("Expected successful ToolOutput::Function"),
        }
    }

    #[test]
    fn test_format_output_truncation_message() {
        let files: Vec<FileEntry> = (0..100)
            .map(|i| FileEntry {
                absolute_path: PathBuf::from(format!("/tmp/file{i}.rs")),
                relative_path: format!("file{i}.rs"),
                modified_time: None,
            })
            .collect();

        let result = format_output(files, "*.rs", 100).unwrap();

        match result {
            ToolOutput::Function { content, .. } => {
                assert!(content.contains("Found 100 file(s) matching \"*.rs\""));
                assert!(
                    content.contains("(Results limited to 100"),
                    "Should show truncation warning when len == limit"
                );
            }
            _ => panic!("Expected ToolOutput::Function"),
        }
    }

    #[test]
    fn test_format_output_no_truncation_when_under_limit() {
        let files: Vec<FileEntry> = (0..50)
            .map(|i| FileEntry {
                absolute_path: PathBuf::from(format!("/tmp/file{i}.rs")),
                relative_path: format!("file{i}.rs"),
                modified_time: None,
            })
            .collect();

        let result = format_output(files, "*.rs", 100).unwrap();

        match result {
            ToolOutput::Function { content, .. } => {
                assert!(content.contains("Found 50 file(s) matching \"*.rs\""));
                assert!(
                    !content.contains("Results limited to"),
                    "Should NOT show truncation warning when len < limit"
                );
            }
            _ => panic!("Expected ToolOutput::Function"),
        }
    }

    #[test]
    fn test_format_output_no_matches() {
        let files = vec![];

        let result = format_output(files, "*.xyz", 100).unwrap();

        match result {
            ToolOutput::Function {
                content,
                success: Some(false),
                ..
            } => {
                assert_eq!(content, "No files found matching pattern \"*.xyz\"");
            }
            _ => panic!("Expected unsuccessful ToolOutput::Function"),
        }
    }
}
