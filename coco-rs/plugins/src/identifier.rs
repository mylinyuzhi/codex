//! Plugin identifiers and parsing.
//!
//! TS source: `utils/plugins/pluginIdentifier.ts:123` + `utils/plugins/schemas.ts` (PluginId).
//!
//! Format: `name@marketplace`. The `@` separator distinguishes plugin name
//! from its marketplace; both segments can contain alphanumerics, dashes,
//! underscores. Bare names (no `@`) are valid in dependency declarations
//! and inherit the declarer's marketplace at qualification time.

use serde::Deserialize;
use serde::Serialize;
use std::fmt;

/// Synthetic marketplace sentinel for `--plugin-dir` plugins.
/// TS: `INLINE_MARKETPLACE = 'inline'`.
pub const INLINE_MARKETPLACE: &str = "inline";

/// Builtin marketplace sentinel for compiled-in plugins.
/// TS: `BUILTIN_MARKETPLACE_NAME = 'builtin'` from `plugins/builtinPlugins.ts:23`.
pub const BUILTIN_MARKETPLACE: &str = "builtin";

/// Strongly-typed plugin identifier.
///
/// Wire form: `"name@marketplace"`. Serializes via Display/FromStr.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PluginId {
    pub name: String,
    pub marketplace: Option<String>,
}

impl PluginId {
    pub fn new(name: impl Into<String>, marketplace: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            marketplace: Some(marketplace.into()),
        }
    }

    pub fn bare(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            marketplace: None,
        }
    }

    /// Parse `name@marketplace` form. Bare names map to `marketplace: None`.
    /// TS: `parsePluginIdentifier(input)`.
    pub fn parse(input: &str) -> Self {
        if let Some((name, marketplace)) = input.rsplit_once('@') {
            Self {
                name: name.to_string(),
                marketplace: if marketplace.is_empty() {
                    None
                } else {
                    Some(marketplace.to_string())
                },
            }
        } else {
            Self::bare(input)
        }
    }

    /// Whether this id refers to a builtin plugin (`<name>@builtin`).
    pub fn is_builtin(&self) -> bool {
        matches!(&self.marketplace, Some(m) if m == BUILTIN_MARKETPLACE)
    }

    /// Whether this id is from a `--plugin-dir` source.
    pub fn is_inline(&self) -> bool {
        matches!(&self.marketplace, Some(m) if m == INLINE_MARKETPLACE)
    }
}

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.marketplace {
            Some(m) => write!(f, "{}@{}", self.name, m),
            None => write!(f, "{}", self.name),
        }
    }
}

impl Serialize for PluginId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PluginId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(Self::parse(&raw))
    }
}

/// Plugin-scope ordering: priority high-to-low (Managed wins).
///
/// TS: `installed_plugins.json` V2 `scope` field. Higher scope wins on
/// name collisions; `Managed` cannot be disabled by users.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginScope {
    /// Local development via `--plugin-dir`.
    Local = 0,
    /// `<cwd>/.claude/plugins/`.
    Project = 1,
    /// `~/.coco/plugins/`.
    User = 2,
    /// Enterprise/policy-managed plugins. Cannot be disabled by users.
    Managed = 3,
}

impl PluginScope {
    pub fn as_str(self) -> &'static str {
        match self {
            PluginScope::Local => "local",
            PluginScope::Project => "project",
            PluginScope::User => "user",
            PluginScope::Managed => "managed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_qualified() {
        let id = PluginId::parse("foo@market");
        assert_eq!(id.name, "foo");
        assert_eq!(id.marketplace.as_deref(), Some("market"));
    }

    #[test]
    fn parse_bare() {
        let id = PluginId::parse("foo");
        assert_eq!(id.name, "foo");
        assert!(id.marketplace.is_none());
    }

    #[test]
    fn display_roundtrip() {
        assert_eq!(PluginId::parse("foo@bar").to_string(), "foo@bar");
        assert_eq!(PluginId::parse("foo").to_string(), "foo");
    }

    #[test]
    fn builtin_detection() {
        assert!(PluginId::parse("foo@builtin").is_builtin());
        assert!(!PluginId::parse("foo@market").is_builtin());
        assert!(!PluginId::parse("foo").is_builtin());
    }

    #[test]
    fn inline_detection() {
        assert!(PluginId::parse("foo@inline").is_inline());
        assert!(!PluginId::parse("foo@market").is_inline());
    }

    #[test]
    fn scope_priority_order() {
        assert!(PluginScope::Managed > PluginScope::User);
        assert!(PluginScope::User > PluginScope::Project);
        assert!(PluginScope::Project > PluginScope::Local);
    }
}
