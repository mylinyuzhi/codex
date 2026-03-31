use super::*;
use std::path::PathBuf;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[tokio::test]
async fn test_bash_echo() {
    let tool = BashTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "command": "echo hello"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(content.contains("hello"));
    assert!(!result.is_error);
}

#[tokio::test]
async fn test_bash_failure() {
    let tool = BashTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "command": "exit 1"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(result.is_error);
}

#[test]
fn test_is_read_only() {
    assert!(is_read_only_command("ls -la"));
    assert!(is_read_only_command("cat file.txt"));
    assert!(is_read_only_command("git status"));
    assert!(is_read_only_command("git log --oneline"));
    assert!(is_read_only_command("git diff HEAD~1"));
    assert!(is_read_only_command("git show HEAD"));
    assert!(is_read_only_command("git branch -a"));
    assert!(is_read_only_command("git rev-parse HEAD"));
    assert!(is_read_only_command("git blame src/main.rs"));
    assert!(is_read_only_command("git ls-files"));
    assert!(!is_read_only_command("rm -rf /"));
    assert!(!is_read_only_command("ls && rm foo"));
    assert!(!is_read_only_command("echo foo > bar"));
}

#[test]
fn test_git_write_commands_not_read_only() {
    // Destructive git commands must NOT be classified as read-only
    assert!(!is_read_only_command("git push"));
    assert!(!is_read_only_command("git push origin main"));
    assert!(!is_read_only_command("git reset --hard HEAD~1"));
    assert!(!is_read_only_command("git clean -f"));
    assert!(!is_read_only_command("git checkout ."));
    assert!(!is_read_only_command("git commit -m 'test'"));
    assert!(!is_read_only_command("git merge feature"));
    assert!(!is_read_only_command("git rebase main"));
    assert!(!is_read_only_command("git stash"));
    assert!(!is_read_only_command("git add ."));
    assert!(!is_read_only_command("git pull"));
    assert!(!is_read_only_command("git fetch"));
    // git with no subcommand
    assert!(!is_read_only_command("git"));
}

#[test]
fn test_tool_properties() {
    let tool = BashTool::new();
    assert_eq!(tool.name(), cocode_protocol::ToolName::Bash.as_str());
    assert!(!tool.is_concurrent_safe());
}

// -- Plan mode safe binary checks --

#[test]
fn test_plan_mode_safe_binary_basic() {
    // Basic read-only commands
    assert!(is_plan_mode_safe_binary("ls", &[]));
    assert!(is_plan_mode_safe_binary("cat", &[]));
    assert!(is_plan_mode_safe_binary(
        "grep",
        &["-r".into(), "pattern".into()]
    ));
    assert!(is_plan_mode_safe_binary("wc", &["-l".into()]));
    assert!(is_plan_mode_safe_binary(
        "find",
        &[".".into(), "-name".into(), "*.rs".into()]
    ));

    // Text processing commands
    assert!(is_plan_mode_safe_binary("awk", &["{print $1}".into()]));
    assert!(is_plan_mode_safe_binary("sort", &[]));
    assert!(is_plan_mode_safe_binary("uniq", &["-c".into()]));
    assert!(is_plan_mode_safe_binary(
        "cut",
        &["-d,".into(), "-f1".into()]
    ));
    assert!(is_plan_mode_safe_binary(
        "tr",
        &["a-z".into(), "A-Z".into()]
    ));
    assert!(is_plan_mode_safe_binary("jq", &[".data".into()]));

    // Path utilities
    assert!(is_plan_mode_safe_binary("dirname", &["/foo/bar".into()]));
    assert!(is_plan_mode_safe_binary("basename", &["/foo/bar".into()]));
    assert!(is_plan_mode_safe_binary("realpath", &[".".into()]));

    // Unsafe commands
    assert!(!is_plan_mode_safe_binary("rm", &["-rf".into(), "/".into()]));
    assert!(!is_plan_mode_safe_binary("mkdir", &["foo".into()]));
    assert!(!is_plan_mode_safe_binary("cp", &["a".into(), "b".into()]));
    assert!(!is_plan_mode_safe_binary("mv", &["a".into(), "b".into()]));
    assert!(!is_plan_mode_safe_binary(
        "chmod",
        &["755".into(), "file".into()]
    ));
    assert!(!is_plan_mode_safe_binary(
        "curl",
        &["https://example.com".into()]
    ));
    assert!(!is_plan_mode_safe_binary("npm", &["install".into()]));
}

#[test]
fn test_plan_mode_git_subcommands() {
    // Read-only git subcommands
    assert!(is_plan_mode_safe_binary("git", &["status".into()]));
    assert!(is_plan_mode_safe_binary(
        "git",
        &["log".into(), "--oneline".into()]
    ));
    assert!(is_plan_mode_safe_binary(
        "git",
        &["diff".into(), "HEAD~1".into()]
    ));
    assert!(is_plan_mode_safe_binary(
        "git",
        &["show".into(), "HEAD".into()]
    ));
    assert!(is_plan_mode_safe_binary(
        "git",
        &["branch".into(), "-a".into()]
    ));
    assert!(is_plan_mode_safe_binary(
        "git",
        &["rev-parse".into(), "HEAD".into()]
    ));
    assert!(is_plan_mode_safe_binary(
        "git",
        &["blame".into(), "src/main.rs".into()]
    ));
    assert!(is_plan_mode_safe_binary("git", &["ls-files".into()]));
    assert!(is_plan_mode_safe_binary(
        "git",
        &["ls-tree".into(), "HEAD".into()]
    ));
    assert!(is_plan_mode_safe_binary(
        "git",
        &["cat-file".into(), "-p".into(), "HEAD".into()]
    ));
    assert!(is_plan_mode_safe_binary(
        "git",
        &["config".into(), "--get".into(), "user.name".into()]
    ));
    assert!(is_plan_mode_safe_binary("git", &["shortlog".into()]));
    assert!(is_plan_mode_safe_binary("git", &["describe".into()]));

    // Write git subcommands must be denied
    assert!(!is_plan_mode_safe_binary("git", &["push".into()]));
    assert!(!is_plan_mode_safe_binary(
        "git",
        &["commit".into(), "-m".into(), "msg".into()]
    ));
    assert!(!is_plan_mode_safe_binary(
        "git",
        &["add".into(), ".".into()]
    ));
    assert!(!is_plan_mode_safe_binary(
        "git",
        &["reset".into(), "--hard".into()]
    ));
    assert!(!is_plan_mode_safe_binary(
        "git",
        &["checkout".into(), ".".into()]
    ));
    assert!(!is_plan_mode_safe_binary(
        "git",
        &["merge".into(), "feature".into()]
    ));
    assert!(!is_plan_mode_safe_binary(
        "git",
        &["rebase".into(), "main".into()]
    ));
    assert!(!is_plan_mode_safe_binary("git", &["stash".into()]));
    assert!(!is_plan_mode_safe_binary("git", &["pull".into()]));
    assert!(!is_plan_mode_safe_binary("git", &["fetch".into()]));
    assert!(!is_plan_mode_safe_binary(
        "git",
        &["clean".into(), "-f".into()]
    ));
    // git with no subcommand
    assert!(!is_plan_mode_safe_binary("git", &[]));
}

#[test]
fn test_plan_mode_sed_inplace_blocked() {
    // sed without -i is fine
    assert!(is_plan_mode_safe_binary("sed", &["s/foo/bar/".into()]));
    assert!(is_plan_mode_safe_binary(
        "sed",
        &["-n".into(), "s/foo/bar/p".into()]
    ));
    assert!(is_plan_mode_safe_binary(
        "sed",
        &["-e".into(), "s/foo/bar/".into()]
    ));

    // sed -i in all forms must be blocked
    assert!(!is_plan_mode_safe_binary(
        "sed",
        &["-i".into(), "s/foo/bar/".into(), "file".into()]
    ));
    assert!(!is_plan_mode_safe_binary(
        "sed",
        &["-i.bak".into(), "s/foo/bar/".into(), "file".into()]
    ));
    // Combined flags with i
    assert!(!is_plan_mode_safe_binary(
        "sed",
        &["-ni".into(), "s/foo/bar/p".into()]
    ));
    assert!(!is_plan_mode_safe_binary(
        "sed",
        &["-in".into(), "s/foo/bar/p".into()]
    ));
}

// -- Plan mode allowed command checks --

#[test]
fn test_plan_mode_allowed_simple_commands() {
    // Simple read-only commands (fast path)
    assert!(is_plan_mode_allowed("ls -la"));
    assert!(is_plan_mode_allowed("cat file.txt"));
    assert!(is_plan_mode_allowed("grep -r pattern ."));
    assert!(is_plan_mode_allowed("git status"));
    assert!(is_plan_mode_allowed("git log --oneline"));
    assert!(is_plan_mode_allowed("echo hello"));
    assert!(is_plan_mode_allowed("pwd"));
    assert!(is_plan_mode_allowed("wc -l file.txt"));
}

#[test]
fn test_plan_mode_allowed_pipelines() {
    // Pipelines of safe commands
    assert!(is_plan_mode_allowed("cat file.txt | grep pattern"));
    assert!(is_plan_mode_allowed("cat file.txt | grep pattern | wc -l"));
    assert!(is_plan_mode_allowed("ls -la | sort | head -20"));
    assert!(is_plan_mode_allowed("find . -name '*.rs' | wc -l"));
    assert!(is_plan_mode_allowed("git log --oneline | head -10"));
    assert!(is_plan_mode_allowed(
        "cat file | awk '{print $1}' | sort | uniq -c"
    ));
    assert!(is_plan_mode_allowed("git diff HEAD | grep '+' | wc -l"));
    assert!(is_plan_mode_allowed("cat data.json | jq '.items'"));
}

#[test]
fn test_plan_mode_allowed_chained_commands() {
    // Chained safe commands
    assert!(is_plan_mode_allowed("ls -la && pwd"));
    assert!(is_plan_mode_allowed("git status && git log --oneline"));
    assert!(is_plan_mode_allowed("echo foo || echo bar"));
}

#[test]
fn test_plan_mode_denied_write_commands() {
    // Write commands must be denied
    assert!(!is_plan_mode_allowed("rm -rf /"));
    assert!(!is_plan_mode_allowed("mkdir new_dir"));
    assert!(!is_plan_mode_allowed("cp a b"));
    assert!(!is_plan_mode_allowed("mv a b"));
    assert!(!is_plan_mode_allowed("chmod 755 file"));
    assert!(!is_plan_mode_allowed("npm install"));
    assert!(!is_plan_mode_allowed("cargo build"));
    assert!(!is_plan_mode_allowed("make"));
}

#[test]
fn test_plan_mode_denied_redirections() {
    // Redirections are blocked by try_extract_safe_commands
    assert!(!is_plan_mode_allowed("echo foo > bar"));
    assert!(!is_plan_mode_allowed("cat file >> output"));
    assert!(!is_plan_mode_allowed("ls > filelist.txt"));
}

#[test]
fn test_plan_mode_denied_command_substitution() {
    // Command substitutions are blocked by try_extract_safe_commands
    assert!(!is_plan_mode_allowed("echo $(pwd)"));
    assert!(!is_plan_mode_allowed("ls $(cat dirs.txt)"));
}

#[test]
fn test_plan_mode_denied_variable_expansion() {
    // Variable expansions are blocked by try_extract_safe_commands
    assert!(!is_plan_mode_allowed("echo $HOME"));
    assert!(!is_plan_mode_allowed("ls ${SOME_DIR}"));
}

#[test]
fn test_plan_mode_denied_unsafe_in_pipeline() {
    // Pipeline with any unsafe command is denied
    assert!(!is_plan_mode_allowed("cat file | rm -rf /"));
    assert!(!is_plan_mode_allowed("ls | xargs rm"));
    assert!(!is_plan_mode_allowed("grep pattern | curl -X POST"));
    assert!(!is_plan_mode_allowed("cat file | python -c 'import os'"));
}

#[test]
fn test_plan_mode_denied_git_write_commands() {
    // Git write commands
    assert!(!is_plan_mode_allowed("git push"));
    assert!(!is_plan_mode_allowed("git commit -m 'msg'"));
    assert!(!is_plan_mode_allowed("git add ."));
    assert!(!is_plan_mode_allowed("git reset --hard HEAD"));
}

#[test]
fn test_plan_mode_denied_sed_inplace() {
    // sed -i in pipelines
    assert!(!is_plan_mode_allowed("cat file | sed -i 's/foo/bar/' file"));
}

#[test]
fn test_plan_mode_allowed_sed_without_inplace() {
    // sed without -i in pipeline is fine
    assert!(is_plan_mode_allowed("cat file | sed 's/foo/bar/'"));
    assert!(is_plan_mode_allowed("cat file | sed -n 's/foo/bar/p'"));
}

#[tokio::test]
async fn test_plan_mode_check_permission_allows_readonly() {
    let tool = BashTool::new();
    let mut ctx = make_context();
    ctx.env.is_plan_mode = true;

    // Read-only command should be allowed
    let input = serde_json::json!({ "command": "ls -la" });
    let result = tool.check_permission(&input, &ctx).await;
    assert!(matches!(result, PermissionResult::Allowed));

    // Pipeline of safe commands should be allowed
    let input = serde_json::json!({ "command": "cat file | grep pattern | wc -l" });
    let result = tool.check_permission(&input, &ctx).await;
    assert!(matches!(result, PermissionResult::Allowed));
}

#[tokio::test]
async fn test_plan_mode_check_permission_denies_write() {
    let tool = BashTool::new();
    let mut ctx = make_context();
    ctx.env.is_plan_mode = true;

    // Write command should be denied
    let input = serde_json::json!({ "command": "rm -rf /" });
    let result = tool.check_permission(&input, &ctx).await;
    assert!(matches!(result, PermissionResult::Denied { .. }));

    // npm install should be denied
    let input = serde_json::json!({ "command": "npm install" });
    let result = tool.check_permission(&input, &ctx).await;
    assert!(matches!(result, PermissionResult::Denied { .. }));
}

// -- Compound command risk checks --

#[test]
fn test_compound_multiple_cd_flagged() {
    let commands = vec![
        vec!["cd".into(), "/tmp".into()],
        vec!["cd".into(), "/var".into()],
        vec!["ls".into()],
    ];
    assert!(
        check_compound_risks(&commands).is_some(),
        "multiple cd should be flagged"
    );
}

#[test]
fn test_compound_cd_plus_git_write_flagged() {
    let commands = vec![
        vec!["cd".into(), "/tmp".into()],
        vec!["git".into(), "push".into()],
    ];
    assert!(
        check_compound_risks(&commands).is_some(),
        "cd + git push should be flagged"
    );
}

#[test]
fn test_compound_too_many_subcommands() {
    let commands: Vec<Vec<String>> = (0..25)
        .map(|_| vec!["echo".into(), "hello".into()])
        .collect();
    assert!(
        check_compound_risks(&commands).is_some(),
        "25 subcommands should exceed limit"
    );
}

#[test]
fn test_compound_cd_plus_safe_command_ok() {
    let commands = vec![
        vec!["cd".into(), "/tmp".into()],
        vec!["ls".into(), "-la".into()],
    ];
    assert!(
        check_compound_risks(&commands).is_none(),
        "cd + ls should be fine"
    );
}

#[test]
fn test_compound_single_cd_ok() {
    let commands = vec![
        vec!["cd".into(), "/tmp".into()],
        vec!["echo".into(), "hello".into()],
    ];
    assert!(
        check_compound_risks(&commands).is_none(),
        "single cd should be fine"
    );
}

#[tokio::test]
async fn test_non_plan_mode_unaffected() {
    let tool = BashTool::new();
    let ctx = make_context();
    assert!(!ctx.env.is_plan_mode);

    // In normal mode, read-only commands are still Allowed
    let input = serde_json::json!({ "command": "ls -la" });
    let result = tool.check_permission(&input, &ctx).await;
    assert!(matches!(result, PermissionResult::Allowed));

    // In normal mode, non-read-only commands go through normal flow (NeedsApproval)
    let input = serde_json::json!({ "command": "npm install" });
    let result = tool.check_permission(&input, &ctx).await;
    assert!(matches!(result, PermissionResult::NeedsApproval { .. }));
}
