use serde::Deserialize;
use serde::Serialize;
use std::path::Path;

/// Platform information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Darwin,
    Linux,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Darwin
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            Self::Darwin => "macOS",
            Self::Linux => "Linux",
            Self::Windows => "Windows",
        }
    }
}

/// Shell type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellKind {
    Bash,
    Zsh,
    Sh,
    PowerShell,
}

impl ShellKind {
    /// Detect the current shell from $SHELL env var.
    pub fn detect() -> Self {
        let shell = std::env::var("SHELL").unwrap_or_default();
        if shell.contains("zsh") {
            Self::Zsh
        } else if shell.contains("bash") {
            Self::Bash
        } else if shell.contains("pwsh") || shell.contains("powershell") {
            Self::PowerShell
        } else {
            Self::Bash // default
        }
    }
}

/// Collected environment information for system prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    pub cwd: String,
    pub platform: Platform,
    pub shell: ShellKind,
    pub os_version: String,
    pub model: String,
    pub knowledge_cutoff: String,
    pub is_git_repo: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_status: Option<GitStatus>,
}

/// Git repository status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatus {
    pub branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    pub status: String,
    pub recent_commits: String,
}

/// Build environment info for the given working directory.
pub fn get_environment_info(cwd: &Path, model: &str) -> EnvironmentInfo {
    let is_git_repo = cwd.join(".git").exists();
    let git_status = if is_git_repo {
        get_git_status(cwd).ok()
    } else {
        None
    };

    let os_version = get_os_version();

    EnvironmentInfo {
        cwd: cwd.to_string_lossy().to_string(),
        platform: Platform::current(),
        shell: ShellKind::detect(),
        os_version,
        model: model.to_string(),
        knowledge_cutoff: "May 2025".to_string(),
        is_git_repo,
        git_status,
    }
}

/// Get git status for a repository.
fn get_git_status(cwd: &Path) -> anyhow::Result<GitStatus> {
    let branch = run_git(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let status = run_git(cwd, &["status", "--short"])?;
    let user = run_git(cwd, &["config", "user.name"]).ok();
    let recent_commits = run_git(cwd, &["log", "--oneline", "-5"]).unwrap_or_default();

    // Detect main branch
    let main_branch = detect_main_branch(cwd);

    Ok(GitStatus {
        branch,
        main_branch,
        user,
        status,
        recent_commits,
    })
}

fn detect_main_branch(cwd: &Path) -> Option<String> {
    for candidate in &["main", "master"] {
        if run_git(cwd, &["rev-parse", "--verify", candidate]).is_ok() {
            return Some(candidate.to_string());
        }
    }
    None
}

fn run_git(cwd: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        anyhow::bail!("git command failed")
    }
}

fn get_os_version() -> String {
    let output = std::process::Command::new("uname").arg("-sr").output().ok();
    output
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| format!("{}", Platform::current().display_name()))
}

#[cfg(test)]
#[path = "environment.test.rs"]
mod tests;
