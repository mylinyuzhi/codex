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
    /// Should not be used; kept for backward compatibility.
    Deprecated,
    /// Feature flag is a no-op; kept so old configs still parse.
    Removed,
}

impl Stage {
    pub fn experimental_menu_name(self) -> Option<&'static str> {
        match self {
            Stage::Experimental { name, .. } => Some(name),
            Stage::UnderDevelopment | Stage::Stable | Stage::Deprecated | Stage::Removed => None,
        }
    }

    pub fn experimental_menu_description(self) -> Option<&'static str> {
        match self {
            Stage::Experimental {
                menu_description, ..
            } => Some(menu_description),
            Stage::UnderDevelopment | Stage::Stable | Stage::Deprecated | Stage::Removed => None,
        }
    }

    pub fn experimental_announcement(self) -> Option<&'static str> {
        match self {
            Stage::Experimental {
                announcement: "", ..
            } => None,
            Stage::Experimental { announcement, .. } => Some(announcement),
            Stage::UnderDevelopment | Stage::Stable | Stage::Deprecated | Stage::Removed => None,
        }
    }
}

/// Unique features toggled via configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    // Stable.
    /// Create a ghost commit at each turn.
    GhostCommit,
    /// Enable file checkpointing (rewind support). When disabled, no
    /// file backups or snapshots are created.
    FileCheckpointing,

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
    /// Gate the NotebookEdit tool behind opt-in config.
    NotebookEdit,
    /// Enable interview-style plan mode (iterative pair-planning).
    PlanModeInterview,
    /// Enable structured task management (TaskCreate/TaskUpdate/TaskGet/TaskList).
    /// Mutually exclusive with TodoWrite — when enabled, TodoWrite is hidden.
    StructuredTasks,
    /// Enable cron/scheduling tools (CronCreate/CronDelete/CronList).
    Cron,
    /// Enable worktree tools (EnterWorktree/ExitWorktree).
    Worktree,
    /// Enable background task execution (Task tool's `run_in_background` parameter).
    /// When disabled, all tasks run in the foreground regardless of the parameter.
    BackgroundTasks,
    /// Enable auto memory (MEMORY.md per-project persistence).
    AutoMemory,
    /// Enable relevant memories system reminder (semantic search).
    RelevantMemories,
    /// Enable background memory extraction agent.
    MemoryExtraction,
    /// Enable team memory (shared memory across organization members).
    /// Adds a `team/` subdirectory under the auto memory directory
    /// with its own MEMORY.md index and topic files.
    TeamMemory,
    /// Enable user-customizable keybindings via `keybindings.json`.
    KeybindingCustomization,
    /// Enable IDE integration (MCP connection to IDE extensions).
    Ide,
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

    /// Normalize feature dependencies (auto-enable prerequisites).
    ///
    /// For example, `RelevantMemories` requires `AutoMemory`, and
    /// `MemoryExtraction` requires `AutoMemory`.
    pub fn normalize_dependencies(&mut self) {
        if self.enabled(Feature::RelevantMemories) && !self.enabled(Feature::AutoMemory) {
            self.enable(Feature::AutoMemory);
        }
        if self.enabled(Feature::MemoryExtraction) && !self.enabled(Feature::AutoMemory) {
            self.enable(Feature::AutoMemory);
        }
        if self.enabled(Feature::TeamMemory) && !self.enabled(Feature::AutoMemory) {
            self.enable(Feature::AutoMemory);
        }
        if self.enabled(Feature::WindowsSandboxElevated) && !self.enabled(Feature::WindowsSandbox) {
            self.enable(Feature::WindowsSandbox);
        }
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
    FeatureSpec {
        id: Feature::FileCheckpointing,
        key: "file_checkpointing",
        stage: Stage::Stable,
        default_enabled: true,
    },
    // Under development: not shown in menus or announcements.
    FeatureSpec {
        id: Feature::HierarchicalAgents,
        key: "hierarchical_agents",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::WindowsSandbox,
        key: "experimental_windows_sandbox",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::WindowsSandboxElevated,
        key: "elevated_windows_sandbox",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::PowershellUtf8,
        key: "powershell_utf8",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Collab,
        key: "collab",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::WebFetch,
        key: "web_fetch",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::WebSearch,
        key: "web_search",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::Retrieval,
        key: "code_search",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Lsp,
        key: "lsp",
        stage: Stage::UnderDevelopment,
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
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::NotebookEdit,
        key: "notebook_edit",
        stage: Stage::Stable,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::PlanModeInterview,
        key: "plan_mode_interview",
        stage: Stage::Experimental {
            name: "Plan Mode Interview",
            menu_description: "Iterative pair-planning with Q&A instead of 5-phase workflow",
            announcement: "Plan mode interview phase is now available",
        },
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::StructuredTasks,
        key: "structured_tasks",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Cron,
        key: "cron",
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
        id: Feature::BackgroundTasks,
        key: "background_tasks",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::AutoMemory,
        key: "auto_memory",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::RelevantMemories,
        key: "relevant_memories",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::MemoryExtraction,
        key: "memory_extraction",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::TeamMemory,
        key: "team_memory",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::KeybindingCustomization,
        key: "keybinding_customization",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Ide,
        key: "ide",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
];

#[cfg(test)]
#[path = "features.test.rs"]
mod tests;
