//! LLM-powered error correction for failed edits.
//!
//! When search-and-replace strategies fail, this module uses an LLM to analyze
//! the failure and generate corrected search/replace strings.

use crate::client::ModelClient;
use crate::error::Result as CodexResult;
use crate::tools::llm_helper::call_llm_for_text;

/// Result of LLM correction attempt
#[derive(Debug, Clone)]
pub struct CorrectedEdit {
    pub search: String,
    pub replace: String,
    pub explanation: String,
    pub no_changes_required: bool,
}

/// System prompt for LLM correction
const CORRECTION_SYSTEM_PROMPT: &str = r#"You are an expert code-editing assistant specializing in debugging failed search-and-replace operations.

Your task: Analyze the failed edit and provide corrected `search` and `replace` strings that will match the file precisely.

**Critical Rules:**
1. Minimal Correction: Stay close to the original, only fix issues like whitespace/indentation
2. Exact Match: The new `search` must be EXACT literal text from the file
3. Preserve `replace`: Usually keep the original `replace` unchanged
4. No Changes Case: If the change already exists, set `no_changes_required` to true

**Output Format (XML):**
<correction>
  <search>corrected search string</search>
  <replace>corrected replace string</replace>
  <explanation>why it failed and how you fixed it</explanation>
  <no_changes_required>false</no_changes_required>
</correction>"#;

/// Attempt to correct a failed edit using LLM
///
/// # Parameters
/// - `client`: ModelClient to use for LLM call
/// - `instruction`: Original edit instruction (semantic description)
/// - `old_string`: Original search string that failed
/// - `new_string`: Original replace string
/// - `file_content`: Complete file content
/// - `error_msg`: Description of why the edit failed
/// - `timeout_secs`: Timeout for LLM call
///
/// # Returns
/// Corrected edit parameters or error
pub async fn attempt_llm_correction(
    client: &ModelClient,
    instruction: &str,
    old_string: &str,
    new_string: &str,
    file_content: &str,
    error_msg: &str,
    timeout_secs: u64,
) -> CodexResult<CorrectedEdit> {
    let user_prompt = format!(
        r#"# Original Edit Goal
{instruction}

# Failed Parameters
- Search string:
```
{old_string}
```

- Replace string:
```
{new_string}
```

- Error: {error_msg}

# Full File Content
```
{file_content}
```

Provide your correction in XML format."#
    );

    // Call LLM with timeout
    let response =
        call_llm_for_text(client, CORRECTION_SYSTEM_PROMPT, &user_prompt, timeout_secs).await?;

    // Parse XML response
    parse_correction_xml(&response)
}

/// Parse XML correction response from LLM
fn parse_correction_xml(xml: &str) -> CodexResult<CorrectedEdit> {
    let search = extract_xml_tag(xml, "search").ok_or_else(|| {
        crate::error::CodexErr::Fatal("LLM correction missing <search> tag".into())
    })?;

    let replace = extract_xml_tag(xml, "replace").ok_or_else(|| {
        crate::error::CodexErr::Fatal("LLM correction missing <replace> tag".into())
    })?;

    let explanation =
        extract_xml_tag(xml, "explanation").unwrap_or_else(|| "No explanation provided".into());

    let no_changes_required = extract_xml_tag(xml, "no_changes_required")
        .map(|s| s.to_lowercase() == "true")
        .unwrap_or(false);

    Ok(CorrectedEdit {
        search,
        replace,
        explanation,
        no_changes_required,
    })
}

/// Extract content between XML tags
fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{tag}>");
    let end_tag = format!("</{tag}>");

    let start_pos = xml.find(&start_tag)? + start_tag.len();
    let end_pos = xml.find(&end_tag)?;

    if start_pos < end_pos {
        Some(xml[start_pos..end_pos].trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_correction_xml() {
        let xml = r#"<correction>
  <search>corrected search</search>
  <replace>corrected replace</replace>
  <explanation>Fixed indentation</explanation>
  <no_changes_required>false</no_changes_required>
</correction>"#;

        let result = parse_correction_xml(xml);
        assert!(result.is_ok());

        let corrected = result.unwrap();
        assert_eq!(corrected.search, "corrected search");
        assert_eq!(corrected.replace, "corrected replace");
        assert_eq!(corrected.explanation, "Fixed indentation");
        assert!(!corrected.no_changes_required);
    }

    #[test]
    fn test_parse_correction_no_changes() {
        let xml = r#"<correction>
  <search>original</search>
  <replace>replacement</replace>
  <explanation>Change already exists</explanation>
  <no_changes_required>true</no_changes_required>
</correction>"#;

        let result = parse_correction_xml(xml);
        assert!(result.is_ok());

        let corrected = result.unwrap();
        assert!(corrected.no_changes_required);
    }

    #[test]
    fn test_extract_xml_tag() {
        let xml = "<root><tag>content</tag></root>";
        let content = extract_xml_tag(xml, "tag");
        assert_eq!(content, Some("content".to_string()));
    }

    #[test]
    fn test_extract_xml_tag_with_newlines() {
        let xml = "<root>\n  <tag>\n    multi\n    line\n  </tag>\n</root>";
        let content = extract_xml_tag(xml, "tag");
        assert!(content.is_some());
        // Should trim whitespace
        assert!(content.unwrap().contains("multi"));
    }

    #[test]
    fn test_extract_xml_tag_missing() {
        let xml = "<root><other>content</other></root>";
        let content = extract_xml_tag(xml, "tag");
        assert!(content.is_none());
    }
}
