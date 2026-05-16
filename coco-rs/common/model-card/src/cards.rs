//! Static model-card catalog. Add entries here when vendors publish
//! new models; entries are immutable `pub const`.
//!
//! **Citations.** Every cutoff date / pricing entry should track a
//! vendor-published source. When the source is the prior TS port
//! (`constants/prompts.ts::getKnowledgeCutoff`), record that — it
//! shipped to users, so byte-parity is a contract.

use crate::DeprecationInfo;
use crate::KnowledgeCutoff;
use crate::ModelCard;
use crate::ModelFamily;
use crate::Pricing;

// ───────── Claude family ─────────

const CLAUDE_OPUS_4_7: ModelCard = ModelCard {
    canonical_id: "claude-opus-4-7",
    // The `[1m]` and `-1m` suffixes denote the 1M-context preview SKU;
    // same training cutoff as the base model.
    aliases: &["claude-opus-4-7-1m", "claude-opus-4-7[1m]"],
    family: ModelFamily::Claude,
    knowledge_cutoff: Some(KnowledgeCutoff {
        display: "January 2026",
        year: 2026,
        month: 1,
    }),
    pricing: Some(Pricing {
        input_per_million_usd: 15.0,
        output_per_million_usd: 75.0,
        cache_read_per_million_usd: Some(1.5),
        cache_write_per_million_usd: Some(18.75),
    }),
    vendor_context_window: Some(200_000),
    release_date: None,
    deprecation: None,
};

const CLAUDE_OPUS_4_6: ModelCard = ModelCard {
    canonical_id: "claude-opus-4-6",
    aliases: &[],
    family: ModelFamily::Claude,
    knowledge_cutoff: Some(KnowledgeCutoff {
        display: "May 2025",
        year: 2025,
        month: 5,
    }),
    pricing: Some(Pricing {
        input_per_million_usd: 15.0,
        output_per_million_usd: 75.0,
        cache_read_per_million_usd: Some(1.5),
        cache_write_per_million_usd: Some(18.75),
    }),
    vendor_context_window: Some(200_000),
    release_date: None,
    deprecation: None,
};

const CLAUDE_OPUS_4_5: ModelCard = ModelCard {
    canonical_id: "claude-opus-4-5",
    aliases: &[],
    family: ModelFamily::Claude,
    knowledge_cutoff: Some(KnowledgeCutoff {
        display: "May 2025",
        year: 2025,
        month: 5,
    }),
    pricing: Some(Pricing {
        input_per_million_usd: 15.0,
        output_per_million_usd: 75.0,
        cache_read_per_million_usd: Some(1.5),
        cache_write_per_million_usd: Some(18.75),
    }),
    vendor_context_window: Some(200_000),
    release_date: None,
    deprecation: None,
};

const CLAUDE_SONNET_4_6: ModelCard = ModelCard {
    canonical_id: "claude-sonnet-4-6",
    aliases: &["claude-sonnet-4-6-1m", "claude-sonnet-4-6[1m]"],
    family: ModelFamily::Claude,
    knowledge_cutoff: Some(KnowledgeCutoff {
        display: "August 2025",
        year: 2025,
        month: 8,
    }),
    pricing: Some(Pricing {
        input_per_million_usd: 3.0,
        output_per_million_usd: 15.0,
        cache_read_per_million_usd: Some(0.3),
        cache_write_per_million_usd: Some(3.75),
    }),
    vendor_context_window: Some(200_000),
    release_date: None,
    deprecation: None,
};

const CLAUDE_SONNET_4_5: ModelCard = ModelCard {
    canonical_id: "claude-sonnet-4-5",
    aliases: &[],
    family: ModelFamily::Claude,
    knowledge_cutoff: Some(KnowledgeCutoff {
        display: "January 2025",
        year: 2025,
        month: 1,
    }),
    pricing: Some(Pricing {
        input_per_million_usd: 3.0,
        output_per_million_usd: 15.0,
        cache_read_per_million_usd: Some(0.3),
        cache_write_per_million_usd: Some(3.75),
    }),
    vendor_context_window: Some(200_000),
    release_date: None,
    deprecation: None,
};

const CLAUDE_HAIKU_4_5: ModelCard = ModelCard {
    canonical_id: "claude-haiku-4-5",
    aliases: &[],
    family: ModelFamily::Claude,
    knowledge_cutoff: Some(KnowledgeCutoff {
        display: "February 2025",
        year: 2025,
        month: 2,
    }),
    pricing: Some(Pricing {
        input_per_million_usd: 1.0,
        output_per_million_usd: 5.0,
        cache_read_per_million_usd: Some(0.1),
        cache_write_per_million_usd: Some(1.25),
    }),
    vendor_context_window: Some(200_000),
    release_date: None,
    deprecation: None,
};

// ───────── GPT family (placeholder — fill from vendor docs) ─────────

const GPT_5_4: ModelCard = ModelCard {
    canonical_id: "gpt-5-4",
    aliases: &[],
    family: ModelFamily::Gpt,
    knowledge_cutoff: None,
    pricing: None,
    vendor_context_window: None,
    release_date: None,
    deprecation: None,
};

// ───────── Catalog ─────────

/// All known model cards. Order here is irrelevant — lookup is by id.
pub const ALL: &[&ModelCard] = &[
    &CLAUDE_OPUS_4_7,
    &CLAUDE_OPUS_4_6,
    &CLAUDE_OPUS_4_5,
    &CLAUDE_SONNET_4_6,
    &CLAUDE_SONNET_4_5,
    &CLAUDE_HAIKU_4_5,
    &GPT_5_4,
];

// Silence dead_code for the deprecation field on entries that haven't
// declared one yet. The field is part of the public schema and will be
// populated when a vendor announces sunset; the lint would otherwise
// fire on every consumer build.
#[allow(dead_code)]
const _DEPRECATION_FIELD_REFERENCED: Option<DeprecationInfo> = None;
