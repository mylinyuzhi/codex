//! Configuration migration system.
//!
//! TS: migrations/ (603 LOC) — migrates old config formats to new.

/// Migration version tracking.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct MigrationState {
    pub current_version: i32,
    pub last_migrated_at: Option<String>,
}

/// The latest migration version.
pub const LATEST_VERSION: i32 = 3;

/// A migration step.
pub struct Migration {
    pub version: i32,
    pub description: &'static str,
    pub migrate: fn(&mut serde_json::Value) -> anyhow::Result<()>,
}

/// All known migrations.
pub fn get_migrations() -> Vec<Migration> {
    vec![
        Migration {
            version: 1,
            description: "Rename 'apiKey' to 'api_key_helper'",
            migrate: |config| {
                if let Some(val) = config.get("apiKey").cloned() {
                    config.as_object_mut().unwrap().remove("apiKey");
                    config["api_key_helper"] = val;
                }
                Ok(())
            },
        },
        Migration {
            version: 2,
            description: "Move permissions to nested object",
            migrate: |config| {
                if config.get("permissions").is_none() {
                    config["permissions"] = serde_json::json!({
                        "allow": [],
                        "deny": []
                    });
                }
                Ok(())
            },
        },
        Migration {
            version: 3,
            description: "Add default auto_mode config",
            migrate: |config| {
                if config.get("auto_mode").is_none() {
                    config["auto_mode"] = serde_json::json!({
                        "allow": [],
                        "soft_deny": [],
                        "environment": []
                    });
                }
                Ok(())
            },
        },
    ]
}

/// Run all pending migrations on a config value.
pub fn run_migrations(
    config: &mut serde_json::Value,
    state: &mut MigrationState,
) -> anyhow::Result<i32> {
    let mut applied = 0;
    for migration in get_migrations() {
        if migration.version > state.current_version {
            (migration.migrate)(config)?;
            state.current_version = migration.version;
            applied += 1;
        }
    }
    if applied > 0 {
        state.last_migrated_at = Some(chrono_stub());
    }
    Ok(applied)
}

fn chrono_stub() -> String {
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    )
}

#[cfg(test)]
#[path = "migrations.test.rs"]
mod tests;
