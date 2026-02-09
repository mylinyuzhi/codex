//! Centralized feature flags and metadata.
//!
//! This module defines a small set of toggles that gate experimental and
//! optional behavior across the codebase. Instead of wiring individual
//! booleans through multiple types, call sites consult a single `Features`
//! container attached to `Config`.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// High-level lifecycle stage for a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Experimental,
    Beta {
        name: &'static str,
        menu_description: &'static str,
        announcement: &'static str,
    },
    Stable,
    Deprecated,
    Removed,
}

impl Stage {
    pub fn beta_menu_name(self) -> Option<&'static str> {
        match self {
            Stage::Beta { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn beta_menu_description(self) -> Option<&'static str> {
        match self {
            Stage::Beta {
                menu_description, ..
            } => Some(menu_description),
            _ => None,
        }
    }

    pub fn beta_announcement(self) -> Option<&'static str> {
        match self {
            Stage::Beta { announcement, .. } => Some(announcement),
            _ => None,
        }
    }
}

/// Unique features toggled via configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    // Stable.
    /// Create a ghost commit at each turn.
    GhostCommit,

    // Experimental
    /// Enable Windows sandbox (restricted token) on Windows.
    WindowsSandbox,
    /// Use the elevated Windows sandbox pipeline (setup + runner).
    WindowsSandboxElevated,
    /// Append additional AGENTS.md guidance to user instructions.
    HierarchicalAgents,
    /// Enforce UTF8 output in Powershell.
    PowershellUtf8,
    /// Enable collab tools.
    Collab,
    WebFetch,
    /// Enable custom web_search tool (DuckDuckGo/Tavily providers).
    WebSearch,
    /// Enable retrieval tool (experimental, requires retrieval.toml configuration).
    Retrieval,
    /// Enable LSP tool for code intelligence (requires pre-installed LSP servers).
    Lsp,
    /// Enable the LS directory listing tool.
    Ls,
    /// Enable MCP resource tools (list_mcp_resources, list_mcp_resource_templates, read_mcp_resource).
    McpResourceTools,
    /// LLM-assisted edit correction when string matching fails.
    SmartEdit,
}

impl Feature {
    pub fn key(self) -> &'static str {
        self.info().key
    }

    pub fn stage(self) -> Stage {
        self.info().stage
    }

    pub fn default_enabled(self) -> bool {
        self.info().default_enabled
    }

    fn info(self) -> &'static FeatureSpec {
        all_features()
            .find(|spec| spec.id == self)
            .unwrap_or_else(|| unreachable!("missing FeatureSpec for {:?}", self))
    }
}

/// Holds the effective set of enabled features.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Features {
    enabled: BTreeSet<Feature>,
}

impl Features {
    /// Starts with built-in defaults.
    pub fn with_defaults() -> Self {
        let mut set = BTreeSet::new();
        for spec in all_features() {
            if spec.default_enabled {
                set.insert(spec.id);
            }
        }
        Self { enabled: set }
    }

    pub fn enabled(&self, f: Feature) -> bool {
        self.enabled.contains(&f)
    }

    pub fn enable(&mut self, f: Feature) -> &mut Self {
        self.enabled.insert(f);
        self
    }

    pub fn disable(&mut self, f: Feature) -> &mut Self {
        self.enabled.remove(&f);
        self
    }

    /// Apply a table of key -> bool toggles (e.g. from TOML).
    pub fn apply_map(&mut self, m: &BTreeMap<String, bool>) {
        for (k, v) in m {
            if let Some(feat) = feature_for_key(k) {
                if *v {
                    self.enable(feat);
                } else {
                    self.disable(feat);
                }
            }
            // Unknown keys are silently ignored - callers can use is_known_feature_key() to validate
        }
    }

    pub fn enabled_features(&self) -> Vec<Feature> {
        self.enabled.iter().copied().collect()
    }
}

/// Returns all feature specifications.
pub fn all_features() -> impl Iterator<Item = &'static FeatureSpec> {
    FEATURES.iter()
}

/// Keys accepted in `[features]` tables.
pub fn feature_for_key(key: &str) -> Option<Feature> {
    for spec in all_features() {
        if spec.key == key {
            return Some(spec.id);
        }
    }
    None
}

/// Returns `true` if the provided string matches a known feature toggle key.
pub fn is_known_feature_key(key: &str) -> bool {
    feature_for_key(key).is_some()
}

/// Single, easy-to-read registry of all feature definitions.
#[derive(Debug, Clone, Copy)]
pub struct FeatureSpec {
    pub id: Feature,
    pub key: &'static str,
    pub stage: Stage,
    pub default_enabled: bool,
}

/// Core feature specifications. Use `all_features()` to include ext features.
const FEATURES: &[FeatureSpec] = &[
    // Stable features.
    FeatureSpec {
        id: Feature::GhostCommit,
        key: "undo",
        stage: Stage::Stable,
        default_enabled: false,
    },
    // Beta program. Rendered in the `/experimental` menu for users.
    FeatureSpec {
        id: Feature::HierarchicalAgents,
        key: "hierarchical_agents",
        stage: Stage::Experimental,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::WindowsSandbox,
        key: "experimental_windows_sandbox",
        stage: Stage::Experimental,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::WindowsSandboxElevated,
        key: "elevated_windows_sandbox",
        stage: Stage::Experimental,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::PowershellUtf8,
        key: "powershell_utf8",
        stage: Stage::Experimental,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Collab,
        key: "collab",
        stage: Stage::Experimental,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::WebFetch,
        key: "web_fetch",
        stage: Stage::Experimental,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::WebSearch,
        key: "web_search",
        stage: Stage::Experimental,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Retrieval,
        key: "code_search",
        stage: Stage::Experimental,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Lsp,
        key: "lsp",
        stage: Stage::Experimental,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Ls,
        key: "ls",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::McpResourceTools,
        key: "mcp_resource_tools",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::SmartEdit,
        key: "smart_edit",
        stage: Stage::Experimental,
        default_enabled: false,
    },
];

#[cfg(test)]
#[path = "features.test.rs"]
mod tests;
