//! 简单 LLM 纠错（不需要 instruction）
//!
//! 当精确匹配失败时，使用 LLM 分析原因并提供纠正建议

use crate::client::ModelClient;
use crate::error::Result as CodexResult;
use crate::tools::llm_helper::call_llm_for_text;

/// 纠正结果
#[derive(Debug, Clone)]
pub struct CorrectedEdit {
    pub search: String,
    pub replace: String,
    pub explanation: String,
    pub no_changes_required: bool,
}

/// 系统提示（通用纠错）
const SYSTEM_PROMPT: &str = r#"You are a code editing assistant specializing in fixing failed search-and-replace operations.

**Your Task:**
Analyze why the search string didn't match and provide a corrected version.

**Common Issues:**
1. Over-escaped characters (\\n, \\t, \\" etc) - LLMs often do this
2. Whitespace/indentation mismatches
3. Missing context or wrong context

**Critical Rules:**
1. The corrected `search` must be EXACT literal text from the file
2. Usually keep `replace` unchanged (only fix if also has escaping issues)
3. If the desired change already exists, set `no_changes_required` to true
4. Provide brief explanation of what was wrong and how you fixed it

**Output Format (XML):**
<correction>
  <search>corrected search string</search>
  <replace>corrected replace string (usually unchanged)</replace>
  <explanation>why it failed and how fixed</explanation>
  <no_changes_required>false</no_changes_required>
</correction>"#;

/// 尝试简单 LLM 纠错
pub async fn attempt_correction(
    client: &ModelClient,
    old_string: &str,
    new_string: &str,
    file_content: &str,
    error_msg: &str,
    timeout_secs: u64,
) -> CodexResult<CorrectedEdit> {
    let user_prompt = format!(
        r#"# Failed Search-and-Replace

**Search string that failed:**
```
{old_string}
```

**Replacement string:**
```
{new_string}
```

**Error:** {error_msg}

**Full file content:**
```
{file_content}
```

Analyze the issue and provide your correction in XML format."#
    );

    let response = call_llm_for_text(client, SYSTEM_PROMPT, &user_prompt, timeout_secs).await?;

    parse_xml(&response)
}

/// 根据 old_string 的修正调整 new_string
///
/// 当 LLM 修正了 old_string 的缩进/空格时，new_string 应该做相应调整以保持语义一致。
/// 这对应 gemini-cli 的 `correctNewString()` 函数。
pub async fn adapt_new_string(
    client: &ModelClient,
    original_old: &str,
    corrected_old: &str,
    original_new: &str,
    timeout_secs: u64,
) -> CodexResult<String> {
    // 如果 old 没变，new 也不用变
    if original_old == corrected_old {
        return Ok(original_new.to_string());
    }

    let user_prompt = format!(
        r#"# Task: Adapt new_string based on old_string corrections

**Original old_string:**
```
{original_old}
```

**Corrected old_string (what actually matched the file):**
```
{corrected_old}
```

**Original new_string:**
```
{original_new}
```

**Instructions:**
The old_string was corrected (likely whitespace, indentation, or escaping changes).
Adapt the new_string to maintain the same relative changes, applying the same corrections.

For example:
- If indentation was added to old_string, add the same indentation to new_string
- If escaping was fixed in old_string, apply the same fix to new_string
- Maintain the semantic intent of the replacement

**Output Format (XML):**
<adaptation>
  <adapted_new_string>your adapted version</adapted_new_string>
  <explanation>brief explanation of changes</explanation>
</adaptation>"#
    );

    let response =
        call_llm_for_text(client, ADAPTATION_SYSTEM_PROMPT, &user_prompt, timeout_secs).await?;

    parse_adaptation_xml(&response)
}

/// 仅修正 new_string 的转义问题（不依赖 old_string 的变化）
///
/// 当 Phase 1 成功但 new_string 有转义问题时使用。
/// 对应 gemini-cli 的 `correctNewStringEscaping()` 函数。
pub async fn correct_new_string_escaping(
    client: &ModelClient,
    old_string: &str,
    new_string: &str,
    timeout_secs: u64,
) -> CodexResult<String> {
    let user_prompt = format!(
        r#"# Task: Fix escaping in new_string

**old_string (for context):**
```
{old_string}
```

**new_string (has escaping issues):**
```
{new_string}
```

**Instructions:**
The new_string appears to have over-escaping issues (e.g., \\n instead of actual newline).
Fix the escaping to produce the correct literal string.

**Common fixes:**
- \\n → actual newline character
- \\t → actual tab character
- \\\\ → single backslash

**Output Format (XML):**
<escaping_fix>
  <corrected_new_string>fixed version</corrected_new_string>
  <explanation>what was fixed</explanation>
</escaping_fix>"#
    );

    let response = call_llm_for_text(
        client,
        ESCAPING_FIX_SYSTEM_PROMPT,
        &user_prompt,
        timeout_secs,
    )
    .await?;

    parse_escaping_fix_xml(&response)
}

/// 系统提示：new_string 适配
const ADAPTATION_SYSTEM_PROMPT: &str = r#"You are a code editing assistant specializing in maintaining semantic consistency.

**Your Task:**
Adapt the new_string to match corrections made to old_string.

**Critical Rules:**
1. Apply the SAME corrections (indentation, whitespace, escaping) to new_string
2. Maintain the semantic intent of the original new_string
3. Be minimal - only change what's necessary to match the corrections

**Output Format (XML):**
<adaptation>
  <adapted_new_string>corrected version</adapted_new_string>
  <explanation>what changed and why</explanation>
</adaptation>"#;

/// 系统提示：new_string 转义修复
const ESCAPING_FIX_SYSTEM_PROMPT: &str = r#"You are a code editing assistant specializing in fixing string escaping issues.

**Common Issues:**
- \\n should be actual newline character
- \\t should be actual tab character
- \\\\ should be single backslash

**Critical Rules:**
1. Fix ONLY escaping issues, don't change semantic content
2. The output must be the EXACT string that should appear in the file

**Output Format (XML):**
<escaping_fix>
  <corrected_new_string>fixed version</corrected_new_string>
  <explanation>brief explanation</explanation>
</escaping_fix>"#;

fn parse_xml(xml: &str) -> CodexResult<CorrectedEdit> {
    let search = extract_tag(xml, "search").ok_or_else(|| {
        crate::error::CodexErr::Fatal("LLM correction missing <search> tag".into())
    })?;

    let replace = extract_tag(xml, "replace").ok_or_else(|| {
        crate::error::CodexErr::Fatal("LLM correction missing <replace> tag".into())
    })?;

    let explanation =
        extract_tag(xml, "explanation").unwrap_or_else(|| "No explanation provided".into());

    let no_changes_required = extract_tag(xml, "no_changes_required")
        .map(|s| s.to_lowercase() == "true")
        .unwrap_or(false);

    Ok(CorrectedEdit {
        search,
        replace,
        explanation,
        no_changes_required,
    })
}

/// 解析适配结果 XML
fn parse_adaptation_xml(xml: &str) -> CodexResult<String> {
    extract_tag(xml, "adapted_new_string").ok_or_else(|| {
        crate::error::CodexErr::Fatal("LLM adaptation missing <adapted_new_string> tag".into())
    })
}

/// 解析转义修复结果 XML
fn parse_escaping_fix_xml(xml: &str) -> CodexResult<String> {
    extract_tag(xml, "corrected_new_string").ok_or_else(|| {
        crate::error::CodexErr::Fatal("LLM escaping fix missing <corrected_new_string> tag".into())
    })
}

fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{tag}>");
    let end_tag = format!("</{tag}>");

    let start = xml.find(&start_tag)? + start_tag.len();
    let end = xml.find(&end_tag)?;

    if start <= end {
        Some(xml[start..end].trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_xml() {
        let xml = r#"<correction>
  <search>fixed search</search>
  <replace>fixed replace</replace>
  <explanation>Over-escaped newlines</explanation>
  <no_changes_required>false</no_changes_required>
</correction>"#;

        let result = parse_xml(xml).unwrap();
        assert_eq!(result.search, "fixed search");
        assert_eq!(result.replace, "fixed replace");
        assert_eq!(result.explanation, "Over-escaped newlines");
        assert!(!result.no_changes_required);
    }

    #[test]
    fn test_parse_xml_no_changes() {
        let xml = r#"<correction>
  <search>x</search>
  <replace>y</replace>
  <explanation>Already done</explanation>
  <no_changes_required>true</no_changes_required>
</correction>"#;

        let result = parse_xml(xml).unwrap();
        assert!(result.no_changes_required);
    }

    #[test]
    fn test_extract_tag() {
        assert_eq!(extract_tag("<a>content</a>", "a"), Some("content".into()));
        assert_eq!(extract_tag("<a>  spaced  </a>", "a"), Some("spaced".into()));
        assert_eq!(extract_tag("<a></a>", "a"), Some("".into()));
        assert_eq!(extract_tag("<b>x</b>", "a"), None);
    }

    #[test]
    fn test_parse_xml_missing_tags() {
        let xml = "<correction><search>test</search></correction>";
        assert!(parse_xml(xml).is_err());
    }
}
