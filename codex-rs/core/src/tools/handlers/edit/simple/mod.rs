//! 简单精确编辑器
//!
//! 特点：
//! - 精确字符串匹配（逐字符比较）
//! - 自动反转义尝试（处理 LLM 过度转义）
//! - 简单 LLM 纠错（无需 instruction）
//! - 文件修改检测（hash-based）

use async_trait::async_trait;
use serde::Deserialize;
use std::fs;

use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;

use super::common;

pub(crate) mod correction;

/// 简单编辑器 Handler
pub struct EditHandler;

/// 编辑参数
#[derive(Debug, Clone, Deserialize)]
struct EditArgs {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default = "default_expected_replacements")]
    expected_replacements: i32,
}

fn default_expected_replacements() -> i32 {
    1
}

/// 替换结果
struct ReplacementResult {
    new_content: String,
    occurrences: i32,
}

#[async_trait]
impl ToolHandler for EditHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let arguments = match &invocation.payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "edit requires Function payload".into(),
                ));
            }
        };

        let args: EditArgs = serde_json::from_str(arguments)
            .map_err(|e| FunctionCallError::RespondToModel(format!("Invalid arguments: {e}")))?;

        // 验证
        validate_args(&args)?;

        let file_path = invocation.turn.resolve_path(Some(args.file_path.clone()));

        // 处理文件读取/创建
        let (content, line_ending) = match read_or_create_file(&file_path, &args)? {
            FileState::Created => {
                return Ok(ToolOutput::Function {
                    content: format!("Created new file: {}", args.file_path),
                    content_items: None,
                    success: Some(true),
                });
            }
            FileState::Existing {
                content,
                line_ending,
            } => (content, line_ending),
        };

        // 规范化换行符
        let normalized_content = content.replace("\r\n", "\n");
        let normalized_old = args.old_string.replace("\r\n", "\n");
        let normalized_new = args.new_string.replace("\r\n", "\n");

        // 计算初始 hash（并发修改检测）
        let initial_hash = common::hash_content(&normalized_content);

        // 阶段1：精确匹配
        let result = try_exact_replacement(&normalized_old, &normalized_new, &normalized_content);

        if is_success(&result, args.expected_replacements) {
            // Phase 1 成功，但检查 new_string 是否有转义问题（对齐 gemini-cli）
            let new_appears_escaped =
                common::unescape_string(&normalized_new) != normalized_new;

            if new_appears_escaped {
                // new_string 有转义问题，调用 LLM 修正
                let corrected_new = correction::correct_new_string_escaping(
                    &invocation.turn.client,
                    &normalized_old,
                    &normalized_new,
                    40, // 40秒超时
                )
                .await
                .map_err(|e| {
                    FunctionCallError::RespondToModel(format!(
                        "Edit succeeded but new_string has escaping issues. LLM correction failed: {e}"
                    ))
                })?;

                // 使用修正后的 new_string 重新替换
                let final_result =
                    try_exact_replacement(&normalized_old, &corrected_new, &normalized_content);
                return write_and_respond(
                    &file_path,
                    &final_result.new_content,
                    line_ending,
                    &args,
                    "exact-corrected-new",
                );
            }

            return write_and_respond(
                &file_path,
                &result.new_content,
                line_ending,
                &args,
                "exact",
            );
        }

        // 阶段2：反转义尝试 old_string（与 gemini-cli 对齐）
        let unescaped_old = common::unescape_string(&normalized_old);
        if unescaped_old != normalized_old {
            // 先用原始 new_string 尝试（不反转义）
            let result =
                try_exact_replacement(&unescaped_old, &normalized_new, &normalized_content);

            if is_success(&result, args.expected_replacements) {
                // 成功了！但检查 new_string 是否也需要适配
                let new_appears_escaped =
                    common::unescape_string(&normalized_new) != normalized_new;

                if new_appears_escaped {
                    // new_string 看起来被转义，调用 LLM 适配
                    let adapted_new = correction::adapt_new_string(
                        &invocation.turn.client,
                        &normalized_old,
                        &unescaped_old,
                        &normalized_new,
                        40, // 40秒超时
                    )
                    .await
                    .map_err(|e| {
                        FunctionCallError::RespondToModel(format!(
                            "Phase 2 succeeded but new_string adaptation failed: {e}"
                        ))
                    })?;

                    // 使用适配后的 new_string 重新替换
                    let final_result =
                        try_exact_replacement(&unescaped_old, &adapted_new, &normalized_content);
                    return write_and_respond(
                        &file_path,
                        &final_result.new_content,
                        line_ending,
                        &args,
                        "unescaped-adapted",
                    );
                }

                return write_and_respond(
                    &file_path,
                    &result.new_content,
                    line_ending,
                    &args,
                    "unescaped",
                );
            }
        }

        // 阶段3：检测并发修改 + LLM 纠错
        let (content_for_llm, error_msg) = detect_concurrent_modification(
            &file_path,
            &content,
            &initial_hash,
            &result,
            args.expected_replacements,
        )?;

        let corrected = correction::attempt_correction(
            &invocation.turn.client,
            &normalized_old,
            &normalized_new,
            &content_for_llm,
            &error_msg,
            40, // 40秒超时
        )
        .await
        .map_err(|e| {
            FunctionCallError::RespondToModel(format!(
                "Edit failed: {}. LLM correction also failed: {e}",
                error_msg
            ))
        })?;

        // 检查是否无需更改
        if corrected.no_changes_required {
            return Ok(ToolOutput::Function {
                content: format!(
                    "No changes required for {}.\nExplanation: {}",
                    args.file_path, corrected.explanation
                ),
                content_items: None,
                success: Some(true),
            });
        }

        // 阶段4：使用纠正后的参数重试
        let retry_result =
            try_exact_replacement(&corrected.search, &corrected.replace, &content_for_llm);

        if is_success(&retry_result, args.expected_replacements) {
            write_and_respond_with_explanation(
                &file_path,
                &retry_result.new_content,
                line_ending,
                &args,
                &corrected.explanation,
            )
        } else {
            Err(FunctionCallError::RespondToModel(format!(
                "Edit failed even after LLM correction: found {} occurrences.\nExplanation: {}",
                retry_result.occurrences, corrected.explanation
            )))
        }
    }
}

// ========== 辅助函数 ==========

fn validate_args(args: &EditArgs) -> Result<(), FunctionCallError> {
    if args.expected_replacements < 1 {
        return Err(FunctionCallError::RespondToModel(
            "expected_replacements must be at least 1".into(),
        ));
    }

    if args.old_string == args.new_string {
        return Err(FunctionCallError::RespondToModel(
            "No changes: old_string equals new_string".into(),
        ));
    }

    Ok(())
}

enum FileState {
    Created,
    Existing {
        content: String,
        line_ending: &'static str,
    },
}

fn read_or_create_file(
    file_path: &std::path::Path,
    args: &EditArgs,
) -> Result<FileState, FunctionCallError> {
    match fs::read_to_string(file_path) {
        Ok(content) => {
            if args.old_string.is_empty() {
                return Err(FunctionCallError::RespondToModel(format!(
                    "File already exists: {}. Use non-empty old_string to edit.",
                    args.file_path
                )));
            }
            let line_ending = common::detect_line_ending(&content);
            Ok(FileState::Existing {
                content,
                line_ending,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if args.old_string.is_empty() {
                // 创建新文件
                if let Some(parent) = file_path.parent() {
                    fs::create_dir_all(parent).map_err(|e| {
                        FunctionCallError::RespondToModel(format!(
                            "Failed to create directories: {e}"
                        ))
                    })?;
                }
                fs::write(file_path, &args.new_string).map_err(|e| {
                    FunctionCallError::RespondToModel(format!("Failed to write file: {e}"))
                })?;
                Ok(FileState::Created)
            } else {
                Err(FunctionCallError::RespondToModel(format!(
                    "File not found: {}. Use empty old_string to create.",
                    args.file_path
                )))
            }
        }
        Err(e) => Err(FunctionCallError::RespondToModel(format!(
            "Failed to read file: {e}"
        ))),
    }
}

fn try_exact_replacement(old: &str, new: &str, content: &str) -> ReplacementResult {
    let occurrences = common::exact_match_count(content, old);
    let new_content = if occurrences > 0 {
        common::safe_literal_replace(content, old, new)
    } else {
        content.to_string()
    };

    ReplacementResult {
        new_content,
        occurrences,
    }
}

fn is_success(result: &ReplacementResult, expected: i32) -> bool {
    result.occurrences == expected && result.occurrences > 0
}

fn detect_concurrent_modification(
    file_path: &std::path::Path,
    original_content: &str,
    initial_hash: &str,
    result: &ReplacementResult,
    expected: i32,
) -> Result<(String, String), FunctionCallError> {
    let error_msg = format!(
        "Found {} occurrences (expected {})",
        result.occurrences, expected
    );

    let on_disk_content = fs::read_to_string(file_path)
        .map_err(|e| FunctionCallError::RespondToModel(format!("Failed to re-read file: {e}")))?;

    let on_disk_hash = common::hash_content(&on_disk_content);

    if initial_hash != on_disk_hash {
        Ok((
            on_disk_content,
            format!(
                "File modified externally. Using latest version. Original error: {}",
                error_msg
            ),
        ))
    } else {
        Ok((original_content.to_string(), error_msg))
    }
}

fn write_and_respond(
    file_path: &std::path::Path,
    content: &str,
    line_ending: &'static str,
    args: &EditArgs,
    strategy: &str,
) -> Result<ToolOutput, FunctionCallError> {
    let final_content = if line_ending == "\r\n" {
        content.replace('\n', "\r\n")
    } else {
        content.to_string()
    };

    fs::write(file_path, &final_content)
        .map_err(|e| FunctionCallError::RespondToModel(format!("Failed to write file: {e}")))?;

    Ok(ToolOutput::Function {
        content: format!(
            "Successfully edited {} (strategy: {})",
            args.file_path, strategy
        ),
        content_items: None,
        success: Some(true),
    })
}

fn write_and_respond_with_explanation(
    file_path: &std::path::Path,
    content: &str,
    line_ending: &'static str,
    args: &EditArgs,
    explanation: &str,
) -> Result<ToolOutput, FunctionCallError> {
    let final_content = if line_ending == "\r\n" {
        content.replace('\n', "\r\n")
    } else {
        content.to_string()
    };

    fs::write(file_path, &final_content)
        .map_err(|e| FunctionCallError::RespondToModel(format!("Failed to write file: {e}")))?;

    Ok(ToolOutput::Function {
        content: format!(
            "Successfully edited {} with LLM correction\nExplanation: {}",
            args.file_path, explanation
        ),
        content_items: None,
        success: Some(true),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_args() {
        let valid = EditArgs {
            file_path: "test.txt".into(),
            old_string: "old".into(),
            new_string: "new".into(),
            expected_replacements: 1,
        };
        assert!(validate_args(&valid).is_ok());

        let invalid_count = EditArgs {
            expected_replacements: 0,
            ..valid.clone()
        };
        assert!(validate_args(&invalid_count).is_err());

        let invalid_same = EditArgs {
            old_string: "same".into(),
            new_string: "same".into(),
            ..valid
        };
        assert!(validate_args(&invalid_same).is_err());
    }

    #[test]
    fn test_try_exact_replacement() {
        let result = try_exact_replacement("old", "new", "old old");
        assert_eq!(result.occurrences, 2);
        assert_eq!(result.new_content, "new new");

        let no_match = try_exact_replacement("notfound", "x", "content");
        assert_eq!(no_match.occurrences, 0);
        assert_eq!(no_match.new_content, "content");
    }

    #[test]
    fn test_is_success() {
        let result = ReplacementResult {
            new_content: "test".into(),
            occurrences: 2,
        };
        assert!(is_success(&result, 2));
        assert!(!is_success(&result, 1));
        assert!(!is_success(&result, 3));
    }

    #[test]
    fn test_default_expected_replacements() {
        assert_eq!(default_expected_replacements(), 1);
    }

    // ========== Phase 2 逻辑测试 ==========

    #[test]
    fn test_phase2_new_string_not_escaped_should_not_change() {
        // old 需要反转义，new 正确不需要
        let normalized_old = "\\nconst x = 1;";
        let normalized_new = "\nconst y = 2;"; // 已经是正确的换行符

        let unescaped_old = common::unescape_string(normalized_old);
        assert_eq!(unescaped_old, "\nconst x = 1;");

        // new 应该保持不变
        let new_appears_escaped = common::unescape_string(normalized_new) != normalized_new;
        assert!(
            !new_appears_escaped,
            "new_string should not appear escaped"
        );
    }

    #[test]
    fn test_phase2_both_strings_appear_escaped() {
        // 两个都看起来被转义
        let normalized_old = "hello\\nworld";
        let normalized_new = "hi\\nthere";

        let old_appears_escaped = common::unescape_string(normalized_old) != normalized_old;
        let new_appears_escaped = common::unescape_string(normalized_new) != normalized_new;

        assert!(old_appears_escaped);
        assert!(new_appears_escaped);
        // 这种情况需要 LLM 适配
    }

    #[test]
    fn test_unescape_does_not_change_correct_strings() {
        // 确保反转义不会破坏已经正确的字符串
        let correct = "hello\nworld"; // 真正的换行符
        let result = common::unescape_string(correct);
        assert_eq!(
            result, correct,
            "Should not change already-correct strings"
        );
    }

    #[test]
    fn test_unescape_handles_multiple_backslashes() {
        // \\n (2 backslashes + n) → \n
        assert_eq!(common::unescape_string("\\\\n"), "\n");

        // \\\\n (4 backslashes + n) → \n
        assert_eq!(common::unescape_string("\\\\\\\\n"), "\n");

        // \\\n (3 backslashes + n) → \n
        assert_eq!(common::unescape_string("\\\\\\n"), "\n");
    }

    #[test]
    fn test_unescape_mixed_escaping() {
        // 混合：有的需要反转义，有的不需要
        let input = "hello\\nworld\ntest"; // \\n + 真正的\n
        let result = common::unescape_string(input);
        assert_eq!(
            result, "hello\nworld\ntest",
            "Should handle mixed escaping"
        );
    }

    #[test]
    fn test_new_string_appears_escaped_detection() {
        // 测试检测逻辑
        let escaped_new = "function test()\\n{"; // 有转义
        let correct_new = "function test()\n{"; // 正确

        let escaped_appears = common::unescape_string(escaped_new) != escaped_new;
        let correct_appears = common::unescape_string(correct_new) != correct_new;

        assert!(escaped_appears, "Should detect escaping in escaped_new");
        assert!(
            !correct_appears,
            "Should not detect escaping in correct_new"
        );
    }

    #[test]
    fn test_backtick_escaping() {
        // 反引号也应该被处理
        let input = "code\\`block";
        let result = common::unescape_string(input);
        assert_eq!(result, "code`block");
    }

    #[test]
    fn test_actual_newline_with_backslash() {
        // 实际换行符后的反斜杠
        let input = "test\\\nmore";
        let result = common::unescape_string(input);
        assert_eq!(result, "test\nmore");
    }

    #[test]
    fn test_empty_old_string_different_from_new() {
        // 测试验证：old 和 new 不能相同
        let args = EditArgs {
            file_path: "test.txt".into(),
            old_string: "same".into(),
            new_string: "same".into(),
            expected_replacements: 1,
        };

        assert!(
            validate_args(&args).is_err(),
            "Should reject when old equals new"
        );
    }

    #[test]
    fn test_expected_replacements_minimum() {
        // 测试验证：expected_replacements 必须 >= 1
        let args = EditArgs {
            file_path: "test.txt".into(),
            old_string: "old".into(),
            new_string: "new".into(),
            expected_replacements: 0,
        };

        assert!(
            validate_args(&args).is_err(),
            "Should reject when expected_replacements < 1"
        );
    }

    #[test]
    fn test_replacement_result_basic() {
        // 测试基本替换逻辑
        let result = try_exact_replacement("foo", "bar", "foo foo foo");
        assert_eq!(result.occurrences, 3);
        assert_eq!(result.new_content, "bar bar bar");
    }

    // ========== expected_replacements 边界测试 ==========

    #[test]
    fn test_expected_replacements_exact_match() {
        // 恰好匹配预期数量
        let result = try_exact_replacement("x", "y", "x x x");
        assert!(is_success(&result, 3));
        assert!(!is_success(&result, 2));
        assert!(!is_success(&result, 4));
    }

    #[test]
    fn test_expected_replacements_zero_occurrences() {
        // 文件中没有匹配
        let result = try_exact_replacement("notfound", "replacement", "hello world");
        assert_eq!(result.occurrences, 0);
        assert!(!is_success(&result, 1));
    }

    #[test]
    fn test_expected_replacements_more_than_expected() {
        // 实际出现次数多于预期
        let result = try_exact_replacement("a", "b", "a a a a a");
        assert_eq!(result.occurrences, 5);
        assert!(!is_success(&result, 2), "Should fail when more occurrences than expected");
    }

    // ========== 文件创建逻辑测试 ==========

    #[test]
    fn test_validate_args_creation_mode() {
        // old_string为空表示创建文件模式，这应该是有效的
        let create_args = EditArgs {
            file_path: "new_file.txt".into(),
            old_string: "".into(),
            new_string: "content".into(),
            expected_replacements: 1,
        };
        // 这应该通过验证（expected_replacements会被忽略在创建模式）
        // 但我们的验证要求expected_replacements >= 1
        assert!(validate_args(&create_args).is_ok());
    }

    // ========== 字符串相等性检查 ==========

    #[test]
    fn test_validate_rejects_identical_strings() {
        let args = EditArgs {
            file_path: "test.txt".into(),
            old_string: "identical".into(),
            new_string: "identical".into(),
            expected_replacements: 1,
        };

        let result = validate_args(&args);
        assert!(result.is_err());

        if let Err(e) = result {
            let error_msg = format!("{:?}", e);
            assert!(error_msg.contains("No changes") || error_msg.contains("equals"));
        }
    }

    // ========== 特殊字符处理测试 ==========

    #[test]
    fn test_replacement_with_special_characters() {
        // $ 字符（Rust的replace是字面量，所以应该正常工作）
        let result = try_exact_replacement("$50", "$100", "Price: $50");
        assert_eq!(result.occurrences, 1);
        assert_eq!(result.new_content, "Price: $100");

        // 正则表达式特殊字符
        let result2 = try_exact_replacement("a.b", "x.y", "test a.b end");
        assert_eq!(result2.occurrences, 1);
        assert_eq!(result2.new_content, "test x.y end");
    }

    #[test]
    fn test_replacement_multiline() {
        // 多行文本替换
        let old = "line1\nline2";
        let new = "new1\nnew2";
        let content = "start\nline1\nline2\nend";

        let result = try_exact_replacement(old, new, content);
        assert_eq!(result.occurrences, 1);
        assert_eq!(result.new_content, "start\nnew1\nnew2\nend");
    }

    #[test]
    fn test_replacement_empty_strings() {
        // 空字符串替换（删除）
        let result = try_exact_replacement("delete", "", "keep delete keep");
        assert_eq!(result.occurrences, 1);
        assert_eq!(result.new_content, "keep  keep");

        // 替换为空字符串
        let result2 = try_exact_replacement("x", "", "axbxc");
        assert_eq!(result2.occurrences, 2);
        assert_eq!(result2.new_content, "abc");
    }

    #[test]
    fn test_detection_logic_comprehensive() {
        // 测试检测逻辑的各种情况

        // 不应该被检测为转义的情况
        let correct1 = "hello\nworld";  // 真实换行
        assert!(!string_appears_escaped(correct1));

        let correct2 = "path/to/file";  // 普通路径
        assert!(!string_appears_escaped(correct2));

        // 应该被检测为转义的情况
        let escaped1 = "hello\\nworld";  // 转义的换行
        assert!(string_appears_escaped(escaped1));

        let escaped2 = "tab\\there";  // 转义的tab
        assert!(string_appears_escaped(escaped2));
    }

    #[test]
    fn test_crlf_preservation() {
        // 测试 CRLF 行尾风格的保留
        // 模拟 write_and_respond() 中的转换逻辑

        // 场景1: CRLF 文件，处理后应该转回 CRLF
        let original_crlf = "line1\r\nline2\r\nline3\r\n";
        let detected = common::detect_line_ending(original_crlf);
        assert_eq!(detected, "\r\n", "应该检测到 CRLF");

        // 规范化为 LF 进行处理
        let normalized = original_crlf.replace("\r\n", "\n");
        assert_eq!(normalized, "line1\nline2\nline3\n");

        // 处理后的内容（假设替换了某些内容）
        let processed = normalized.replace("line2", "modified");
        assert_eq!(processed, "line1\nmodified\nline3\n");

        // 转回原始行尾风格（CRLF）
        let final_content = if detected == "\r\n" {
            processed.replace('\n', "\r\n")
        } else {
            processed
        };
        assert_eq!(final_content, "line1\r\nmodified\r\nline3\r\n");
        assert!(final_content.contains("\r\n"), "最终内容应该包含 CRLF");
        assert!(!final_content.contains("\n\n"), "不应该有双换行");

        // 场景2: LF 文件，应该保持 LF
        let original_lf = "line1\nline2\nline3\n";
        let detected_lf = common::detect_line_ending(original_lf);
        assert_eq!(detected_lf, "\n", "应该检测到 LF");

        let processed_lf = original_lf.replace("line2", "modified");
        let final_lf = if detected_lf == "\r\n" {
            processed_lf.replace('\n', "\r\n")
        } else {
            processed_lf
        };
        assert_eq!(final_lf, "line1\nmodified\nline3\n");
        assert!(!final_lf.contains("\r\n"), "LF 文件不应该有 CRLF");
    }

    // 辅助函数：检测字符串是否看起来被转义
    fn string_appears_escaped(s: &str) -> bool {
        common::unescape_string(s) != s
    }
}
