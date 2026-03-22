use serde::Deserialize;
use serde::Serialize;

/// Configuration for model fallback behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    /// Whether model fallback is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Ordered list of fallback models to try when the primary model fails.
    #[serde(default)]
    pub fallback_models: Vec<String>,

    /// Maximum number of retry attempts before giving up.
    #[serde(default = "default_max_retries")]
    pub max_retries: i32,
}

fn default_max_retries() -> i32 {
    3
}

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            fallback_models: Vec::new(),
            max_retries: default_max_retries(),
        }
    }
}

/// Tracks the current fallback state during loop execution.
pub struct FallbackState {
    /// The model currently being used.
    pub current_model: String,

    /// Number of fallback attempts made so far.
    pub attempts: i32,

    /// History of all fallback transitions.
    pub history: Vec<FallbackAttempt>,
}

/// A single fallback transition record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackAttempt {
    /// The model that failed.
    pub from_model: String,

    /// The model that was switched to.
    pub to_model: String,

    /// Human-readable reason for the fallback.
    pub reason: String,
}

impl FallbackState {
    /// Create a new fallback state for the given primary model.
    pub fn new(model: String) -> Self {
        Self {
            current_model: model,
            attempts: 0,
            history: Vec::new(),
        }
    }

    /// Returns `true` when a fallback should be attempted (fallback is enabled
    /// and we have not exceeded the retry limit).
    pub fn should_fallback(&self, config: &FallbackConfig) -> bool {
        config.enabled && self.attempts < config.max_retries && !config.fallback_models.is_empty()
    }

    /// Select the next fallback model, if one is available.
    ///
    /// Models are tried in the order they appear in `config.fallback_models`.
    /// Returns `None` when all options have been exhausted.
    pub fn next_model(&self, config: &FallbackConfig) -> Option<String> {
        if !config.enabled || config.fallback_models.is_empty() {
            return None;
        }

        let idx = self.attempts as usize;
        config.fallback_models.get(idx).cloned()
    }

    /// Record a fallback transition.
    pub fn record_fallback(&mut self, to: String, reason: String) {
        self.history.push(FallbackAttempt {
            from_model: self.current_model.clone(),
            to_model: to.clone(),
            reason,
        });
        self.current_model = to;
        self.attempts += 1;
    }
}

#[cfg(test)]
#[path = "fallback.test.rs"]
mod tests;
