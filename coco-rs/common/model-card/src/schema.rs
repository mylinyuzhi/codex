/// Vendor-published facts about an LLM.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCard {
    /// Canonical model id. OpenRouter models use their `id` value here.
    pub canonical_id: String,
    /// Alternate ids that resolve to this card.
    pub aliases: Vec<String>,
    /// Coarse family useful for UI grouping. Not used for lookup.
    pub family: ModelFamily,
    /// Vendor/OpenRouter stated training-data cutoff.
    pub knowledge_cutoff: Option<KnowledgeCutoff>,
    /// USD list-price tokens-per-million.
    pub pricing: Option<Pricing>,
    /// Vendor-stated max context window. User config may choose smaller.
    pub vendor_context_window: Option<i64>,
    /// Human display name from the source catalog.
    pub display_name: Option<String>,
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

/// Knowledge cutoff with original source date, display string, and sortable
/// `(year, month)` tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeCutoff {
    /// Source date, usually `YYYY-MM-DD`.
    pub raw: String,
    /// Human-readable string rendered into the env block.
    pub display: String,
    pub year: i16,
    pub month: u8,
}

impl KnowledgeCutoff {
    pub(crate) fn from_raw(raw: &str) -> Option<Self> {
        let mut parts = raw.split('-');
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        if !(1..=12).contains(&month) {
            return None;
        }
        Some(Self {
            raw: raw.to_string(),
            display: format!("{} {year}", month_name(month)),
            year,
            month,
        })
    }
}

/// USD per million tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct Pricing {
    pub input_per_million_usd: f64,
    pub output_per_million_usd: f64,
    pub cache_read_per_million_usd: Option<f64>,
    pub cache_write_per_million_usd: Option<f64>,
    pub source: PricingSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PricingSource {
    OfficialProvider {
        provider: String,
        source_url: String,
    },
    OpenRouterFallback,
}

/// Error returned when a model-card catalog cannot be parsed.
#[derive(Debug)]
pub enum ModelCardError {
    Json(serde_json::Error),
    EmptyCatalog,
}

impl std::fmt::Display for ModelCardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(err) => write!(f, "invalid model-card JSON: {err}"),
            Self::EmptyCatalog => f.write_str("model-card catalog is empty"),
        }
    }
}

impl std::error::Error for ModelCardError {}

impl From<serde_json::Error> for ModelCardError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct OpenRouterModelsResponse {
    pub(crate) data: Vec<OpenRouterModel>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct OpenRouterModel {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) canonical_slug: Option<String>,
    #[serde(default)]
    pub(crate) hugging_face_id: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) context_length: Option<i64>,
    #[serde(default)]
    pub(crate) pricing: Option<OpenRouterPricing>,
    #[serde(default)]
    pub(crate) top_provider: Option<OpenRouterTopProvider>,
    #[serde(default)]
    pub(crate) knowledge_cutoff: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct OpenRouterTopProvider {
    #[serde(default)]
    pub(crate) context_length: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct OpenRouterPricing {
    #[serde(default)]
    pub(crate) prompt: Option<String>,
    #[serde(default)]
    pub(crate) completion: Option<String>,
    #[serde(default)]
    pub(crate) input_cache_read: Option<String>,
    #[serde(default)]
    pub(crate) input_cache_write: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct OfficialPricingFile {
    pub(crate) provider: String,
    pub(crate) source_url: String,
    pub(crate) models: Vec<OfficialPricingRecord>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct OfficialPricingRecord {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) aliases: Vec<String>,
    pub(crate) input_per_million_usd: f64,
    pub(crate) output_per_million_usd: f64,
    #[serde(default)]
    pub(crate) cache_read_per_million_usd: Option<f64>,
    #[serde(default)]
    pub(crate) cache_write_per_million_usd: Option<f64>,
}

fn month_name(month: u8) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        12 => "December",
        _ => unreachable!("month checked by caller"),
    }
}
