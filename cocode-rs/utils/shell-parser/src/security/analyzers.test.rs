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
