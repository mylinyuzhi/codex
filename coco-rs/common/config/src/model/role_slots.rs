//! Per-role fallback chain + recovery policy.
//!
//! Each `ModelRole` binds to a `RoleSlots<T>`: a primary plus an
//! ordered list of fallbacks the runtime walks on capacity errors.
//!
//! # Shapes
//!
//! Both `T = ModelSelection` (JSON config side) and `T = ModelSpec`
//! (runtime-resolved side) reuse this one generic, avoiding a parallel
//! type pair. Only the `ModelSelection` instantiation has a custom
//! deserializer — the runtime side is only ever built programmatically
//! by the runtime-config resolver.
//!
//! # JSON shapes accepted for `RoleSlots<ModelSelection>`
//!
//! 1. Bare string: `"anthropic/claude-opus-4-6"` — splits on `/` into
//!    `(provider, model_id)`.
//! 2. Legacy flat: `{ "provider": "x", "model_id": "y" }` —
//!    same as existing `ModelSelection`.
//! 3. Single fallback: `{ "primary": …, "fallback": …, "recovery": …? }`.
//! 4. Plural fallbacks: `{ "primary": …, "fallbacks": [ … ], "recovery": …? }`.
//!
//! Shapes (1) and (2) produce `RoleSlots { primary, fallbacks: vec![], recovery: None }`.
//! Shapes (3) and (4) cannot be combined in the same entry — specifying
//! both `fallback` and `fallbacks` is a hard deserialization error. The
//! nested form uses `deny_unknown_fields` so typos in field names
//! surface immediately with actionable messages instead of silently
//! falling through to another variant.

use std::time::Duration;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::de::Error;

use super::ModelSelection;

/// Per-role primary + ordered fallback chain + optional recovery policy.
///
/// Generic over `T` so the config-facing (`ModelSelection`) and
/// runtime-facing (`ModelSpec`) instantiations share code. Keeping a
/// single type avoids drift between the two sides and mirrors the
/// existing `ModelResult<T>`-style generics in the codebase.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoleSlots<T> {
    pub primary: T,
    /// Ordered fallbacks. Empty = no fallback configured.
    pub fallbacks: Vec<T>,
    /// Recovery policy; `None` = sticky (no auto-return to primary).
    pub recovery: Option<FallbackRecoveryPolicy>,
}

impl<T> RoleSlots<T> {
    pub fn new(primary: T) -> Self {
        Self {
            primary,
            fallbacks: Vec::new(),
            recovery: None,
        }
    }

    pub fn with_fallback(mut self, fallback: T) -> Self {
        self.fallbacks.push(fallback);
        self
    }

    pub fn with_fallbacks(mut self, fallbacks: Vec<T>) -> Self {
        self.fallbacks = fallbacks;
        self
    }

    pub fn with_recovery(mut self, policy: FallbackRecoveryPolicy) -> Self {
        self.recovery = Some(policy);
        self
    }

    /// Map both primary and fallbacks with a single closure.
    ///
    /// Used by the runtime-config resolver to lift
    /// `RoleSlots<ModelSelection>` (config-side) into
    /// `RoleSlots<ModelSpec>` (runtime-side) by resolving each
    /// selection against the provider catalog.
    pub fn try_map<U, E, F>(self, mut f: F) -> Result<RoleSlots<U>, E>
    where
        F: FnMut(T) -> Result<U, E>,
    {
        let primary = f(self.primary)?;
        let fallbacks = self
            .fallbacks
            .into_iter()
            .map(&mut f)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RoleSlots {
            primary,
            fallbacks,
            recovery: self.recovery,
        })
    }
}

/// Half-open recovery policy: after switching to a fallback, periodically
/// probe the primary. Backoff doubles on each probe failure up to
/// `max_backoff`; `max_attempts` caps total probes per session.
///
/// Wire format uses seconds (humans edit these in settings.json); the
/// runtime converts to `Duration` via the getter methods.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FallbackRecoveryPolicy {
    /// Seconds before the first probe. Also the initial backoff.
    pub initial_backoff_secs: u64,
    /// Upper bound on backoff in seconds.
    pub max_backoff_secs: u64,
    /// Maximum probe attempts per session.
    pub max_attempts: u32,
}

impl Default for FallbackRecoveryPolicy {
    fn default() -> Self {
        // 60 s initial → 30 min cap, 10 attempts.
        Self {
            initial_backoff_secs: 60,
            max_backoff_secs: 1_800,
            max_attempts: 10,
        }
    }
}

impl FallbackRecoveryPolicy {
    pub fn initial_backoff(&self) -> Duration {
        Duration::from_secs(self.initial_backoff_secs)
    }

    pub fn max_backoff(&self) -> Duration {
        Duration::from_secs(self.max_backoff_secs.max(self.initial_backoff_secs))
    }
}

// ─── Deserializer for RoleSlots<ModelSelection> ─────────────────────────────

impl<'de> Deserialize<'de> for RoleSlots<ModelSelection> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        // Dispatch explicitly on the observed JSON shape instead of
        // relying on serde's untagged fallthrough. `ModelSelection`
        // has `#[serde(default)]`, which makes every object a
        // "legal" legacy shape — including objects with typo'd
        // keys — so an untagged-union approach would silently
        // accept typos as empty legacy selections and surface a
        // misleading "non-empty" error. Routing on presence of
        // `primary`/`fallback`/`fallbacks`/`recovery` keys is
        // deterministic and yields actionable error messages.
        let value = serde_json::Value::deserialize(d)?;

        if let Some(s) = value.as_str() {
            return parse_bare_string(s).map_err(D::Error::custom);
        }

        let obj = value.as_object().ok_or_else(|| {
            D::Error::custom("role selection must be a string, flat object, or nested object")
        })?;

        let has_nested_keys = obj.contains_key("primary")
            || obj.contains_key("fallback")
            || obj.contains_key("fallbacks")
            || obj.contains_key("recovery");

        if has_nested_keys {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Nested {
                primary: ModelSelection,
                #[serde(default)]
                fallback: Option<ModelSelection>,
                #[serde(default)]
                fallbacks: Option<Vec<ModelSelection>>,
                #[serde(default)]
                recovery: Option<FallbackRecoveryPolicy>,
            }
            let n: Nested = serde_json::from_value(value).map_err(D::Error::custom)?;
            let fallbacks = match (n.fallback, n.fallbacks) {
                (Some(_), Some(_)) => {
                    return Err(D::Error::custom(
                        "use either `fallback` (single) or `fallbacks` (list), not both",
                    ));
                }
                (Some(one), None) => vec![one],
                (None, Some(list)) => list,
                (None, None) => Vec::new(),
            };
            Ok(RoleSlots {
                primary: n.primary,
                fallbacks,
                recovery: n.recovery,
            })
        } else {
            // Legacy flat form: {"provider": ..., "model_id": ...}.
            // Reject any unknown keys so typos like "modle_id" don't
            // silently become empty selections.
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Flat {
                #[serde(default)]
                provider: String,
                #[serde(default)]
                model_id: String,
            }
            let f: Flat = serde_json::from_value(value).map_err(D::Error::custom)?;
            if f.provider.is_empty() || f.model_id.is_empty() {
                return Err(D::Error::custom(
                    "role selection must include non-empty `provider` and `model_id`",
                ));
            }
            Ok(RoleSlots::new(ModelSelection {
                provider: f.provider,
                model_id: f.model_id,
            }))
        }
    }
}

/// Emit the compact nested form on serialize. Round-tripping a
/// bare-string-form config through serde produces the nested form —
/// acceptable because the nested form is always valid input.
impl Serialize for RoleSlots<ModelSelection> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("RoleSlots", 3)?;
        st.serialize_field("primary", &self.primary)?;
        if !self.fallbacks.is_empty() {
            st.serialize_field("fallbacks", &self.fallbacks)?;
        } else {
            st.skip_field("fallbacks")?;
        }
        if let Some(r) = self.recovery {
            st.serialize_field("recovery", &r)?;
        } else {
            st.skip_field("recovery")?;
        }
        st.end()
    }
}

fn parse_bare_string(s: &str) -> Result<RoleSlots<ModelSelection>, String> {
    let (provider, model_id) = s.split_once('/').ok_or_else(|| {
        format!("model selection `{s}` must use explicit `provider/model_id` format")
    })?;
    if provider.is_empty() || model_id.is_empty() {
        return Err(format!(
            "model selection `{s}` must use explicit `provider/model_id` format"
        ));
    }
    Ok(RoleSlots::new(ModelSelection {
        provider: provider.to_string(),
        model_id: model_id.to_string(),
    }))
}

#[cfg(test)]
#[path = "role_slots.test.rs"]
mod tests;
