//! Load typed permission rules from settings sources.
//!
//! Bridges [`coco_config::SettingsWithSource::sourced_permission_rules`]
//! (which returns string rules grouped by [`SettingSource`]) and
//! [`coco_query::QueryEngineConfig::{allow,deny,ask}_rules`] (which
//! expects [`coco_types::PermissionRulesBySource`] indexed by
//! [`coco_types::PermissionRuleSource`]).
//!
//! TS parity: `loadPermissionRules()` in
//! `utils/permissions/permissionsLoader.ts` does the same string →
//! typed conversion before threading the rules into the evaluator.
//!
//! Plugin-sourced rules are dropped: `coco-types` does not model a
//! `Plugin` rule source, and TS treats plugin permissions as project
//! contributions that are merged at a higher layer.

use coco_config::SettingSource;
use coco_config::SettingsWithSource;
use coco_config::SourcedRule;
use coco_permissions::parse_rule_string;
use coco_types::PermissionBehavior;
use coco_types::PermissionRule;
use coco_types::PermissionRuleSource;
use coco_types::PermissionRulesBySource;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// Map a config-layer [`SettingSource`] to the matching
/// permission-layer [`PermissionRuleSource`].
///
/// `Plugin` has no permission-layer counterpart and is dropped.
fn setting_source_to_permission_source(s: SettingSource) -> Option<PermissionRuleSource> {
    match s {
        SettingSource::User => Some(PermissionRuleSource::UserSettings),
        SettingSource::Project => Some(PermissionRuleSource::ProjectSettings),
        SettingSource::Local => Some(PermissionRuleSource::LocalSettings),
        SettingSource::Flag => Some(PermissionRuleSource::FlagSettings),
        SettingSource::Policy => Some(PermissionRuleSource::PolicySettings),
        SettingSource::Plugin => None,
    }
}

/// Convert a flat sourced-rule list into a [`PermissionRulesBySource`]
/// map, parsing each rule string via
/// [`coco_permissions::parse_rule_string`].
fn build_rules_by_source(
    rules: &[SourcedRule],
    behavior: PermissionBehavior,
) -> PermissionRulesBySource {
    let mut out: PermissionRulesBySource = Default::default();
    for sourced in rules {
        let Some(source) = setting_source_to_permission_source(sourced.source) else {
            continue;
        };
        let value = parse_rule_string(&sourced.rule);
        out.entry(source).or_default().push(PermissionRule {
            source,
            behavior,
            value,
        });
    }
    out
}

/// Build the (allow, deny, ask) rule maps for
/// [`coco_query::QueryEngineConfig`] from the sourced settings. The
/// caller spreads these onto the engine config alongside the
/// existing `..Default::default()` rule fields.
pub fn typed_permission_rules(
    settings: &SettingsWithSource,
) -> (
    PermissionRulesBySource,
    PermissionRulesBySource,
    PermissionRulesBySource,
) {
    let (allow, deny, ask) = settings.sourced_permission_rules();
    (
        build_rules_by_source(&allow, PermissionBehavior::Allow),
        build_rules_by_source(&deny, PermissionBehavior::Deny),
        build_rules_by_source(&ask, PermissionBehavior::Ask),
    )
}

/// Resolve the source root used for leading-`/` file permission rules.
///
/// TS evidence:
/// - `settings.ts::getSettingsRootPathForSource`
/// - `permissions/filesystem.ts::rootPathForSource`
///
/// `Read(/foo/**)` in user settings is rooted at the coco config home;
/// the same rule in project/local/policy/session/command/CLI sources is
/// rooted at the original cwd. Flag settings are rooted at the directory
/// containing the flag settings file.
pub fn permission_rule_source_roots(
    settings: &SettingsWithSource,
    original_cwd: &Path,
) -> HashMap<PermissionRuleSource, PathBuf> {
    let mut roots = HashMap::new();
    let original_cwd = original_cwd.to_path_buf();

    for source in [
        PermissionRuleSource::Session,
        PermissionRuleSource::Command,
        PermissionRuleSource::CliArg,
        PermissionRuleSource::ProjectSettings,
        PermissionRuleSource::LocalSettings,
        PermissionRuleSource::PolicySettings,
    ] {
        roots.insert(source, original_cwd.clone());
    }

    let user_root = settings
        .source_paths
        .get(&SettingSource::User)
        .and_then(|path| path.parent())
        .map(Path::to_path_buf)
        .or_else(|| Some(coco_config::global_config::config_home()))
        .unwrap_or_else(|| original_cwd.clone());
    roots.insert(PermissionRuleSource::UserSettings, user_root);

    let flag_root = settings
        .source_paths
        .get(&SettingSource::Flag)
        .and_then(|path| path.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| original_cwd.clone());
    roots.insert(PermissionRuleSource::FlagSettings, flag_root);

    roots
}

#[cfg(test)]
#[path = "permission_rule_loader.test.rs"]
mod tests;
