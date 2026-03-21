use super::*;
use crate::parser::ShellParser;

fn analyze_command(source: &str) -> SecurityAnalysis {
    let mut parser = ShellParser::new();
    let cmd = parser.parse(source);
    let mut analysis = SecurityAnalysis::new();
    for analyzer in default_analyzers() {
        analyzer.analyze(&cmd, &mut analysis);
    }
    analysis
}

#[test]
fn test_jq_danger() {
    let analysis = analyze_command("jq 'system(\"id\")'");
    assert!(analysis.risks.iter().any(|r| r.kind == RiskKind::JqDanger));
}

#[test]
fn test_obfuscated_flags() {
    let analysis = analyze_command("echo $'hello\\nworld'");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::ObfuscatedFlags)
    );
}

#[test]
fn test_command_substitution() {
    let analysis = analyze_command("echo $(pwd)");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::DangerousSubstitution)
    );
}

#[test]
fn test_privilege_escalation() {
    let analysis = analyze_command("sudo rm -rf /");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::PrivilegeEscalation)
    );
}

#[test]
fn test_rm_rf() {
    let analysis = analyze_command("rm -rf /tmp/*");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::FileSystemTampering)
    );
}

#[test]
fn test_code_execution_eval() {
    let analysis = analyze_command("eval $cmd");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::CodeExecution)
    );
}

#[test]
fn test_heredoc_unsafe_in_command_substitution() {
    // Unquoted heredoc inside $() — should flag
    let analysis = analyze_command("echo $(cat <<EOF\nhello $USER\nEOF\n)");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::UnsafeHeredocSubstitution)
    );
}

#[test]
fn test_heredoc_safe_quoted_delimiter() {
    // Quoted heredoc — should NOT flag UnsafeHeredocSubstitution
    let analysis = analyze_command("cat <<'EOF'\nhello $USER\nEOF");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::UnsafeHeredocSubstitution)
    );
}

#[test]
fn test_heredoc_safe_outside_substitution() {
    // Unquoted heredoc NOT inside $() — should NOT flag
    let analysis = analyze_command("cat <<EOF\nhello $USER\nEOF");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::UnsafeHeredocSubstitution)
    );
}

#[test]
fn test_safe_command() {
    let analysis = analyze_command("ls -la");
    // Should have no high/critical risks
    assert!(analysis.risks.iter().all(|r| r.level < RiskLevel::High));
}

// =========================================================================
// Phase 2: New analyzer tests
// =========================================================================

#[test]
fn test_backslash_escaped_whitespace() {
    let analysis = analyze_command("echo hello\\ world");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::BackslashEscapedWhitespace)
    );
}

#[test]
fn test_backslash_escaped_whitespace_safe() {
    // Inside single quotes is literal, should not flag
    let analysis = analyze_command("echo 'hello\\ world'");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::BackslashEscapedWhitespace)
    );
}

#[test]
fn test_backslash_escaped_operators() {
    let analysis = analyze_command("echo test\\;id");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::BackslashEscapedOperators)
    );
}

#[test]
fn test_backslash_escaped_operators_safe() {
    // Inside quotes should not flag
    let analysis = analyze_command("echo 'test\\;id'");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::BackslashEscapedOperators)
    );
}

#[test]
fn test_unicode_whitespace() {
    // U+00A0 non-breaking space
    let analysis = analyze_command("echo\u{00A0}hello");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::UnicodeWhitespace)
    );
}

#[test]
fn test_unicode_whitespace_safe() {
    let analysis = analyze_command("echo hello world");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::UnicodeWhitespace)
    );
}

#[test]
fn test_mid_word_hash() {
    let analysis = analyze_command("echo test#comment");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::MidWordHash)
    );
}

#[test]
fn test_mid_word_hash_safe() {
    // Hash preceded by whitespace is a normal comment
    let analysis = analyze_command("echo test # comment");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::MidWordHash)
    );
}

#[test]
fn test_brace_expansion() {
    let analysis = analyze_command("echo {a,b,c}");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::BraceExpansion)
    );
}

#[test]
fn test_brace_expansion_range() {
    let analysis = analyze_command("echo {1..10}");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::BraceExpansion)
    );
}

#[test]
fn test_brace_expansion_safe() {
    // Inside quotes should not flag
    let analysis = analyze_command("echo '{a,b}'");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::BraceExpansion)
    );
}

#[test]
fn test_zsh_dangerous_commands() {
    let analysis = analyze_command("zmodload zsh/system");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::ZshDangerousCommands)
    );
}

#[test]
fn test_zsh_dangerous_commands_safe() {
    let analysis = analyze_command("echo hello");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::ZshDangerousCommands)
    );
}

#[test]
fn test_comment_quote_desync() {
    // Odd number of single quotes after #
    let analysis = analyze_command("echo test #it's broken");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::CommentQuoteDesync)
    );
}

#[test]
fn test_comment_quote_desync_safe() {
    // Even quotes after # is fine
    let analysis = analyze_command("echo test # balanced 'a' 'b'");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::CommentQuoteDesync)
    );
}

#[test]
fn test_comment_quote_desync_hash_inside_quotes() {
    // A `#` inside double quotes is a literal character, not a comment delimiter
    let analysis = analyze_command("echo \"color#red\"");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::CommentQuoteDesync),
        "hash inside double quotes should not flag CommentQuoteDesync: {analysis:?}"
    );
}

#[test]
fn test_comment_quote_desync_hash_inside_single_quotes() {
    // A `#` inside single quotes is also a literal character
    let analysis = analyze_command("echo 'color#red'");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::CommentQuoteDesync),
        "hash inside single quotes should not flag CommentQuoteDesync: {analysis:?}"
    );
}

#[test]
fn test_quoted_newline_hash() {
    let analysis = analyze_command("echo \"hello\n# injected\"");
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::QuotedNewlineHash)
    );
}

#[test]
fn test_quoted_newline_hash_safe() {
    let analysis = analyze_command("echo \"hello world\"");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::QuotedNewlineHash)
    );
}

// =========================================================================
// Phase 3: Layer 0 + safe pattern tests
// =========================================================================

#[test]
fn test_single_quote_bypass() {
    // SingleQuoteBypassAnalyzer is Layer 0 (not in default_analyzers), test directly
    let mut parser = ShellParser::new();
    let cmd = parser.parse("echo 'test\\' | evil");
    let mut analysis = SecurityAnalysis::new();
    SingleQuoteBypassAnalyzer.analyze(&cmd, &mut analysis);
    assert!(
        analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::SingleQuoteBypass),
        "should detect single quote bypass: {analysis:?}"
    );
}

#[test]
fn test_single_quote_bypass_safe() {
    let mut parser = ShellParser::new();
    let cmd = parser.parse("echo 'normal string'");
    let mut analysis = SecurityAnalysis::new();
    SingleQuoteBypassAnalyzer.analyze(&cmd, &mut analysis);
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::SingleQuoteBypass)
    );
}

#[test]
fn test_single_quote_bypass_double_backslash_safe() {
    // Even number of backslashes before closing quote is legitimate
    let mut parser = ShellParser::new();
    let cmd = parser.parse("echo 'test\\\\' more");
    let mut analysis = SecurityAnalysis::new();
    SingleQuoteBypassAnalyzer.analyze(&cmd, &mut analysis);
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::SingleQuoteBypass),
        "double backslash should NOT flag: {analysis:?}"
    );
}

// =========================================================================
// Phase 4: Existing analyzer fix tests
// =========================================================================

#[test]
fn test_jq_file_flag() {
    let analysis = analyze_command("jq -f dangerous.jq .");
    assert!(
        analysis.risks.iter().any(|r| r.kind == RiskKind::JqDanger),
        "jq -f should produce JqDanger: {analysis:?}"
    );
}

#[test]
fn test_malformed_tokens_single_quoted_brackets() {
    // Brackets inside single quotes are literal, should NOT flag
    let analysis = analyze_command("echo 'func(){}'");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::MalformedTokens && r.message.contains("unbalanced")),
        "single-quoted brackets should not flag: {analysis:?}"
    );
}

#[test]
fn test_malformed_tokens_double_quoted_brackets() {
    // Brackets inside double quotes should not cause false positive
    let analysis = analyze_command("echo \"array[0]\"");
    assert!(
        !analysis
            .risks
            .iter()
            .any(|r| r.kind == RiskKind::MalformedTokens && r.message.contains("unbalanced")),
        "double-quoted balanced brackets should not flag: {analysis:?}"
    );
}
