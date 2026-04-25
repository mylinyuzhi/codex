//! Central store for agent definitions. Built-ins + per-source loaders feed
//! the store; `snapshot()` returns an immutable per-turn view.
//!
//! TS: `loadAgentsDir.ts:193-221` `getActiveAgentsFromList`.
//!
//! Sync-only: `load()` and `reload()` walk the filesystem with `std::fs`.
//! No tokio, no watcher — `app/state` owns the watcher and calls
//! `reload()` on file changes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use coco_frontmatter::parse;
use coco_types::{AgentDefinition, AgentSource};

use crate::builtins::{BuiltinAgentCatalog, builtin_definitions};
use crate::frontmatter::parse_agent_markdown;
use crate::snapshot::AgentCatalogSnapshot;
use crate::validation::{AgentDefinitionValidator, ValidationDiagnostic, ValidationError};

/// Filename comparator for deterministic agent enumeration. `read_dir`
/// returns OS-dependent order (inode order on ext4, alphabetic on btrfs,
/// arbitrary on Windows/APFS). Sort by file path so same-priority
/// collisions resolve identically across platforms.
fn sorted_md_paths(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .collect();
    paths.sort();
    Ok(paths)
}

/// One loaded definition with its provenance recorded.
///
/// Per-file warnings live on `AgentLoadReport.warnings` (paired with the
/// path); they are not duplicated here to avoid two diverging copies after
/// `insert_definition` and similar mutators.
#[derive(Debug, Clone)]
pub struct LoadedAgentDefinition {
    pub definition: AgentDefinition,
    /// Absolute file path for markdown agents, `None` for in-process / built-in.
    pub path: Option<PathBuf>,
}

/// What `AgentDefinitionStore::load()` returns alongside the snapshot —
/// useful for `/agents validate` and bootstrap diagnostics.
#[derive(Debug, Default, Clone)]
pub struct AgentLoadReport {
    pub failed: Vec<ValidationDiagnostic>,
    pub warnings: Vec<ValidationDiagnostic>,
}

impl AgentLoadReport {
    /// No diagnostics at all — every file loaded with no issues.
    pub fn is_silent(&self) -> bool {
        self.failed.is_empty() && self.warnings.is_empty()
    }

    /// No load failures. Recoverable warnings (e.g. invalid color dropped)
    /// do not count as failures because the affected definition still
    /// loaded and is usable.
    pub fn has_failures(&self) -> bool {
        !self.failed.is_empty()
    }
}

/// Where to look for markdown agent files. Order is informational only —
/// precedence is driven by `AgentSource::priority`, not directory order.
#[derive(Debug, Clone, Default)]
pub struct AgentSearchPaths {
    pub user_dir: Option<PathBuf>,
    pub project_dirs: Vec<PathBuf>,
    pub flag_dirs: Vec<PathBuf>,
    pub policy_dirs: Vec<PathBuf>,
    pub plugin_dirs: Vec<PathBuf>,
}

impl AgentSearchPaths {
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Aggregates built-ins, plugins, user, project, flag, and policy agents
/// into a single catalog with TS-parity precedence.
pub struct AgentDefinitionStore {
    catalog: BuiltinAgentCatalog,
    paths: AgentSearchPaths,
    snapshot: Arc<AgentCatalogSnapshot>,
    last_report: AgentLoadReport,
}

impl AgentDefinitionStore {
    /// Construct an empty store. Call `load()` once to populate the snapshot.
    pub fn new(catalog: BuiltinAgentCatalog, paths: AgentSearchPaths) -> Self {
        Self {
            catalog,
            paths,
            snapshot: Arc::new(AgentCatalogSnapshot::new(BTreeMap::new(), Vec::new())),
            last_report: AgentLoadReport::default(),
        }
    }

    /// Returns the current snapshot. Cheap pointer clone — no per-turn
    /// allocations even when callers re-snapshot every turn.
    pub fn snapshot(&self) -> Arc<AgentCatalogSnapshot> {
        Arc::clone(&self.snapshot)
    }

    pub fn last_report(&self) -> &AgentLoadReport {
        &self.last_report
    }

    pub fn catalog(&self) -> BuiltinAgentCatalog {
        self.catalog
    }

    pub fn paths(&self) -> &AgentSearchPaths {
        &self.paths
    }

    /// Walk built-ins and configured search paths, build a fresh snapshot.
    pub fn load(&mut self) -> &AgentLoadReport {
        let mut all: Vec<LoadedAgentDefinition> = Vec::new();
        let mut failed: Vec<ValidationDiagnostic> = Vec::new();
        let mut warnings: Vec<ValidationDiagnostic> = Vec::new();

        for def in builtin_definitions(self.catalog) {
            all.push(LoadedAgentDefinition {
                definition: def,
                path: None,
            });
        }

        let plan: [(&[PathBuf], AgentSource); 5] = [
            (&self.paths.plugin_dirs, AgentSource::Plugin),
            (self.paths.user_dir.as_slice(), AgentSource::UserSettings),
            (&self.paths.project_dirs, AgentSource::ProjectSettings),
            (&self.paths.flag_dirs, AgentSource::FlagSettings),
            (&self.paths.policy_dirs, AgentSource::PolicySettings),
        ];
        for (dirs, source) in plan {
            for dir in dirs {
                collect_dir(dir, source, &mut all, &mut failed, &mut warnings);
            }
        }

        let active = compute_active(&all);
        self.snapshot = Arc::new(AgentCatalogSnapshot::new(active, all));
        self.last_report = AgentLoadReport { failed, warnings };
        &self.last_report
    }

    /// Manual refresh from disk. The watcher in `app/state` invokes this
    /// when an agent file changes.
    pub fn reload(&mut self) -> &AgentLoadReport {
        self.load()
    }

    /// Add a definition from an in-process source (SDK, CLI flag JSON).
    /// The store re-applies precedence after the insert.
    pub fn insert_definition(&mut self, def: AgentDefinition) {
        let mut all = self.snapshot.all().to_vec();
        all.push(LoadedAgentDefinition {
            definition: def,
            path: None,
        });
        let active = compute_active(&all);
        self.snapshot = Arc::new(AgentCatalogSnapshot::new(active, all));
    }
}

fn collect_dir(
    dir: &Path,
    source: AgentSource,
    all: &mut Vec<LoadedAgentDefinition>,
    failed: &mut Vec<ValidationDiagnostic>,
    warnings: &mut Vec<ValidationDiagnostic>,
) {
    let paths = match sorted_md_paths(dir) {
        Ok(paths) => paths,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                tracing::debug!(target: "coco_subagent", path=%dir.display(), error=%err, "read_dir failed");
            }
            return;
        }
    };

    for path in paths {
        match load_one(&path, source) {
            Ok((def, def_warnings)) => {
                // Always surface frontmatter warnings, even when the
                // semantic validator rejects the definition.
                for w in &def_warnings {
                    warnings.push(ValidationDiagnostic::new(
                        path.clone(),
                        Some(def.name.clone()),
                        w.clone(),
                    ));
                }
                let semantic_errors = AgentDefinitionValidator::check(&def);
                if semantic_errors.is_empty() {
                    all.push(LoadedAgentDefinition {
                        definition: def,
                        path: Some(path.clone()),
                    });
                } else {
                    for err in semantic_errors {
                        failed.push(ValidationDiagnostic::new(
                            path.clone(),
                            Some(def.name.clone()),
                            err,
                        ));
                    }
                }
            }
            Err(diag) => failed.push(diag),
        }
    }
}

fn load_one(
    path: &Path,
    source: AgentSource,
) -> Result<(AgentDefinition, Vec<ValidationError>), ValidationDiagnostic> {
    let raw = std::fs::read_to_string(path).map_err(|err| {
        ValidationDiagnostic::new(
            path.to_path_buf(),
            None,
            ValidationError::Io {
                message: err.to_string(),
            },
        )
    })?;
    let parsed = parse(&raw);
    parse_agent_markdown(path, &parsed.content, &parsed.data, source)
        .map_err(|err| ValidationDiagnostic::new(path.to_path_buf(), None, err.into()))
}

/// Apply TS-parity precedence: later (higher-or-equal-priority) source
/// wins. Equal-priority later wins matches TS `Map.set` semantics in
/// `getActiveAgentsFromList`. Caller controls iteration order via the
/// `paths` field on `AgentSearchPaths`; intra-directory order is sorted
/// in `sorted_md_paths` for cross-OS determinism.
fn compute_active(all: &[LoadedAgentDefinition]) -> BTreeMap<String, AgentDefinition> {
    let mut active: BTreeMap<String, AgentDefinition> = BTreeMap::new();
    for loaded in all {
        let priority = loaded.definition.source.priority();
        match active.get(&loaded.definition.name) {
            Some(existing) if existing.source.priority() > priority => {}
            _ => {
                active.insert(loaded.definition.name.clone(), loaded.definition.clone());
            }
        }
    }
    active
}
