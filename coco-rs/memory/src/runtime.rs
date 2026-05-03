//! [`MemoryRuntime`] — single entry point that composes the three
//! services. Sessions hold one `Arc<MemoryRuntime>` and call into it
//! at turn boundaries / shutdown.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

const SESSION_MEMORY_SUBDIR: &str = "session-memory";

use coco_tool_runtime::AgentHandleRef;
use coco_tool_runtime::SideQueryHandle;
use coco_tool_runtime::SideQueryRequest;
use coco_types::ModelRole;
use coco_types::SideQueryToolDef;

use crate::config::MemoryConfig;
use crate::path::MemoryDir;
use crate::recall::PrefetchState;
use crate::recall::RelevantMemory;
use crate::recall::SELECT_MEMORIES_SYSTEM_PROMPT;
use crate::recall::build_selection_prompt;
use crate::recall::load_relevant_memories;
use crate::recall::parse_selection_response;
use crate::recall::select_heuristic;
use crate::scan::scan_memory_files;
use crate::service::DreamService;
use crate::service::ExtractService;
use crate::service::SessionMemoryService;
use crate::telemetry::MemoryTelemetryEmitter;
use crate::telemetry::NoopEmitter;

/// Telemetry source label for the recall ranker side-query.
const RECALL_QUERY_SOURCE: &str = "memory_recall";

/// Forced-tool name used to coerce the recall ranker into structured
/// output. Mirrors TS `selectRelevantMemories`'s `tool_choice` shape.
const RECALL_TOOL_NAME: &str = "select_memories";

/// Composed memory runtime — one per session.
pub struct MemoryRuntime {
    pub directories: MemoryDir,
    pub config: MemoryConfig,
    pub extract: Arc<ExtractService>,
    pub dream: Arc<DreamService>,
    pub session_memory: Arc<SessionMemoryService>,
    /// Cross-turn recall state. Encapsulated — external callers reach
    /// it through [`MemoryRuntime::recall`] and [`MemoryRuntime::reset`].
    recall_state: Arc<PrefetchState>,
    /// Master swappable cell shared with every service. The CLI / SDK
    /// runner can [`MemoryRuntime::install_agent`] a real
    /// `SwarmAgentHandle` after the engine is built; until then the
    /// services see whatever was passed at build (typically
    /// `NoOpAgentHandle`).
    agent_slot: crate::service::extract::AgentSlot,
    /// LLM ranker handle. `None` ⇒ recall falls back to the recency
    /// heuristic. Use [`MemoryRuntime::install_side_query`] to plug in
    /// a `coco-inference` adapter once it's built.
    side_query: tokio::sync::RwLock<Option<SideQueryHandle>>,
}

impl std::fmt::Debug for MemoryRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryRuntime")
            .field("directories", &self.directories)
            .field("session_memory_file", &self.session_memory.file_path())
            .finish_non_exhaustive()
    }
}

/// Convenience builder for [`MemoryRuntime`]. Pass it `config_home`
/// (typically `~/.coco`), the project root, and the agent handle —
/// it derives all the directory paths.
pub struct MemoryRuntimeBuilder {
    pub config_home: PathBuf,
    pub project_root: PathBuf,
    pub session_id: String,
    pub config: MemoryConfig,
    pub agent: AgentHandleRef,
    pub telemetry: Arc<dyn MemoryTelemetryEmitter>,
    pub side_query: Option<SideQueryHandle>,
}

impl MemoryRuntimeBuilder {
    pub fn new(
        config_home: impl Into<PathBuf>,
        project_root: impl Into<PathBuf>,
        session_id: impl Into<String>,
        config: MemoryConfig,
        agent: AgentHandleRef,
    ) -> Self {
        Self {
            config_home: config_home.into(),
            project_root: project_root.into(),
            session_id: session_id.into(),
            config,
            agent,
            telemetry: Arc::new(NoopEmitter),
            side_query: None,
        }
    }

    pub fn with_telemetry(mut self, telemetry: Arc<dyn MemoryTelemetryEmitter>) -> Self {
        self.telemetry = telemetry;
        self
    }

    /// Plug in the LLM ranker. When set, `MemoryRuntime::recall`
    /// dispatches a `ModelRole::Memory` side-query to pick the top-K
    /// relevant memories instead of falling back to recency.
    pub fn with_side_query(mut self, side_query: SideQueryHandle) -> Self {
        self.side_query = Some(side_query);
        self
    }

    pub fn build(self) -> MemoryRuntime {
        let directories = MemoryDir::resolve(
            &self.config_home,
            &self.project_root,
            self.config.directory.as_deref(),
        );
        let session_memory_dir = self.config_home.join(SESSION_MEMORY_SUBDIR);
        // Master swappable cell — every service sees the same handle
        // and observes any later `install_agent` swap.
        let agent_slot: crate::service::extract::AgentSlot =
            Arc::new(tokio::sync::RwLock::new(self.agent.clone()));
        let extract = Arc::new(ExtractService::with_shared_agent(
            directories.personal.clone(),
            self.config.clone(),
            agent_slot.clone(),
            self.telemetry.clone(),
        ));
        let dream = Arc::new(DreamService::with_shared_agent(
            directories.personal.clone(),
            self.config.clone(),
            agent_slot.clone(),
            self.telemetry.clone(),
        ));
        let session_memory = Arc::new(SessionMemoryService::with_shared_agent(
            self.session_id,
            session_memory_dir,
            self.config.clone(),
            agent_slot.clone(),
            self.telemetry.clone(),
        ));
        MemoryRuntime {
            directories,
            config: self.config,
            extract,
            dream,
            session_memory,
            recall_state: Arc::new(PrefetchState::new()),
            agent_slot,
            side_query: tokio::sync::RwLock::new(self.side_query),
        }
    }
}

impl MemoryRuntime {
    /// Replace the agent handle every service uses for forked spawns.
    /// Call this from the SDK / TUI runner once the real
    /// `SwarmAgentHandle` is built — until then services use whatever
    /// the builder received (typically `NoOpAgentHandle`).
    pub async fn install_agent(&self, handle: coco_tool_runtime::AgentHandleRef) {
        *self.agent_slot.write().await = handle;
    }

    /// Plug in a [`coco_tool_runtime::SideQueryHandle`] for the
    /// recall ranker. With a handle present, [`Self::recall`]
    /// dispatches a `ModelRole::Memory` side-query; without one it
    /// falls back to the recency heuristic.
    pub async fn install_side_query(&self, handle: SideQueryHandle) {
        *self.side_query.write().await = Some(handle);
    }

    /// Reset per-conversation state across all services + recall.
    /// Called from the CLI's `/clear` flow so a cleared conversation
    /// doesn't drag the prior already-surfaced set / extraction
    /// cursor / session-memory init flag into the next round. The
    /// on-disk MEMORY.md and topic files are left alone — those are
    /// genuinely cross-conversation memory.
    pub async fn reset(&self) {
        self.recall_state.reset();
        self.extract.reset().await;
        self.session_memory.reset().await;
    }

    /// Convenience — current personal memory directory.
    pub fn personal_dir(&self) -> &Path {
        &self.directories.personal
    }

    /// Convenience — current team memory directory.
    pub fn team_dir(&self) -> &Path {
        &self.directories.team
    }

    /// Render the auto-memory system-prompt block for this session.
    ///
    /// Reads `MEMORY.md` (and team `MEMORY.md` when team mode is on),
    /// truncates to caps, and concatenates the verbatim type-taxonomy,
    /// how-to-save, and when-to-access blocks. The caller threads the
    /// returned string into `coco_context::build_system_prompt`'s
    /// `memory_section` slot.
    pub async fn render_system_prompt_section(&self) -> Option<String> {
        use crate::prompt::SystemPromptVariant;
        use crate::prompt::build_system_prompt_section;
        use crate::store::truncate_entrypoint_content;

        let variant = if self.config.kairos_mode {
            SystemPromptVariant::Kairos
        } else if self.config.team_memory_enabled {
            SystemPromptVariant::Combined
        } else {
            SystemPromptVariant::Auto
        };

        let personal_index = tokio::fs::read_to_string(self.directories.personal_index())
            .await
            .ok()
            .map(|s| truncate_entrypoint_content(&s).content);
        let team_index = if matches!(variant, SystemPromptVariant::Combined) {
            tokio::fs::read_to_string(self.directories.team_index())
                .await
                .ok()
                .map(|s| truncate_entrypoint_content(&s).content)
        } else {
            None
        };

        Some(build_system_prompt_section(
            variant,
            &self.directories.personal,
            if matches!(variant, SystemPromptVariant::Combined) {
                Some(&self.directories.team)
            } else {
                None
            },
            personal_index.as_deref(),
            team_index.as_deref(),
            self.config.skip_index,
            None,
        ))
    }

    /// Recall the top-K relevant memories for `query`.
    ///
    /// When a [`SideQueryHandle`] is wired through, this issues a
    /// [`ModelRole::Memory`] side-query that ranks the manifest and
    /// returns up to 5 filenames; the returned files are loaded with
    /// freshness headers and per-session byte-budget enforcement
    /// applied via [`PrefetchState`]. When no handle is present (e.g.
    /// the harness ran without inference), falls back to a recency
    /// heuristic so memory still surfaces something rather than
    /// nothing.
    ///
    /// `recent_tools` lets the ranker deprioritize reference docs for
    /// tools the model is actively exercising — TS parity.
    pub async fn recall(&self, query: &str, recent_tools: &[String]) -> Vec<RelevantMemory> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        // Cold-start short-circuit: with no MEMORY.md the directory
        // either doesn't exist or holds nothing curated, so skip the
        // full directory walk + 200 frontmatter reads.
        if !self.directories.personal_index().exists() {
            return Vec::new();
        }
        let scanned = scan_memory_files(&self.directories.personal);
        if scanned.is_empty() {
            return Vec::new();
        }

        let side_query = self.side_query.read().await.clone();
        let selected: Vec<String> = match side_query {
            Some(handle) => {
                let user_prompt =
                    build_selection_prompt(query, &scanned, &self.recall_state, recent_tools);
                // Force structured output via a synthetic
                // `select_memories` tool — TS parity with
                // `selectRelevantMemories.ts`'s `tool_choice: { type:
                // "tool", name: "select_memories" }`. Strict JSON
                // shape is more reliable than a permissive
                // `parse_selection_response` regex over free text.
                let tool = SideQueryToolDef {
                    name: RECALL_TOOL_NAME.into(),
                    description:
                        "Return up to 5 memory filenames most relevant to the user's query.".into(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "selected_memories": {
                                "type": "array",
                                "items": { "type": "string" },
                                "maxItems": 5,
                            }
                        },
                        "required": ["selected_memories"],
                        "additionalProperties": false,
                    }),
                };
                let request = SideQueryRequest::with_forced_tool(
                    SELECT_MEMORIES_SYSTEM_PROMPT,
                    &user_prompt,
                    tool,
                    RECALL_QUERY_SOURCE,
                )
                .with_model_role(ModelRole::Memory);
                match handle.query(request).await {
                    Ok(resp) => {
                        // Prefer the structured tool input; fall back
                        // to text-mode parsing for providers that
                        // don't honor `tool_choice` (TS legacy path).
                        let names = resp
                            .tool_uses
                            .first()
                            .and_then(|tu| tu.input.get("selected_memories"))
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|s| s.as_str().map(str::to_string))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_else(|| {
                                let text = resp.text.clone().unwrap_or_default();
                                parse_selection_response(&text)
                            });
                        // Ranker returns filenames; resolve to absolute paths
                        // by matching against the scanned manifest.
                        names
                            .into_iter()
                            .filter_map(|name| {
                                scanned
                                    .iter()
                                    .find(|m| m.filename == name)
                                    .map(|m| m.path.to_string_lossy().into_owned())
                            })
                            .collect()
                    }
                    Err(err) => {
                        tracing::debug!("memory recall ranker failed, falling back: {err}");
                        select_heuristic(&scanned, &self.recall_state)
                    }
                }
            }
            None => select_heuristic(&scanned, &self.recall_state),
        };

        load_relevant_memories(&selected, &self.recall_state)
    }
}
