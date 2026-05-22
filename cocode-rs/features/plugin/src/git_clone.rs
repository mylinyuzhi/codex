//! Git operations for plugin installation.

use std::path::Path;

use tracing::debug;
use tracing::info;
use tracing::warn;

use crate::error::Result;
use crate::error::plugin_error::GitCloneFailedSnafu;

/// Shallow clone a git repository with submodules.
pub async fn git_clone(url: &str, target: &Path, branch: Option<&str>) -> Result<()> {
    let mut args = vec![
        "-c",
        "credential.helper=",
        "-c",
        "core.sshCommand=ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new",
        "clone",
        "--depth",
        "1",
        "--recurse-submodules",
        "--shallow-submodules",
    ];

    if let Some(b) = branch {
        args.push("--branch");
        args.push(b);
    }

    args.push(url);
    let target_str = target.to_string_lossy();
    args.push(&target_str);

    debug!(url, ?branch, target = %target.display(), "Cloning repository");

    let output = tokio::process::Command::new("git")
        .args(&args)
        .output()
        .await
        .map_err(|e| {
            GitCloneFailedSnafu {
                url: url.to_string(),
                message: format!("Failed to spawn git: {e}"),
            }
            .build()
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GitCloneFailedSnafu {
            url: url.to_string(),
            message: stderr.to_string(),
        }
        .build());
    }

    info!(url, target = %target.display(), "Repository cloned");
    Ok(())
}

/// Clone with SSH -> HTTPS fallback for github.com URLs.
pub async fn git_clone_with_fallback(url: &str, target: &Path, branch: Option<&str>) -> Result<()> {
    match git_clone(url, target, branch).await {
        Ok(()) => Ok(()),
        Err(e) => {
            // If this is a github.com SSH URL, try HTTPS fallback
            if url.starts_with("git@github.com:") {
                let https_url = url
                    .replace("git@github.com:", "https://github.com/")
                    .trim_end_matches(".git")
                    .to_string()
                    + ".git";

                warn!(
                    ssh_url = url,
                    https_url = %https_url,
                    "SSH clone failed, trying HTTPS fallback"
                );

                // Clean up failed clone attempt
                if target.exists() {
                    let _ = tokio::fs::remove_dir_all(target).await;
                }

                git_clone(&https_url, target, branch).await
            } else {
                Err(e)
            }
        }
    }
}

/// Get the HEAD commit SHA from a git repository.
pub async fn get_commit_sha(repo_path: &Path) -> Result<Option<String>> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .await
        .map_err(|e| {
            GitCloneFailedSnafu {
                url: repo_path.display().to_string(),
                message: format!("Failed to get commit SHA: {e}"),
            }
            .build()
        })?;

    if output.status.success() {
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(Some(sha))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
#[path = "git_clone.test.rs"]
mod tests;
