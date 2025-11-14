//! 文件操作工具
//!
//! 提供内容哈希、行尾检测、换行符恢复等功能

use sha2::Digest;
use sha2::Sha256;

/// 计算文件内容的 SHA256 hash
///
/// 用于检测并发文件修改
///
/// # Examples
/// ```
/// # use codex_core::tools::handlers::edit::common::file_ops::hash_content;
/// let hash = hash_content("hello world");
/// assert_eq!(hash.len(), 64); // SHA256 is 64 hex chars
/// ```
pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 检测文件的行尾风格
///
/// # Returns
/// - `"\r\n"` 如果文件使用 CRLF（Windows）
/// - `"\n"` 如果文件使用 LF（Unix/Mac）
///
/// # Examples
/// ```
/// # use codex_core::tools::handlers::edit::common::file_ops::detect_line_ending;
/// assert_eq!(detect_line_ending("line1\nline2\n"), "\n");
/// assert_eq!(detect_line_ending("line1\r\nline2\r\n"), "\r\n");
/// ```
pub fn detect_line_ending(content: &str) -> &'static str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// 恢复原始文件的尾部换行符
///
/// 确保编辑后文件的换行符状态与原文件一致
///
/// # Examples
/// ```
/// # use codex_core::tools::handlers::edit::common::file_ops::restore_trailing_newline;
/// let original = "content\n";
/// let modified = "new content";
/// let result = restore_trailing_newline(original, modified);
/// assert_eq!(result, "new content\n");
/// ```
pub fn restore_trailing_newline(original: &str, modified: &str) -> String {
    let had_trailing = original.ends_with('\n');
    let has_trailing = modified.ends_with('\n');

    match (had_trailing, has_trailing) {
        (true, false) => format!("{modified}\n"),
        (false, true) => modified.trim_end_matches('\n').to_string(),
        _ => modified.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_content() {
        let content1 = "hello world";
        let content2 = "hello world";
        let content3 = "different";

        let hash1 = hash_content(content1);
        let hash2 = hash_content(content2);
        let hash3 = hash_content(content3);

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // SHA256 is 64 hex chars
    }

    #[test]
    fn test_detect_line_ending() {
        assert_eq!(detect_line_ending("line1\nline2\n"), "\n");
        assert_eq!(detect_line_ending("line1\r\nline2\r\n"), "\r\n");
        assert_eq!(detect_line_ending("no newline"), "\n");
        assert_eq!(detect_line_ending("mixed\nand\r\nlines"), "\r\n"); // CRLF takes precedence
    }

    #[test]
    fn test_restore_trailing_newline() {
        // Had trailing, modified doesn't
        assert_eq!(restore_trailing_newline("a\n", "b"), "b\n");

        // Didn't have trailing, modified does
        assert_eq!(restore_trailing_newline("a", "b\n"), "b");

        // Both have trailing
        assert_eq!(restore_trailing_newline("a\n", "b\n"), "b\n");

        // Neither has trailing
        assert_eq!(restore_trailing_newline("a", "b"), "b");

        // Empty strings
        assert_eq!(restore_trailing_newline("", ""), "");
        assert_eq!(restore_trailing_newline("\n", ""), "\n");
    }

    #[test]
    fn test_restore_trailing_newline_multiple() {
        // 函数只恢复单个尾部换行符，不保留多个
        // 原文件有换行符（无论多少个），modified没有 → 添加一个
        assert_eq!(restore_trailing_newline("a\n\n\n", "b"), "b\n");
        assert_eq!(restore_trailing_newline("a\n\n", "b"), "b\n");

        // CRLF情况 - 函数检测 \n，不区分 CRLF vs LF
        // 原文件以 \r\n 结尾（ends_with('\n') = true）
        assert_eq!(restore_trailing_newline("a\r\n", "b"), "b\n");
        assert_eq!(restore_trailing_newline("a\r\n\r\n", "b"), "b\n");
    }

    #[test]
    fn test_detect_line_ending_edge_cases() {
        // 只有CRLF，无LF
        assert_eq!(detect_line_ending("\r\n\r\n\r\n"), "\r\n");

        // 空字符串默认LF
        assert_eq!(detect_line_ending(""), "\n");

        // 只有CR（不完整的CRLF）
        assert_eq!(detect_line_ending("line1\rline2"), "\n");
    }

    #[test]
    fn test_hash_content_consistency() {
        // 相同内容多次hash应该产生相同结果
        let content = "test content\nwith multiple\nlines";
        let hash1 = hash_content(content);
        let hash2 = hash_content(content);
        let hash3 = hash_content(content);

        assert_eq!(hash1, hash2);
        assert_eq!(hash2, hash3);

        // 微小差异应该产生不同hash
        let content_with_space = "test content\nwith multiple\nlines ";
        let hash_different = hash_content(content_with_space);
        assert_ne!(hash1, hash_different);
    }
}
