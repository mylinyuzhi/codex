use std::collections::BTreeSet;

use crate::ModelCardError;
use crate::resolver::lookup_keys;
use crate::schema::OfficialPricingFile;
use crate::schema::OpenRouterPricing;
use crate::schema::Pricing;
use crate::schema::PricingSource;

const OFFICIAL_ANTHROPIC_PRICING_JSON: &str =
    include_str!("../data/official-pricing/anthropic.json");

#[derive(Debug, Default)]
pub(crate) struct OfficialPricingIndex {
    records: Vec<OfficialPricingEntry>,
}

#[derive(Debug)]
struct OfficialPricingEntry {
    provider: String,
    keys: BTreeSet<String>,
    pricing: Pricing,
}

impl OfficialPricingIndex {
    pub(crate) fn lookup(
        &self,
        provider: Option<&str>,
        id: &str,
        canonical_slug: Option<&str>,
        aliases: &[String],
    ) -> Option<Pricing> {
        let mut keys = BTreeSet::new();
        if let Some(provider) = provider {
            keys.extend(lookup_keys(Some(provider), id));
        }
        keys.extend(lookup_keys(provider, id));
        if let Some(slug) = canonical_slug {
            keys.extend(lookup_keys(provider, slug));
        }
        for alias in aliases {
            keys.extend(lookup_keys(provider, alias));
        }

        self.records
            .iter()
            .find(|record| {
                provider.is_none_or(|p| p == record.provider)
                    && keys.iter().any(|key| record.keys.contains(key))
            })
            .map(|record| record.pricing.clone())
    }
}

pub(crate) fn load_official_pricing() -> Result<OfficialPricingIndex, ModelCardError> {
    let mut index = OfficialPricingIndex::default();
    for json in [OFFICIAL_ANTHROPIC_PRICING_JSON] {
        let file: OfficialPricingFile = serde_json::from_str(json)?;
        for record in file.models {
            let mut keys = BTreeSet::new();
            keys.extend(lookup_keys(Some(&file.provider), &record.id));
            for alias in &record.aliases {
                keys.extend(lookup_keys(Some(&file.provider), alias));
            }
            let source_url = file.source_url.clone();
            let provider = file.provider.clone();
            index.records.push(OfficialPricingEntry {
                provider: provider.clone(),
                keys,
                pricing: Pricing {
                    input_per_million_usd: record.input_per_million_usd,
                    output_per_million_usd: record.output_per_million_usd,
                    cache_read_per_million_usd: record.cache_read_per_million_usd,
                    cache_write_per_million_usd: record.cache_write_per_million_usd,
                    source: PricingSource::OfficialProvider {
                        provider,
                        source_url,
                    },
                },
            });
        }
    }
    Ok(index)
}

impl Pricing {
    pub(crate) fn from_openrouter(pricing: &OpenRouterPricing) -> Option<Self> {
        let input = parse_usd_per_token(pricing.prompt.as_deref())? * 1_000_000.0;
        let output = parse_usd_per_token(pricing.completion.as_deref())? * 1_000_000.0;
        Some(Self {
            input_per_million_usd: input,
            output_per_million_usd: output,
            cache_read_per_million_usd: parse_usd_per_token(pricing.input_cache_read.as_deref())
                .map(|v| v * 1_000_000.0),
            cache_write_per_million_usd: parse_usd_per_token(pricing.input_cache_write.as_deref())
                .map(|v| v * 1_000_000.0),
            source: PricingSource::OpenRouterFallback,
        })
    }
}

fn parse_usd_per_token(value: Option<&str>) -> Option<f64> {
    value?.parse::<f64>().ok()
}
