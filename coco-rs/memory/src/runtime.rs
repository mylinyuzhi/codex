//! [`MemoryRuntime`] — single entry point that composes the three
//! services. Sessions hold one `Arc<MemoryRuntime>` and call into it
//! at turn boundaries / shutdown.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use coco_paths::ProjectPaths;
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
use crate::service::dream::DreamOutcome;
use crate::service::extract::ExtractOutcome;
use crate::service::session::SessionMemoryOutcome;
use crate::store::EntrypointTruncation;
use crate::telemetry::MemoryEvent;
use crate::telemetry::MemoryTelemetryEmitter;
use crate::telemetry::NoopEmitter;

/// Telemetry source label for the recall ranker side-query.
const RECALL_QUERY_SOURCE: &str = "memory_recall";

/// Read a `MEMORY.md` index file, logging unexpected errors at debug
/// level. `ENOENT` is the expected "cold start" case and stays silent;
/// EACCES / EIO / etc. would otherwise be swallowed by `.ok()` and
/// surface as "no memory available" with no log trail.
async fn read_index_file(path: &std::path::Path) -> Option<String> {
    match tokio::fs::read_to_string(path).await {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            tracing::debug!(
                target: "coco_memory::runtime",
                path = %path.display(),
                error = %e,
                "MEMORY.md unreadable — memory section omitted"
            );
            None
        }
    }
}

/// Forced-tool name used to coerce the recall ranker into structured
/// output. Mirrors TS `selectRelevantMemories`'s `tool_choice` shape.
const RECALL_TOOL_NAME: &str = "select_memories";

/// Lazy enumerator returning session IDs that have produced
/// transcripts since the last consolidation. The auto-dream
/// scheduler invokes the closure **only** after the time + scan
/// throttle gates pass — TS parity with `listSessionsTouchedSince`,
/// which TS calls only after the time gate (`autoDream.ts:155`).
pub type SessionEnumerator = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

/// Composed memory runtime — one per session.
pub struct MemoryRuntime {
    pub directories: MemoryDir,
    pub config: MemoryConfig,
    pub extract: Arc<ExtractService>,
    pub dream: Arc<DreamService>,
    pub session_memory: Arc<SessionMemoryService>,
    /// Project session-transcript root — TS `getProjectDir(getOriginalCwd())`.
    /// Substituted into the dream prompt's grep examples and the
    /// optional searching-past-context section. `None` leaves the
    /// `{TRANSCRIPT_DIR}` placeholder in prompt copy.
    transcript_dir: Option<PathBuf>,
    /// Cross-turn recall state. Encapsulated — external callers reach
    /// it through [`MemoryRuntime::recall`] and [`MemoryRuntime::reset`].
    recall_state: Arc<PrefetchState>,
    /// Master swappable cell shared with every service. The CLI / SDK
    /// runner can [`MemoryRuntime::install_agent`] a real
    /// `SwarmAgentHandle` after the engine is built; until then the
    /// services see whatever was passed at build (typically
    /// `NoOpAgentHandle`). Sync `std::sync::RwLock` because read sites
    /// clone-and-drop the guard immediately — no `.await` required.
    agent_slot: crate::service::extract::AgentSlot,
    /// LLM ranker handle. `None` ⇒ recall falls back to the recency
    /// heuristic. Use [`MemoryRuntime::install_side_query`] to plug in
    /// a `coco-inference` adapter once it's built.
    ///
    /// `OnceLock` instead of `RwLock<Option<...>>` because installation
    /// is genuinely one-shot in production (called once by the CLI/SDK
    /// runner during bootstrap). Reads become a sync atomic load with
    /// no lock acquisition.
    side_query: OnceLock<SideQueryHandle>,
    /// Lazy session enumerator used by [`Self::tick_dream`]. The
    /// session-runtime wires this with a closure that reads the
    /// project's `TranscriptStore`. Absence ⇒ tick_dream sees an empty
    /// list, which keeps the session-gate as the limiting factor and
    /// matches the pre-wire baseline. Install-once via `OnceLock`.
    session_enumerator: OnceLock<SessionEnumerator>,
    /// Shared inbox for user-visible save notices. Extract / dream
    /// push into it on success; the engine drains it once per turn
    /// in `finalize_turn_post_tools` and injects a
    /// `SystemMemorySavedMessage` into history. TS parity:
    /// `appendSystemMessage(createMemorySavedMessage(...))`.
    notices: crate::notice::NoticeInbox,
    /// Telemetry emitter shared with the services. The runtime owns
    /// a clone so [`Self::render_system_prompt_section`] can fire
    /// `MemdirLoaded` directly — TS `tengu_memdir_loaded`.
    telemetry: Arc<dyn MemoryTelemetryEmitter>,
    /// Midnight-rollover latch for KAIROS mode. Inert outside KAIROS:
    /// `finalize_turn` only consults it when `config.kairos_mode` is
    /// set, so the watcher stays at its empty default and costs
    /// nothing for sessions that don't opt in. TS parity: the KAIROS
    /// arm of `getDateChangeAttachments`
    /// (`utils/attachments.ts:1437-1441`).
    kairos_rollover: crate::kairos::KairosRolloverWatcher,
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
    /// Optional pre-resolved project transcript directory (TS
    /// `getProjectDir(getOriginalCwd())`). Surfaces into the dream
    /// prompt's grep example and the searching-past-context block.
    pub transcript_dir: Option<PathBuf>,
    /// Whether auto-compact is active for this session — TS
    /// `isAutoCompactEnabled()` (`compact/autoCompact.ts`). Only
    /// affects telemetry: SM still constructs and the engine layer
    /// gates its dispatch separately. Defaults to `true` so legacy
    /// callers don't accidentally suppress the init event.
    pub auto_compact_enabled: bool,
    /// Optional pre-computed [`ProjectPaths`]. When `None`, [`Self::build`]
    /// derives one from `config_home + canonical(project_root)`. Callers
    /// that already hold an `Arc<ProjectPaths>` (e.g. `app-cli`'s
    /// `paths::project_paths`) supply it here so we don't re-do the
    /// canonical-git-root + slug walk a second time.
    pub project_paths: Option<Arc<ProjectPaths>>,
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
            transcript_dir: None,
            auto_compact_enabled: true,
            project_paths: None,
        }
    }

    /// Reuse a pre-computed [`ProjectPaths`] instead of building a
    /// fresh one in [`Self::build`]. Saves the canonical-git-root
    /// walk + slug NFC pass when the caller already holds a shared
    /// instance.
    pub fn with_project_paths(mut self, project_paths: Arc<ProjectPaths>) -> Self {
        self.project_paths = Some(project_paths);
        self
    }

    /// Tell the runtime whether auto-compact is active for this
    /// session — surfaced by the SM init telemetry event so dashboards
    /// can correlate SM activity with compact configuration.
    pub fn with_auto_compact_enabled(mut self, enabled: bool) -> Self {
        self.auto_compact_enabled = enabled;
        self
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

    /// Pre-resolve the project transcript directory (TS
    /// `getProjectDir`). When unset the prompt copy keeps the
    /// `{TRANSCRIPT_DIR}` placeholder for the model to fill.
    pub fn with_transcript_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.transcript_dir = Some(dir.into());
        self
    }

    pub fn build(self) -> MemoryRuntime {
        // TS `sessionMemory.ts:362-367 logEvent('tengu_session_memory_init', ...)`
        // — fired once at registration time so dashboards can count
        // SM-enabled sessions vs auto-compact-disabled ones.
        if self.config.session_memory_enabled {
            self.telemetry.emit(MemoryEvent::SessionMemoryInit {
                auto_compact_enabled: self.auto_compact_enabled,
            });
        }
        // `memory_base` is the root of the per-project memory layout
        // (`<base>/projects/<slug>/memory/`). `memory_base_override`
        // (from `COCO_REMOTE_MEMORY_DIR`) shifts it without touching
        // unrelated subsystems' paths — transcripts and the project
        // session dir still use the caller-supplied `project_paths`
        // unless this override is set, in which case we rebuild
        // memory-scoped `ProjectPaths` on top of the new base. TS
        // parity: `getMemoryBaseDir()` in `memdir/paths.ts:85-90`.
        let memory_base: PathBuf = self
            .config
            .memory_base_override
            .clone()
            .unwrap_or_else(|| self.config_home.clone());
        let directories = MemoryDir::resolve(
            &memory_base,
            &self.project_root,
            self.config.directory.as_deref(),
        );
        // When `memory_base_override` is set, the caller's
        // `project_paths` (computed from the default `config_home`)
        // would point session-memory + dream-lock to the wrong base.
        // Rebuild a memory-scoped `ProjectPaths` against the override
        // so the SM file lives at `<override>/projects/<slug>/<sid>/
        // session-memory/summary.md`.
        let project_paths = if self.config.memory_base_override.is_some() {
            let canonical = coco_git::find_canonical_git_root(&self.project_root)
                .unwrap_or_else(|| self.project_root.clone());
            Arc::new(ProjectPaths::new(memory_base, &canonical))
        } else {
            self.project_paths.unwrap_or_else(|| {
                let canonical = coco_git::find_canonical_git_root(&self.project_root)
                    .unwrap_or_else(|| self.project_root.clone());
                Arc::new(ProjectPaths::new(self.config_home.clone(), &canonical))
            })
        };
        // Master swappable cell — every service sees the same handle
        // and observes any later `install_agent` swap.
        let agent_slot: crate::service::extract::AgentSlot =
            Arc::new(std::sync::RwLock::new(self.agent.clone()));
        // Single shared notice inbox — `extract` and `dream` push
        // user-visible save notices here on success; the engine
        // drains via `MemoryRuntime::drain_user_notices()`. SM also
        // shares the inbox even though TS doesn't push from it,
        // keeping the API uniform if a future surface lands.
        let notices = crate::notice::NoticeInbox::default();
        let extract = Arc::new(ExtractService::with_shared_agent_and_notices(
            directories.personal.clone(),
            self.config.clone(),
            agent_slot.clone(),
            self.telemetry.clone(),
            notices.clone(),
        ));
        let dream = Arc::new(DreamService::with_shared_agent_and_notices(
            directories.personal.clone(),
            self.config.clone(),
            agent_slot.clone(),
            self.telemetry.clone(),
            notices.clone(),
        ));
        let session_memory = Arc::new(SessionMemoryService::with_shared_agent(
            project_paths,
            self.session_id,
            self.config.clone(),
            agent_slot.clone(),
            self.telemetry.clone(),
        ));
        let side_query_slot = OnceLock::new();
        if let Some(handle) = self.side_query {
            // Builder may pre-install — `set` is infallible on a fresh
            // OnceLock, so .ok() is just type-erasure of the result.
            let _ = side_query_slot.set(handle);
        }
        MemoryRuntime {
            directories,
            config: self.config,
            extract,
            dream,
            session_memory,
            transcript_dir: self.transcript_dir,
            recall_state: Arc::new(PrefetchState::new()),
            agent_slot,
            side_query: side_query_slot,
            session_enumerator: OnceLock::new(),
            notices,
            telemetry: self.telemetry,
            kairos_rollover: crate::kairos::KairosRolloverWatcher::new(),
        }
    }
}

impl MemoryRuntime {
    /// Replace the agent handle every service uses for forked spawns.
    /// Call this from the SDK / TUI runner once the real
    /// `SwarmAgentHandle` is built — until then services use whatever
    /// the builder received (typically `NoOpAgentHandle`). Sync now
    /// that the slot is `std::sync::RwLock`; existing callsites
    /// `.await` was a no-op so removing `async` from the signature
    /// produces no observable change beyond the lighter call.
    pub fn install_agent(&self, handle: coco_tool_runtime::AgentHandleRef) {
        *self
            .agent_slot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = handle;
    }

    /// Plug in a [`coco_tool_runtime::SideQueryHandle`] for the
    /// recall ranker. With a handle present, [`Self::recall`]
    /// dispatches a `ModelRole::Memory` side-query; without one it
    /// falls back to the recency heuristic.
    ///
    /// One-shot — a second call returns the handle back to the caller
    /// (`Err`). This matches the documented "call once at session
    /// bootstrap" contract and prevents accidental swap during a turn,
    /// which would surface as inconsistent ranker behavior mid-recall.
    pub fn install_side_query(&self, handle: SideQueryHandle) -> Result<(), SideQueryHandle> {
        self.side_query.set(handle)
    }

    /// Install a [`SessionEnumerator`] used by [`Self::tick_dream`].
    /// The session-runtime wires this with a closure backed by the
    /// project's `TranscriptStore`; before installation `tick_dream`
    /// sees an empty list and the session gate stays the limiting
    /// factor. One-shot — see [`Self::install_side_query`].
    pub fn install_session_enumerator(
        &self,
        enumerator: SessionEnumerator,
    ) -> Result<(), SessionEnumerator> {
        self.session_enumerator.set(enumerator)
    }

    /// Drain user-visible memory save notices accumulated since the
    /// last call. The engine invokes this from
    /// `finalize_turn_post_tools` and injects a
    /// `SystemMemorySavedMessage` into history for each entry. TS
    /// parity: `appendSystemMessage(createMemorySavedMessage(paths))`
    /// in `extractMemories.ts:495` and `autoDream.ts:243`.
    pub fn drain_user_notices(&self) -> Vec<crate::notice::MemoryUserNotice> {
        self.notices.drain()
    }

    /// Per-turn entry point for the memory subsystem. Aggregates the
    /// three async services (session memory, extract, auto-dream) plus
    /// future post-write inspection (Gap 4) and KAIROS rollover (Gap 2)
    /// into a single black-box call from the engine.
    ///
    /// Architecture: engine pre-computes everything that needs the
    /// `MessageHistory` (cursors, tool counts, fork closures) and
    /// passes them through [`FinalizeTurnContext`]; this method does
    /// the fan-out and post-processing and returns a typed
    /// [`FinalizeTurnReport`] the engine then projects into history
    /// (`SystemMemorySavedMessage` for each notice) and side effects
    /// (KAIROS transcript archive).
    ///
    /// Subagent + bare-mode gating is centralised here so callers
    /// don't need to remember the rules. Returns `skipped=true` when
    /// the gate trips; no LLM call is made.
    pub async fn finalize_turn(&self, ctx: FinalizeTurnContext) -> FinalizeTurnReport {
        if ctx.bare_mode || ctx.is_subagent {
            return FinalizeTurnReport::skipped();
        }

        let extract = self.extract.clone();
        let session_memory = self.session_memory.clone();
        let auto_compact_enabled = ctx.auto_compact_enabled;
        let estimated_tokens = ctx.estimated_tokens;
        let tool_calls_since_sm = ctx.tool_calls_since_sm_cursor;
        let had_tool_calls_in_last_turn = ctx.tool_calls_last_turn > 0;
        let last_msg_id = ctx.last_message_id.clone();
        let extract_input = ctx.extract_input;
        let now_ms = ctx.now_ms;

        // Fan-out — three forks in parallel. Each service gates
        // internally; the lazy `fork_messages` closure inside
        // `extract_input` is only invoked once all extract gates pass.
        // TS parity: `stopHooks.ts` dispatches the three concurrently.
        let (sm_outcome, ex_outcome, dr_outcome) = tokio::join!(
            async {
                if auto_compact_enabled {
                    session_memory
                        .maybe_extract(
                            estimated_tokens,
                            tool_calls_since_sm,
                            had_tool_calls_in_last_turn,
                            last_msg_id,
                        )
                        .await
                } else {
                    SessionMemoryOutcome::Skipped(crate::service::session::SkipReason::Disabled)
                }
            },
            extract.maybe_extract(extract_input),
            self.tick_dream(now_ms),
        );

        // Post-write classification — when the main agent (or user
        // through the `Edit`/`Write`/`NotebookEdit` tool) directly
        // wrote a memory-managed file this turn, surface a
        // `ManualEdit` notice. Dedup by path so a model that hits the
        // same file 5 times only generates one toast. TS parity:
        // `services/useMemoryUpdateNotification` +
        // `utils/memoryFileDetection.ts::detectSessionFileType`.
        let session_memory_file = self.session_memory.file_path();
        let mut dedup: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut manual_edit_paths: Vec<String> = Vec::new();
        for record in &ctx.recent_tool_writes {
            if !record.succeeded {
                continue;
            }
            match crate::path::classify_written_path(
                &record.file_path,
                &self.directories.personal,
                Some(&session_memory_file),
            ) {
                crate::path::WriteClassification::TeamMem
                | crate::path::WriteClassification::AutoMem
                | crate::path::WriteClassification::Claudemd => {
                    let key = record.file_path.display().to_string();
                    if dedup.insert(key.clone()) {
                        manual_edit_paths.push(key);
                    }
                }
                // SessionMem updates come from the SM fork, which
                // already produces its own paths via the engine's
                // `SessionMemoryExtracted` event — no notice.
                // Unrelated is a no-op.
                _ => {}
            }
        }
        if !manual_edit_paths.is_empty() {
            self.notices.push(crate::notice::MemoryUserNotice {
                written_paths: manual_edit_paths,
                verb: crate::notice::NoticeVerb::ManualEdit,
            });
        }

        // KAIROS rollover detection (Gap 2): poll the watcher only when
        // KAIROS mode is on. The watcher seeds on its first tick, so
        // calling it every turn outside KAIROS would spin the latch for
        // no benefit; the conditional keeps the watcher inert until
        // `kairos_mode` flips. Emit telemetry on rollover so dashboards
        // pick it up; the engine receives `Some(yesterday)` and can act
        // (archive a session-transcript bucket, etc.) — TS-private
        // `sessionTranscript.flushOnDateChange` lives downstream of
        // this signal.
        let kairos_rollover = if self.config.kairos_mode {
            let yesterday = self.kairos_rollover.tick(now_ms);
            if let Some(prev) = yesterday {
                let today = prev.succ_opt().unwrap_or(prev);
                self.telemetry.emit(MemoryEvent::KairosRollover {
                    yesterday: prev.format("%Y-%m-%d").to_string(),
                    today: today.format("%Y-%m-%d").to_string(),
                });
            }
            yesterday
        } else {
            None
        };

        FinalizeTurnReport {
            skipped: false,
            session_memory: Some(sm_outcome),
            extract: Some(ex_outcome),
            dream: Some(dr_outcome),
            kairos_rollover,
            notices: self.drain_user_notices(),
        }
    }

    /// Per-turn auto-dream tick — TS parity with `executeAutoDream`
    /// fired from `handleStopHooks`. Runs the three-gate scheduler
    /// (time → scan throttle → session); `enumerate_sessions` is the
    /// installed enumerator (see [`Self::install_session_enumerator`])
    /// and is invoked **only** when the gates require it. Without an
    /// enumerator the call effectively no-ops.
    pub async fn tick_dream(&self, now_ms: i64) -> crate::service::dream::DreamOutcome {
        let enumerator = self.session_enumerator.get().cloned();
        let transcript_dir = self
            .transcript_dir
            .clone()
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        self.dream
            .maybe_consolidate(
                &transcript_dir,
                move || match enumerator {
                    Some(e) => e(),
                    None => Vec::new(),
                },
                now_ms,
            )
            .await
    }

    /// Reset per-conversation state across all services + recall.
    /// Called from the CLI's `/clear` flow so a cleared conversation
    /// doesn't drag the prior already-surfaced set / extraction
    /// cursor / session-memory init flag into the next round. The
    /// on-disk MEMORY.md and topic files are left alone — those are
    /// genuinely cross-conversation memory.
    ///
    /// `DreamService` is intentionally **not** reset: its 24h time
    /// gate + scan-throttle + PID lock are global-cadence concerns,
    /// not per-conversation state. A user running `/clear` shouldn't
    /// re-pay the cost of a multi-minute consolidation, and TS's
    /// `executeAutoDream` doesn't touch its scheduler state on
    /// `clearConversation` either (`services/autoDream/autoDream.ts`
    /// keeps state in module-scope closures across resets).
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

    /// Project session-transcript directory (TS
    /// `getProjectDir(getOriginalCwd())`). `None` when callers built
    /// the runtime without `with_transcript_dir(..)` — prompt copy
    /// then keeps the `{TRANSCRIPT_DIR}` placeholder.
    pub fn transcript_dir(&self) -> Option<&Path> {
        self.transcript_dir.as_deref()
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

        // Truncate-and-keep-stats so we can emit `MemdirLoaded`. TS
        // `tengu_memdir_loaded` fires every time the prompt section
        // is built (`memdir.ts:298-305`) — without this dashboards
        // can't measure how often / how large the memdir is per
        // session, which is the load-bearing input for the recall
        // budget heuristics.
        let personal_trunc: Option<EntrypointTruncation> =
            read_index_file(&self.directories.personal_index())
                .await
                .map(|s| truncate_entrypoint_content(&s));
        let has_team = matches!(variant, SystemPromptVariant::Combined);
        let team_trunc: Option<EntrypointTruncation> = if has_team {
            read_index_file(&self.directories.team_index())
                .await
                .map(|s| truncate_entrypoint_content(&s))
        } else {
            None
        };

        // Emit per-dir telemetry — TS fires twice in combined mode
        // (once per dir). One event with `has_team=true` summarizes
        // the personal-side stats; team's stats ride on a second
        // event so both surfaces stay measurable.
        if let Some(trunc) = &personal_trunc {
            self.telemetry.emit(MemoryEvent::MemdirLoaded {
                line_count: trunc.line_count as i64,
                byte_count: trunc.byte_count as i64,
                was_truncated: trunc.line_truncated,
                was_byte_truncated: trunc.byte_truncated,
                has_team,
            });
        }
        if let Some(trunc) = &team_trunc {
            self.telemetry.emit(MemoryEvent::MemdirLoaded {
                line_count: trunc.line_count as i64,
                byte_count: trunc.byte_count as i64,
                was_truncated: trunc.line_truncated,
                was_byte_truncated: trunc.byte_truncated,
                has_team: true,
            });
        }

        let personal_index = personal_trunc.map(|t| t.content);
        let team_index = team_trunc.map(|t| t.content);

        let transcript_dir = self.transcript_dir.as_deref();
        Some(build_system_prompt_section(
            variant,
            &self.directories.personal,
            if has_team {
                Some(&self.directories.team)
            } else {
                None
            },
            personal_index.as_deref(),
            team_index.as_deref(),
            self.config.skip_index,
            self.config.searching_past_context_enabled,
            transcript_dir,
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
        // Cold-start short-circuit: gate on the scan being empty, NOT
        // on `MEMORY.md`'s presence. TS `findRelevantMemories.ts:46`
        // short-circuits via `scanMemoryFiles(...).length === 0`. A
        // user who has topic files but deleted (or never had) the
        // `MEMORY.md` index still has memories worth surfacing.
        let scanned = scan_memory_files(&self.directories.personal);
        if scanned.is_empty() {
            return Vec::new();
        }

        let side_query = self.side_query.get().cloned();
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
                .with_model_role(ModelRole::Memory)
                // TS `findRelevantMemories.ts:101`
                // `skipSystemPromptPrefix: true` — ranker must not
                // see the main agent's Claude Code preamble. The
                // preamble describes tools/persona unrelated to
                // memory selection and biases the ranker.
                .with_skip_system_prefix(true);
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
                        // by matching against the scanned manifest via a
                        // hash index — O(k) instead of O(n·k).
                        let by_name: std::collections::HashMap<&str, &str> = scanned
                            .iter()
                            .map(|m| (m.filename.as_str(), m.path.to_str().unwrap_or("")))
                            .collect();
                        names
                            .into_iter()
                            .filter_map(|name| {
                                by_name
                                    .get(name.as_str())
                                    .filter(|p| !p.is_empty())
                                    .map(|p| (*p).to_string())
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

/// One main-agent tool call that may have written to disk. The engine
/// extracts these from each finalised turn and passes them into
/// [`MemoryRuntime::finalize_turn`] so memory's `classify_tool_write`
/// pass (Gap 4) can decide whether to emit a `ManualEdit` notice.
#[derive(Debug, Clone)]
pub struct ToolWriteRecord {
    pub tool_name: String,
    pub file_path: PathBuf,
    /// Whether the tool call returned success. Failed writes don't
    /// produce notices (the file wasn't actually changed).
    pub succeeded: bool,
}

/// Inputs to [`MemoryRuntime::finalize_turn`].
///
/// The engine pre-computes every field that depends on
/// `MessageHistory` (cursors, tool-call counts, the fork-messages
/// closure and the `has_memory_writes` closure inside `extract_input`)
/// and hands them through this struct. The runtime then orchestrates
/// the fan-out without re-walking history.
pub struct FinalizeTurnContext {
    /// Estimated token count of the current history (SM init/update gate).
    pub estimated_tokens: i64,
    /// Cumulative tool-call count since SM's last extraction cursor.
    pub tool_calls_since_sm_cursor: i32,
    /// Tool-call count in the last assistant turn — drives SM's
    /// natural-break heuristic.
    pub tool_calls_last_turn: i32,
    /// UUID of the **last** message in history (any kind, not just
    /// assistant). Becomes the new cursor on a successful extraction
    /// for both SM and extract.
    pub last_message_id: Option<String>,
    /// `is_auto_compact_active` snapshot — when off, SM dispatch is
    /// skipped entirely (its primary consumer is the SM-first compact
    /// branch).
    pub auto_compact_enabled: bool,
    /// `--bare` / SDK headless mode flag. Suppresses every memory
    /// fork so scripted invocations don't pay turn-end LLM costs.
    pub bare_mode: bool,
    /// True when this turn is running inside a subagent. Subagents
    /// inherit but don't ADD to the parent's auto-memory.
    pub is_subagent: bool,
    /// Wall-clock at finalize time (passed in so tests stay
    /// deterministic). Used for dream's time-gate + KAIROS rollover.
    pub now_ms: i64,
    /// Pre-built `TurnInput` for the extraction service. Holds the
    /// lazy `fork_messages` and `has_memory_writes` closures the engine
    /// captured against the history.
    pub extract_input: crate::service::extract::TurnInput,
    /// Main-agent writes this turn — picked up by Gap 4 toast.
    pub recent_tool_writes: Vec<ToolWriteRecord>,
}

/// Result of one [`MemoryRuntime::finalize_turn`] call.
///
/// When `skipped == true` (bare mode or subagent), every other field
/// is its `None` / empty default and the engine should not project
/// anything into history.
pub struct FinalizeTurnReport {
    pub skipped: bool,
    pub session_memory: Option<SessionMemoryOutcome>,
    pub extract: Option<ExtractOutcome>,
    pub dream: Option<DreamOutcome>,
    /// Yesterday's date when KAIROS rollover fired this turn. The
    /// engine archives the prior session transcript bucket on this
    /// signal. Always `None` outside KAIROS mode.
    pub kairos_rollover: Option<chrono::NaiveDate>,
    /// User-visible "memory saved/improved/manually-edited/log-appended"
    /// notices accumulated this turn. The engine projects one
    /// `SystemMemorySavedMessage` per entry.
    pub notices: Vec<crate::notice::MemoryUserNotice>,
}

impl FinalizeTurnReport {
    pub fn skipped() -> Self {
        Self {
            skipped: true,
            session_memory: None,
            extract: None,
            dream: None,
            kairos_rollover: None,
            notices: Vec::new(),
        }
    }
}
