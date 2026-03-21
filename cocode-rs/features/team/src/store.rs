//! Team store with dual-layer persistence (in-memory cache + filesystem).
//!
//! Teams are stored at `{base_dir}/{team_name}/config.json`. The in-memory
//! `BTreeMap` acts as a cache; all mutations are written through to disk
//! when `persist` is enabled. Filesystem writes use atomic rename
//! (write-to-temp + rename) for crash safety.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use snafu::ResultExt;
use tokio::sync::Mutex;

use crate::error::Result;
use crate::error::team_error;
use crate::types::MemberStatus;
use crate::types::Team;
use crate::types::TeamMember;

/// Thread-safe team store with optional filesystem persistence.
#[derive(Debug, Clone)]
pub struct TeamStore {
    /// In-memory cache.
    teams: Arc<Mutex<BTreeMap<String, Team>>>,
    /// Base directory for persistence (e.g., `~/.cocode/teams/`).
    base_dir: PathBuf,
    /// Whether to persist to disk.
    persist: bool,
}

impl TeamStore {
    /// Create a new team store.
    ///
    /// If `persist` is `true`, teams are written to `{base_dir}/{name}/config.json`.
    pub fn new(base_dir: PathBuf, persist: bool) -> Self {
        Self {
            teams: Arc::new(Mutex::new(BTreeMap::new())),
            base_dir,
            persist,
        }
    }

    /// Load existing teams from disk.
    ///
    /// Scans `{base_dir}/*/config.json` and populates the in-memory cache.
    /// Silently skips directories without valid config files.
    pub async fn load_from_disk(&self) -> Result<()> {
        if !self.persist || !self.base_dir.exists() {
            return Ok(());
        }

        let mut entries =
            tokio::fs::read_dir(&self.base_dir)
                .await
                .context(team_error::PersistSnafu {
                    message: format!("reading teams dir: {}", self.base_dir.display()),
                })?;

        let mut teams = self.teams.lock().await;
        while let Some(entry) = entries
            .next_entry()
            .await
            .context(team_error::PersistSnafu {
                message: "iterating teams dir",
            })?
        {
            let config_path = entry.path().join("config.json");
            match read_team_file(&config_path).await {
                Ok(team) => {
                    teams.insert(team.name.clone(), team);
                }
                Err(e) => {
                    tracing::warn!(
                        path = %config_path.display(),
                        error = %e,
                        "Skipping invalid team config"
                    );
                }
            }
        }
        Ok(())
    }

    /// Create a new team.
    pub async fn create_team(&self, team: Team) -> Result<()> {
        let name = team.name.clone();
        {
            let mut teams = self.teams.lock().await;
            if teams.contains_key(&name) {
                return Err(team_error::TeamExistsSnafu { name }.build());
            }
            teams.insert(name.clone(), team);
        }
        self.persist_team(&name).await?;
        Ok(())
    }

    /// Delete a team.
    pub async fn delete_team(&self, name: &str) -> Result<()> {
        {
            let mut teams = self.teams.lock().await;
            if teams.remove(name).is_none() {
                return Err(team_error::TeamNotFoundSnafu { name }.build());
            }
        }
        if self.persist {
            let dir = self.base_dir.join(name);
            let _ = tokio::fs::remove_dir_all(&dir).await;
        }
        Ok(())
    }

    /// Get a team by name.
    pub async fn get_team(&self, name: &str) -> Option<Team> {
        let teams = self.teams.lock().await;
        teams.get(name).cloned()
    }

    /// List all teams.
    pub async fn list_teams(&self) -> BTreeMap<String, Team> {
        let teams = self.teams.lock().await;
        teams.clone()
    }

    /// Add a member to a team.
    pub async fn add_member(
        &self,
        team_name: &str,
        member: TeamMember,
        max_members: usize,
    ) -> Result<()> {
        {
            let mut teams = self.teams.lock().await;
            let team = teams
                .get_mut(team_name)
                .ok_or_else(|| team_error::TeamNotFoundSnafu { name: team_name }.build())?;

            if team.members.len() >= max_members {
                return Err(team_error::MaxMembersReachedSnafu {
                    team_name,
                    limit: max_members,
                }
                .build());
            }

            team.members.push(member);
        }
        self.persist_team(team_name).await?;
        Ok(())
    }

    /// Remove a member from a team.
    pub async fn remove_member(&self, team_name: &str, agent_id: &str) -> Result<()> {
        {
            let mut teams = self.teams.lock().await;
            let team = teams
                .get_mut(team_name)
                .ok_or_else(|| team_error::TeamNotFoundSnafu { name: team_name }.build())?;

            let before = team.members.len();
            team.members.retain(|m| m.agent_id != agent_id);
            if team.members.len() == before {
                return Err(team_error::NotAMemberSnafu {
                    agent_id,
                    team_name,
                }
                .build());
            }
        }
        self.persist_team(team_name).await?;
        Ok(())
    }

    /// Update a member's status.
    pub async fn update_member_status(
        &self,
        team_name: &str,
        agent_id: &str,
        status: MemberStatus,
    ) -> Result<()> {
        {
            let mut teams = self.teams.lock().await;
            let team = teams
                .get_mut(team_name)
                .ok_or_else(|| team_error::TeamNotFoundSnafu { name: team_name }.build())?;

            let member = team
                .members
                .iter_mut()
                .find(|m| m.agent_id == agent_id)
                .ok_or_else(|| {
                    team_error::NotAMemberSnafu {
                        agent_id,
                        team_name,
                    }
                    .build()
                })?;

            member.status = status;
        }
        self.persist_team(team_name).await?;
        Ok(())
    }

    /// Get a JSON snapshot of all teams (for `ContextModifier::TeamsUpdated`).
    pub async fn snapshot(&self) -> serde_json::Value {
        let teams = self.teams.lock().await;
        serde_json::to_value(&*teams).unwrap_or_else(|e| {
            tracing::error!("TeamStore serialization failed: {e}");
            serde_json::Value::Object(Default::default())
        })
    }

    /// Persist a single team to disk (atomic write).
    async fn persist_team(&self, name: &str) -> Result<()> {
        if !self.persist {
            return Ok(());
        }

        // Clone the team data and release the lock before doing I/O,
        // so other operations aren't blocked by filesystem latency.
        let team = {
            let teams = self.teams.lock().await;
            match teams.get(name) {
                Some(t) => t.clone(),
                None => return Ok(()),
            }
        };

        let dir = self.base_dir.join(name);
        tokio::fs::create_dir_all(&dir)
            .await
            .context(team_error::PersistSnafu {
                message: format!("creating team dir: {}", dir.display()),
            })?;

        let config_path = dir.join("config.json");
        atomic_write_json(&config_path, &team).await?;
        Ok(())
    }
}

/// Read a team config from a JSON file.
async fn read_team_file(path: &Path) -> Result<Team> {
    let content = tokio::fs::read_to_string(path)
        .await
        .context(team_error::PersistSnafu {
            message: format!("reading: {}", path.display()),
        })?;
    serde_json::from_str(&content).context(team_error::SerdeSnafu {
        message: format!("parsing: {}", path.display()),
    })
}

/// Write JSON to a file atomically (write to temp, then rename).
async fn atomic_write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value).context(team_error::SerdeSnafu {
        message: "serializing team",
    })?;

    let tmp_path = path.with_extension("json.tmp");
    tokio::fs::write(&tmp_path, json.as_bytes())
        .await
        .context(team_error::PersistSnafu {
            message: format!("writing temp: {}", tmp_path.display()),
        })?;

    tokio::fs::rename(&tmp_path, path)
        .await
        .context(team_error::PersistSnafu {
            message: format!("renaming to: {}", path.display()),
        })?;

    Ok(())
}

#[cfg(test)]
#[path = "store.test.rs"]
mod tests;
