//! Runtime environment snapshot.
//!
//! Captures platform, working directory, git state, and model information.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Runtime environment information for the agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    /// Operating system platform (e.g., "darwin", "linux", "windows").
    pub platform: String,
    /// OS version string.
    pub os_version: String,
    /// Current working directory.
    pub cwd: PathBuf,
    /// Whether the cwd is inside a git repository.
    pub is_git_repo: bool,
    /// Current git branch, if in a git repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    /// Current date (e.g., "2025-01-29").
    pub date: String,
    /// Current model name.
    pub model: String,
    /// Maximum context window tokens for this model.
    pub context_window: i32,
    /// Maximum output tokens for this model.
    pub max_output_tokens: i32,
    /// Preferred response language (e.g., "en", "zh", "ja").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_preference: Option<String>,
}

impl EnvironmentInfo {
    /// Create a builder for constructing environment info.
    pub fn builder() -> EnvironmentInfoBuilder {
        EnvironmentInfoBuilder::default()
    }
}

/// Builder for [`EnvironmentInfo`].
#[derive(Debug, Default)]
pub struct EnvironmentInfoBuilder {
    platform: Option<String>,
    os_version: Option<String>,
    cwd: Option<PathBuf>,
    is_git_repo: bool,
    git_branch: Option<String>,
    date: Option<String>,
    model: Option<String>,
    context_window: Option<i32>,
    max_output_tokens: Option<i32>,
    language_preference: Option<String>,
}

impl EnvironmentInfoBuilder {
    pub fn platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = Some(platform.into());
        self
    }

    pub fn os_version(mut self, os_version: impl Into<String>) -> Self {
        self.os_version = Some(os_version.into());
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn is_git_repo(mut self, is_git_repo: bool) -> Self {
        self.is_git_repo = is_git_repo;
        self
    }

    pub fn git_branch(mut self, branch: impl Into<String>) -> Self {
        self.git_branch = Some(branch.into());
        self
    }

    pub fn date(mut self, date: impl Into<String>) -> Self {
        self.date = Some(date.into());
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn context_window(mut self, tokens: i32) -> Self {
        self.context_window = Some(tokens);
        self
    }

    pub fn max_output_tokens(mut self, tokens: i32) -> Self {
        self.max_output_tokens = Some(tokens);
        self
    }

    pub fn language_preference(mut self, lang: impl Into<String>) -> Self {
        self.language_preference = Some(lang.into());
        self
    }

    /// Build the [`EnvironmentInfo`].
    ///
    /// Returns `Err` if required fields are missing.
    pub fn build(self) -> crate::error::Result<EnvironmentInfo> {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        Ok(EnvironmentInfo {
            platform: self
                .platform
                .unwrap_or_else(|| std::env::consts::OS.to_string()),
            os_version: self.os_version.unwrap_or_default(),
            cwd: self.cwd.ok_or_else(|| {
                crate::error::context_error::BuildSnafu {
                    message: "cwd is required",
                }
                .build()
            })?,
            is_git_repo: self.is_git_repo,
            git_branch: self.git_branch,
            date: self.date.unwrap_or(today),
            model: self.model.ok_or_else(|| {
                crate::error::context_error::BuildSnafu {
                    message: "model is required",
                }
                .build()
            })?,
            context_window: self.context_window.unwrap_or(200000),
            max_output_tokens: self.max_output_tokens.unwrap_or(16384),
            language_preference: self.language_preference,
        })
    }
}

#[cfg(test)]
#[path = "environment.test.rs"]
mod tests;
