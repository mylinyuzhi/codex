use serde::Deserialize;
use serde::Serialize;

/// Input-side token breakdown — mirrors `vercel_ai::InputTokenDetails`
/// (which itself flattens `vercel_ai_provider::InputTokens`).
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputTokenDetails {
    /// Input tokens that did NOT come from the prompt cache.
    /// `input_tokens = no_cache_tokens + cache_read_tokens` when the
    /// provider reports both.
    #[serde(default)]
    pub no_cache_tokens: i64,
    /// Input tokens served from the prompt cache. Both OpenAI-compat
    /// (`prompt_tokens_details.cached_tokens`) and Anthropic
    /// (`cache_read_input_tokens`) expose this.
    #[serde(default)]
    pub cache_read_tokens: i64,
    /// Input tokens written to the prompt cache. **Only** the
    /// Anthropic wire shape exposes this (`cache_creation_input_tokens`);
    /// OpenAI-compat responses (DeepSeek, xAI, Groq, …) have no
    /// equivalent field and the value stays 0 there. Matches upstream
    /// `@ai-sdk/openai-compatible`.
    #[serde(default)]
    pub cache_write_tokens: i64,
}

/// Output-side token breakdown — mirrors `vercel_ai::OutputTokenDetails`.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputTokenDetails {
    /// Output tokens spent on plain text (non-reasoning).
    /// `output_tokens = text_tokens + reasoning_tokens` when the
    /// provider reports both.
    #[serde(default)]
    pub text_tokens: i64,
    /// Output tokens spent on reasoning / thinking blocks. Already
    /// counted inside `output_tokens` and billed at the output rate.
    /// Sourced from OpenAI-compat's `completion_tokens_details.reasoning_tokens`
    /// or Anthropic's thinking-block usage where exposed.
    #[serde(default)]
    pub reasoning_tokens: i64,
}

/// Per-request token counts (returned by LLM API).
///
/// **Shape mirrors `vercel_ai::LanguageModelUsage`** — the high-level
/// SDK type at `vercel-ai/ai/src/types/usage.rs`. Field names and
/// nesting are identical so a value can move across the seam without
/// information loss. `i64` is used in place of vercel-ai's `Option<u64>`
/// to match the rest of coco-rs's token-count idiom; "not reported"
/// surfaces as `0`.
///
/// Backward-compat: every nested field is `#[serde(default)]`, so old
/// session JSON without the breakdown fields parses cleanly with
/// breakdown values defaulting to 0.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Total input tokens consumed (= `no_cache + cache_read + cache_write`
    /// when all three are reported).
    pub input_tokens: i64,
    /// Total output tokens consumed (= `text + reasoning` when both
    /// are reported).
    pub output_tokens: i64,
    /// `input_tokens + output_tokens`. Convenience aggregate matching
    /// vercel-ai's `LanguageModelUsage.total_tokens`. Default to 0
    /// when not provided; `Self::total()` recomputes if needed.
    #[serde(default)]
    pub total_tokens: i64,
    /// Input-side breakdown.
    #[serde(default)]
    pub input_token_details: InputTokenDetails,
    /// Output-side breakdown.
    #[serde(default)]
    pub output_token_details: OutputTokenDetails,
}

impl TokenUsage {
    /// Compute `input_tokens + output_tokens` regardless of what's in
    /// the `total_tokens` field. Use this when totalling across calls.
    pub fn total(&self) -> i64 {
        self.input_tokens + self.output_tokens
    }

    // ─── Legacy accessors ────────────────────────────────────────────
    //
    // The flat names below match the field names this struct used to
    // expose pre-redesign. Old call sites changed `usage.foo` →
    // `usage.foo()`. New code should reach into the nested structs
    // directly.

    /// Tokens read from the prompt cache. Equivalent to
    /// `self.input_token_details.cache_read_tokens`.
    pub fn cache_read_input_tokens(&self) -> i64 {
        self.input_token_details.cache_read_tokens
    }

    /// Tokens written to the prompt cache. Equivalent to
    /// `self.input_token_details.cache_write_tokens`. Always 0 for
    /// OpenAI-compat providers (wire-shape limitation).
    pub fn cache_creation_input_tokens(&self) -> i64 {
        self.input_token_details.cache_write_tokens
    }

    /// Output tokens spent on reasoning. Equivalent to
    /// `self.output_token_details.reasoning_tokens`.
    pub fn reasoning_output_tokens(&self) -> i64 {
        self.output_token_details.reasoning_tokens
    }

    /// Output tokens spent on text. Equivalent to
    /// `self.output_token_details.text_tokens`.
    pub fn text_output_tokens(&self) -> i64 {
        self.output_token_details.text_tokens
    }
}

impl std::ops::Add for TokenUsage {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            input_tokens: self.input_tokens + rhs.input_tokens,
            output_tokens: self.output_tokens + rhs.output_tokens,
            total_tokens: self.total_tokens + rhs.total_tokens,
            input_token_details: InputTokenDetails {
                no_cache_tokens: self.input_token_details.no_cache_tokens
                    + rhs.input_token_details.no_cache_tokens,
                cache_read_tokens: self.input_token_details.cache_read_tokens
                    + rhs.input_token_details.cache_read_tokens,
                cache_write_tokens: self.input_token_details.cache_write_tokens
                    + rhs.input_token_details.cache_write_tokens,
            },
            output_token_details: OutputTokenDetails {
                text_tokens: self.output_token_details.text_tokens
                    + rhs.output_token_details.text_tokens,
                reasoning_tokens: self.output_token_details.reasoning_tokens
                    + rhs.output_token_details.reasoning_tokens,
            },
        }
    }
}

impl std::ops::AddAssign for TokenUsage {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

/// Per-model accumulated usage (for cost tracking in coco-messages).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_creation_input_tokens: i64,
    pub web_search_requests: i64,
    pub cost_usd: f64,
}

impl ModelUsage {
    pub fn accumulate(&mut self, usage: TokenUsage, cost: f64) {
        self.input_tokens += usage.input_tokens;
        self.output_tokens += usage.output_tokens;
        self.cache_read_input_tokens += usage.input_token_details.cache_read_tokens;
        self.cache_creation_input_tokens += usage.input_token_details.cache_write_tokens;
        self.cost_usd += cost;
    }
}
