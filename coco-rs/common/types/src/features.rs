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
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    // Token-economy gate (Stable, default=true).
    /// Expose the `web_search` tool to the model.
    WebSearch,
    /// Expose the `web_fetch` tool to the model.
    WebFetch,
    /// Expose MCP management tools and dynamic MCP server tool wrappers to the model.
    Mcp,
    /// Discover skills published by connected MCP servers and surface
    /// them through the SkillTool / slash-command registry.
    /// TS: `feature('MCP_SKILLS')` in `services/mcp/client.ts:117`.
    /// Requires [`Self::Mcp`] — discovery is a no-op on disconnected servers.
    McpSkills,
    /// Expose the `notebook_edit` tool to the model.
    NotebookEdit,
    /// V2 task tooling: expose `TaskCreate`/`TaskGet`/`TaskList`/`TaskUpdate`.
    /// When disabled, `TodoWrite` (V1) is exposed instead. `TaskOutput` and
    /// `TaskStop` operate on the background-task namespace (Bash
    /// `run_in_background`, agent spawns) and stay enabled regardless of
    /// this gate.
    /// TS: `isTodoV2Enabled()` in `utils/tasks.ts:133-139`.
    TaskV2,
    /// Lazy tool-schema loading via `ToolSearch`. When **on** (default),
    /// tools whose `Tool::should_defer() == true` are sent to the model
    /// name-only on turn 1 and discovered via the `ToolSearch` tool —
    /// either through the client-side `discovered_tool_names` patch
    /// (default path, every Provider) or Anthropic's server-side
    /// `tool_reference` expansion (when the model declares
    /// `Capability::ServerSideToolReference`). Saves a large chunk of
    /// the tools-array token budget on sessions with many MCP tools.
    ///
    /// When **off** (the TS `'standard'` mode equivalent), the
    /// `ToolSearch` tool is hidden from the model AND the deferral
    /// filter is short-circuited: every enabled tool gets its full
    /// schema in every request. Choose this when token budget is
    /// not a concern and you'd rather avoid the round-trip cost of
    /// `ToolSearch`.
    ///
    /// TS: `getToolSearchMode()` in `utils/toolSearch.ts:172-198`.
    /// coco-rs exposes only the binary `tst` / `standard` modes —
    /// `tst-auto` (threshold-gated by deferred-tool token count) is
    /// intentionally not ported; it depends on per-Provider token
    /// counting that pollutes the inference seam.
    ToolSearch,

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

    // Skill / command feature gates (TS `feature(...)` calls).
    /// `/loop` skill — recurring task scheduling.
    /// TS: `feature('AGENT_TRIGGERS')` in `skills/bundled/index.ts:47`.
    AgentTriggers,
    /// `/schedule` skill — remote agent scheduling.
    /// TS: `feature('AGENT_TRIGGERS_REMOTE')` in `skills/bundled/index.ts:56`.
    AgentTriggersRemote,
    /// `/claude-api` skill — Claude API/Anthropic SDK helper.
    /// TS: `feature('BUILDING_CLAUDE_APPS')` in `skills/bundled/index.ts:64`.
    BuildingClaudeApps,
    /// `/dream` skill — KAIROS auto-dream memory consolidation.
    /// TS: `feature('KAIROS') || feature('KAIROS_DREAM')` in `skills/bundled/index.ts:35`.
    KairosDream,
    /// `/hunter` skill — bug-finding review artifact.
    /// TS: `feature('REVIEW_ARTIFACT')` in `skills/bundled/index.ts:41`.
    ReviewArtifact,
    /// `/run-skill-generator` skill.
    /// TS: `feature('RUN_SKILL_GENERATOR')` in `skills/bundled/index.ts:73`.
    RunSkillGenerator,
    /// Auto-detect Claude in Chrome installation.
    /// TS: `shouldAutoEnableClaudeInChrome()` in `skills/bundled/index.ts:70`.
    ClaudeInChrome,
    /// `/init` new 8-phase prompt (vs old single-prompt).
    /// TS: `feature('NEW_INIT')` in `commands/init.ts:230`.
    NewInit,
    /// Reactive compaction strategy (vs traditional summarize-all).
    /// TS: `feature('REACTIVE_COMPACT')` in `commands/compact/compact.ts:35`.
    ReactiveCompact,
    /// Prompt-cache break detection wiring during compaction.
    /// TS: `feature('PROMPT_CACHE_BREAK_DETECTION')` in `commands/compact/compact.ts:67`.
    PromptCacheBreakDetection,
    /// Speculative pre-execution of accepted prompt suggestions.
    ///
    /// COW overlay filesystem at `<tmp>/speculation/<pid>/<id>/`,
    /// 3-boundary canUseTool (Edit/Write rewrites to overlay; Bash
    /// via shell-parser read-only check; deny default), MAX_TURNS=20
    /// / MAX_MESSAGES=100, accept/abort lifecycle.
    ///
    /// TS: `services/PromptSuggestion/speculation.ts` (gated upstream
    /// by `USER_TYPE='ant'`; coco-rs uses this Feature instead so
    /// non-Anthropic users can opt in via settings.json).
    ///
    /// **Default false** — experimental; high implementation
    /// complexity (overlay COW, 3-boundary classification,
    /// pipelined-suggestion forks). The Phase-1 `Allow{updated_input}`
    /// path-rewrite mechanism is in place to support this when
    /// the full overlay subsystem ships.
    Speculation,
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
    FeatureSpec {
        id: Feature::Mcp,
        key: "mcp",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::McpSkills,
        key: "mcp_skills",
        // TS marks MCP_SKILLS as experimental (GrowthBook-gated). coco-rs
        // mirrors with `UnderDevelopment` + default-off so a server that
        // publishes skills doesn't silently bypass user consent.
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
        id: Feature::TaskV2,
        key: "task_v2",
        stage: Stage::Stable,
        default_enabled: true,
    },
    FeatureSpec {
        id: Feature::ToolSearch,
        key: "tool_search",
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
        stage: Stage::Experimental {
            name: "Agent Teams",
            menu_description: "Spawn subagents and persistent teammates (Agent / TeamCreate / TeamDelete tools, /agents command)",
            announcement: "Agent teams enabled — use /agents to inspect the catalog and Agent(...) to spawn workers.",
        },
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
    // Skill / command bundled-feature gates (TS `feature(...)` parity).
    FeatureSpec {
        id: Feature::AgentTriggers,
        key: "agent_triggers",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::AgentTriggersRemote,
        key: "agent_triggers_remote",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::BuildingClaudeApps,
        key: "building_claude_apps",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::KairosDream,
        key: "kairos_dream",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::ReviewArtifact,
        key: "review_artifact",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::RunSkillGenerator,
        key: "run_skill_generator",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::ClaudeInChrome,
        key: "claude_in_chrome",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::NewInit,
        key: "new_init",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::ReactiveCompact,
        key: "reactive_compact",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::PromptCacheBreakDetection,
        key: "prompt_cache_break_detection",
        stage: Stage::UnderDevelopment,
        default_enabled: false,
    },
    FeatureSpec {
        id: Feature::Speculation,
        key: "speculation",
        stage: Stage::Experimental {
            name: "Speculation",
            menu_description: "Pre-execute accepted prompt suggestions in an overlay sandbox; instant inject on accept",
            announcement: "Speculation enabled — accepted prompt suggestions will run in an overlay before injection.",
        },
        default_enabled: false,
    },
];

#[cfg(test)]
#[path = "features.test.rs"]
mod tests;
