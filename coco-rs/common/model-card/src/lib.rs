//! Vendor-defined model facts (knowledge cutoff, pricing, release date).
//!
//! Separate from [`coco_config::ModelInfo`] because **ownership and update
//! cadence differ**:
//! - `coco-config::ModelInfo` carries user-configurable knobs (context
//!   window override, sampling, base_instructions, tool_overrides). Users
//!   edit `~/.coco/models.json` to change them.
//! - `coco_model_card::ModelCard` carries vendor-published facts that no
//!   user override can change: training cutoff, list pricing, release
//!   date. We update this crate when a vendor releases a model; users
//!   never edit it.
//!
//! Folding both into one struct would force every test fixture to
//! fabricate plausible cutoff dates and pricing, and would confuse the
//! semantics of `ModelInfo::Default` (already a known sentinel; see
//! `common/config/src/model/mod.rs:82-115`).
//!
//! ## Lookup contract
//!
//! `lookup(model_id) -> Option<&'static ModelCard>`. No substring matching,
//! no case folding tricks. Match is exact against the canonical id and
//! the per-card `aliases` slice. Unknown ids return `None` — callers
//! omit the corresponding env-block line rather than rendering a wrong
//! date.
//!
//! ## Why not phf
//!
//! The catalog is small (~10 entries). A linear scan over a
//! `&'static [&'static ModelCard]` is faster than the perfect-hash setup
//! for N < 50. Avoiding `phf` keeps the dep graph minimal and lets the
//! whole table live in a `pub const` without a `build.rs`.

#![forbid(unsafe_code)]

pub mod cards;

/// Vendor-published facts about an LLM. Constructed at compile time via
/// `pub const` entries in [`cards`]; never built from user input.
#[derive(Debug, Clone, Copy)]
pub struct ModelCard {
    /// Canonical model id — the value that matches `ModelInfo::model_id`
    /// in production resolution.
    pub canonical_id: &'static str,
    /// Alternate ids that resolve to this card. Covers the variants
    /// observed in the wild — e.g. `claude-opus-4-7[1m]`,
    /// `claude-opus-4-7-1m`, abbreviated `opus-4.7`, etc.
    pub aliases: &'static [&'static str],
    /// Coarse family — useful for UI grouping. Not used for lookup.
    pub family: ModelFamily,
    /// Vendor-stated training-data cutoff. `None` for community / mock
    /// / not-yet-published models — the env block omits the line.
    pub knowledge_cutoff: Option<KnowledgeCutoff>,
    /// List-price tokens-per-million. `None` when pricing isn't published
    /// or doesn't apply (e.g. self-hosted).
    pub pricing: Option<Pricing>,
    /// Vendor-stated max context window. Users can configure smaller
    /// windows in `ModelInfo.context_window`; this is the upper bound.
    pub vendor_context_window: Option<i64>,
    /// Release date (YYYY-MM-DD). `None` for unreleased / preview.
    pub release_date: Option<&'static str>,
    /// Deprecation status — when vendor announced sunset and on which
    /// date. UI can warn users running deprecated models.
    pub deprecation: Option<DeprecationInfo>,
}

/// Coarse model family. Add variants as new vendors come online.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFamily {
    Claude,
    Gpt,
    Gemini,
    DeepSeek,
    Other,
}

/// Knowledge cutoff with both a human-readable display string and a
/// sortable `(year, month)` tuple.
#[derive(Debug, Clone, Copy)]
pub struct KnowledgeCutoff {
    /// Human-readable string rendered verbatim into the env block.
    /// E.g. `"January 2026"`. Mirrors TS `constants/prompts.ts`
    /// hardcoded `getKnowledgeCutoff` return values.
    pub display: &'static str,
    /// Sortable representation. Useful for "is this model older than X"
    /// queries without parsing the display string.
    pub year: i16,
    pub month: u8,
}

/// USD per million tokens. All fields nominal — provider may apply
/// discounts (batch, cache, volume) that this struct doesn't model.
#[derive(Debug, Clone, Copy)]
pub struct Pricing {
    pub input_per_million_usd: f32,
    pub output_per_million_usd: f32,
    pub cache_read_per_million_usd: Option<f32>,
    pub cache_write_per_million_usd: Option<f32>,
}

#[derive(Debug, Clone, Copy)]
pub struct DeprecationInfo {
    pub announced: &'static str,
    pub sunset: &'static str,
}

/// Exact-id lookup. Tries the canonical id first, then each card's
/// alias slice. Returns `None` for unknown ids.
///
/// **No substring matching, no case folding.** Inputs that don't appear
/// verbatim in [`cards::ALL`] miss cleanly. This is intentional — the
/// alternative ("`contains` returns the first prefix-match") silently
/// invents data for unknown future models.
pub fn lookup(model_id: &str) -> Option<&'static ModelCard> {
    for card in cards::ALL {
        if card.canonical_id == model_id {
            return Some(card);
        }
        for alias in card.aliases {
            if *alias == model_id {
                return Some(card);
            }
        }
    }
    None
}

/// Convenience for the env-block call site. Returns the display string
/// for the cutoff, or `None` if the model is unknown / has no published
/// cutoff. Callers should omit the env-block line on `None` rather than
/// render an empty value.
pub fn knowledge_cutoff(model_id: &str) -> Option<&'static str> {
    lookup(model_id)?
        .knowledge_cutoff
        .as_ref()
        .map(|c| c.display)
}

/// Convenience for cost-tracking call sites.
pub fn pricing(model_id: &str) -> Option<&'static Pricing> {
    lookup(model_id)?.pricing.as_ref()
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
