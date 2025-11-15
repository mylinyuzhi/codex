//! 文本处理工具
//!
//! 提供字符串替换、转义处理、匹配计数等功能

use regex_lite::Regex;
use std::sync::LazyLock;

/// 安全的字面量替换
///
/// Rust 的 `str::replace` 已经是字面量替换，不会像 JavaScript 那样
/// 解释 $ 特殊序列。这个函数保持接口一致性，未来可能添加特殊处理。
///
/// # Examples
/// ```
/// # use codex_core::tools::handlers::edit::common::text_utils::safe_literal_replace;
/// let result = safe_literal_replace("price is $50", "$50", "$100");
/// assert_eq!(result, "price is $100");
/// ```
pub fn safe_literal_replace(content: &str, old: &str, new: &str) -> String {
    content.replace(old, new)
}

/// 计算精确匹配次数
///
/// # Examples
/// ```
/// # use codex_core::tools::handlers::edit::common::text_utils::exact_match_count;
/// let count = exact_match_count("hello hello world", "hello");
/// assert_eq!(count, 2);
/// ```
pub fn exact_match_count(content: &str, pattern: &str) -> i32 {
    content.matches(pattern).count() as i32
}

/// 反转义字符串（处理 LLM 过度转义问题）
///
/// LLM 经常过度转义字符串，将真实的换行符写成 `\\n`。
/// 这个函数尝试将常见的转义序列还原为真实字符。
///
/// **实现说明：** 使用正则表达式匹配一个或多个反斜杠后跟特殊字符，
/// 与 gemini-cli 的 `unescapeStringForGeminiBug` 对齐。
/// 正则 `\\+(n|t|r|'|"|`|\\|\n)` 匹配：
/// - `\\+`: 一个或多个反斜杠
/// - `(n|t|r|...)`: 后跟的特殊字符
///
/// # Examples
/// ```
/// # use codex_core::tools::handlers::edit::common::text_utils::unescape_string;
/// assert_eq!(unescape_string("hello\\nworld"), "hello\nworld");
/// assert_eq!(unescape_string("tab\\there"), "tab\there");
/// assert_eq!(unescape_string("quote\\\"test"), "quote\"test");
/// // 处理多个连续反斜杠
/// assert_eq!(unescape_string("\\\\\\ntest"), "\ntest");
/// ```
pub fn unescape_string(s: &str) -> String {
    static UNESCAPE_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"\\+(n|t|r|'|"|`|\\|\n)"#).expect("Invalid unescape regex"));

    UNESCAPE_RE
        .replace_all(s, |caps: &regex_lite::Captures| {
            let captured_char = &caps[1];
            match captured_char {
                "n" => "\n".to_string(),
                "t" => "\t".to_string(),
                "r" => "\r".to_string(),
                "'" => "'".to_string(),
                "\"" => "\"".to_string(),
                "`" => "`".to_string(),
                "\\" => "\\".to_string(),
                "\n" => "\n".to_string(), // 实际换行符
                _ => caps[0].to_string(), // 不应到达，但保持安全
            }
        })
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_literal_replace() {
        // 基本替换
        assert_eq!(safe_literal_replace("abc", "b", "x"), "axc");

        // $ 字符（Rust 原生 replace 是字面量）
        assert_eq!(safe_literal_replace("$50", "$50", "$100"), "$100");

        // 多次替换
        assert_eq!(safe_literal_replace("a a a", "a", "b"), "b b b");

        // 无匹配
        assert_eq!(safe_literal_replace("test", "x", "y"), "test");

        // 空字符串
        assert_eq!(safe_literal_replace("", "a", "b"), "");
    }

    #[test]
    fn test_exact_match_count() {
        assert_eq!(exact_match_count("abc abc abc", "abc"), 3);
        assert_eq!(exact_match_count("hello", "world"), 0);
        assert_eq!(exact_match_count("", "x"), 0);

        // Rust 的 matches 不重叠
        assert_eq!(exact_match_count("aaa", "aa"), 1); // Non-overlapping matches

        // 特殊字符
        assert_eq!(exact_match_count("$50 $100", "$"), 2);
    }

    #[test]
    fn test_unescape_string() {
        // 换行符
        assert_eq!(unescape_string("line1\\nline2"), "line1\nline2");

        // Tab
        assert_eq!(unescape_string("col1\\tcol2"), "col1\tcol2");

        // 引号
        assert_eq!(unescape_string("say \\\"hi\\\""), "say \"hi\"");
        assert_eq!(unescape_string("say \\'hi\\'"), "say 'hi'");

        // 回车
        assert_eq!(unescape_string("a\\rb"), "a\rb");

        // 反斜杠
        assert_eq!(unescape_string("path\\\\file"), "path\\file");

        // 混合
        assert_eq!(unescape_string("a\\nb\\tc\\\"d"), "a\nb\tc\"d");

        // 无需转义
        assert_eq!(unescape_string("plain text"), "plain text");

        // 多个连续反斜杠（与 gemini-cli 对齐）
        // \\n (2 backslashes + n) → \n (newline)
        assert_eq!(unescape_string("\\\\n"), "\n");
        // \\\\n (4 backslashes + n) → \n (newline)
        assert_eq!(unescape_string("\\\\\\\\n"), "\n");
        // \\\n (3 backslashes + n) → \n (newline)
        assert_eq!(unescape_string("\\\\\\n"), "\n");

        // 反引号
        assert_eq!(unescape_string("code\\`block"), "code`block");

        // 实际换行符后的反斜杠
        assert_eq!(unescape_string("test\\\nmore"), "test\nmore");
    }

    #[test]
    fn test_unescape_backslash_only() {
        // 单独的反斜杠不应该被改变（没有后续的特殊字符）
        assert_eq!(unescape_string("\\"), "\\");
        assert_eq!(unescape_string("a\\b"), "a\\b"); // \b不在我们的列表中
    }

    #[test]
    fn test_unescape_unsupported_escape_sequences() {
        // \f, \x 等不在我们列表中的转义序列不应该改变
        // 注意：\t, \n, \r 等是支持的，会被转义
        assert_eq!(unescape_string("\\f"), "\\f");
        assert_eq!(unescape_string("\\x"), "\\x");
        assert_eq!(unescape_string("\\v"), "\\v");
        assert_eq!(unescape_string("\\b"), "\\b");
        // 使用不支持的字符 'o' 和 'i'，而非 't' (tab) 和 'n' (newline)
        assert_eq!(unescape_string("path\\on\\file"), "path\\on\\file");
    }

    #[test]
    fn test_unescape_real_newline_not_affected() {
        // 真实的换行符（不是\\n）不应该被改变
        let input = "hello\nworld"; // 真实\n
        let result = unescape_string(input);
        assert_eq!(
            result, input,
            "Real newlines should not be affected by unescape"
        );

        // 混合真实换行符和转义换行符
        let mixed = "line1\nline2\\nline3"; // 真实\n + 转义\n
        assert_eq!(unescape_string(mixed), "line1\nline2\nline3");
    }

    #[test]
    fn test_unescape_empty_string() {
        assert_eq!(unescape_string(""), "");
    }

    #[test]
    fn test_unescape_no_escape_sequences() {
        // 完全没有转义序列的字符串
        let input = "This is a normal string with no escapes!";
        assert_eq!(unescape_string(input), input);
    }
}
