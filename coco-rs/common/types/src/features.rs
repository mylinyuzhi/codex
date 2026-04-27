//! Centralized feature gates.
//!
//! See `docs/coco-rs/feature-gates-and-tool-filtering.md` for the design.
//!
//! Each `Feature` is a coarse-grained capability gate. Sub-toggles (e.g.
//! `MemoryConfig.extraction_enabled`, `RetrievalConfig.reranker.enabled`) live
//! inside their respective subsystem `*Config` structs — never expanded as
//! additional `Feature` variants.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// High-level lifecycle stage for a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Still under development; not shown in menus or announcements.
    UnderDevelopment,
    /// User-facing experimental feature available in the `/experimental` menu.
    Experimental {
        name: &'static str,
        menu_description: &'static str,
        announcement: &'static str,
    },
    /// Stable and ready for production use.
    Stable,
}

impl Stage {
    pub fn experimental_menu_name(self) -> Option<&'static str> {
        match self {
            Stage::Experimental { name, .. } => Some(name),
            Stage::UnderDevelopment | Stage::Stable => None,
        }
    }

    pub fn experimental_menu_description(self) -> Option<&'static str> {
        match self {
            Stage::Experimental {
                menu_description, ..
            } => Some(menu_description),
            Stage::UnderDevelopment | Stage::Stable => None,
        }
    }

    pub fn experimental_announcement(self) -> Option<&'static str> {
        match self {
            Stage::Experimental {
                announcement: "", ..
            } => None,
            Stage::Experimental { announcement, .. } => Some(announcement),
            Stage::UnderDevelopment | Stage::Stable => None,
        }
    }
}

/// User-facing capability gate.
///
/// Each variant represents one coarse-grained subsystem switch. Internal
/// sub-toggles stay inside the corresponding `*Config` struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    // Token-economy gate (Stable, default=true).
    /// Expose the `web_search` tool to the model.
    WebSearch,
    /// Expose the `web_fetch` tool to the model.
    WebFetch,

    // Behavior / safety gate (Stable, default=false for risk-conservative).
    /// Run shell commands inside a sandbox.
    Sandbox,

    // /experimental menu (UnderDevelopment, default=false).
    /// Auto-memory subsystem (extraction, team sync, relevant injection).
    AutoMemory,
    /// Retrieval subsystem (BM25 + vector + AST + RepoMap + reranker).
    Retrieval,
    /// Subagent / swarm spawning (`Task` tool, multi-agent orchestration).
    AgentTeams,
    /// Worktree tools (`EnterWorktree` / `ExitWorktree`).
    Worktree,
    /// LSP-backed code intelligence tool.
    Lsp,
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
        FEATURES
            .iter()
            .find(|spec| spec.id == self)
            .unwrap_or_else(|| unreachable!("missing FeatureSpec for {self:?}"))
    }
}

/// Effective set of enabled features for a session.
///
/// Intentionally **not** `Default` — callers must opt into either
/// [`Features::with_defaults`] (registry-defined defaults) or
/// [`Features::empty`] (no features enabled). The two had identical
/// constructors before, so a stray `Features::default()` silently
/// disabled every flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Features {
    enabled: BTreeSet<Feature>,
}

impl Features {
    /// Build with `default_enabled` from the registry.
    pub fn with_defaults() -> Self {
        let mut set = BTreeSet::new();
        for spec in FEATURES {
            if spec.default_enabled {
                set.insert(spec.id);
            }
        }
        Self { enabled: set }
    }

    /// Empty set — every feature off. Use only when you genuinely
    /// want no features enabled (e.g. test harnesses asserting a
    /// gate's behavior in isolation).
    pub fn empty() -> Self {
        Self {
            enabled: BTreeSet::new(),
        }
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

    pub fn set_enabled(&mut self, f: Feature, enabled: bool) -> &mut Self {
        if enabled {
            self.enable(f)
        } else {
            self.disable(f)
        }
    }

    /// Apply a key→bool table (from settings.json `features` section or env).
    /// Unknown keys are silently ignored.
    pub fn apply_map(&mut self, m: &BTreeMap<String, bool>) -> &mut Self {
        for (k, v) in m {
            if let Some(feat) = feature_for_key(k) {
                self.set_enabled(feat, *v);
            }
        }
        self
    }

    pub fn enabled_features(&self) -> Vec<Feature> {
        self.enabled.iter().copied().collect()
    }
}

/// Single-row registry entry — `(id, key, stage, default_enabled)`.
#[derive(Debug, Clone, Copy)]
pub struct FeatureSpec {
    pub id: Feature,
    pub key: &'static str,
    pub stage: Stage,
    pub default_enabled: bool,
}

/// Iterate every known feature.
pub fn all_features() -> impl Iterator<Item = &'static FeatureSpec> {
    FEATURES.iter()
}

/// Look up a feature by its config key.
pub fn feature_for_key(key: &str) -> Option<Feature> {
    FEATURES.iter().find(|spec| spec.key == key).map(|s| s.id)
}

/// Whether a string matches any known feature key.
pub fn is_known_feature_key(key: &str) -> bool {
    feature_for_key(key).is_some()
}

const FEATURES: &[FeatureSpec] = &[
    // Token-economy gates.
    FeatureSpec {
        id: Feature::WebSearch,
        key: "web_search",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::WebFetch,
        key: "web_fetch",
        stage: Stage::Stable,
        default_enabled: true,
    },
    // Behavior / safety gate.
    FeatureSpec {
        id: Feature::Sandbox,
        key: "sandbox",
        stage: Stage::Stable,
        default_enabled: false,
    },
    // /experimental menu candidates.
    FeatureSpec {
        id: Feature::AutoMemory,
        key: "auto_memory",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Retrieval,
        key: "retrieval",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::AgentTeams,
        key: "agent_teams",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Worktree,
        key: "worktree",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Lsp,
        key: "lsp",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
];

#[cfg(test)]
#[path = "features.test.rs"]
mod tests;
