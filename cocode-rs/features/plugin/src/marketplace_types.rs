//! Pure data types for marketplace operations.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// How to fetch a marketplace.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum MarketplaceSource {
    /// A GitHub repository (owner/repo).
    Github {
        repo: String,
        #[serde(rename = "ref")]
        git_ref: Option<String>,
    },
    /// A generic git URL.
    Git {
        url: String,
        #[serde(rename = "ref")]
        git_ref: Option<String>,
    },
    /// A single file path to a marketplace.json.
    File { path: PathBuf },
    /// A local directory containing plugins.
    Directory { path: PathBuf },
    /// A URL to a marketplace.json.
    Url { url: String },
}

impl MarketplaceSource {
    /// Derive a marketplace name from the source.
    pub fn derive_name(&self) -> String {
        match self {
            Self::Github { repo, .. } => repo.replace('/', "-"),
            Self::Git { url, .. } => url
                .rsplit('/')
                .next()
                .unwrap_or("unknown")
                .trim_end_matches(".git")
                .to_string(),
            Self::File { path } | Self::Directory { path } => path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("local")
                .to_string(),
            Self::Url { url } => url
                .rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or("remote")
                .trim_end_matches(".json")
                .to_string(),
        }
    }
}

/// A plugin listed in a marketplace manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePluginEntry {
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub source: MarketplacePluginSource,
}

/// Where plugin code lives -- relative path or remote source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MarketplacePluginSource {
    /// A relative path within the marketplace directory.
    RelativePath(String),
    /// A remote source (GitHub, git, etc.).
    Remote(MarketplaceSource),
}

/// The marketplace.json content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceManifest {
    pub name: String,
    pub description: Option<String>,
    pub plugins: Vec<MarketplacePluginEntry>,
}

/// A registered marketplace in known_marketplaces.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownMarketplace {
    pub source: MarketplaceSource,
    pub install_location: PathBuf,
    pub last_updated: Option<String>,
    #[serde(default)]
    pub auto_update: bool,
}

#[cfg(test)]
#[path = "marketplace_types.test.rs"]
mod tests;
