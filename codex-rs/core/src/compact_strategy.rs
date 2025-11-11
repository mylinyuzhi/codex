use codex_protocol::models::ResponseItem;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

/// Context provided to compact strategies
pub struct CompactContext {
    /// User messages extracted from history
    pub user_messages: Vec<String>,
    /// Complete conversation history
    pub history: Vec<ResponseItem>,
    /// Current working directory
    pub cwd: PathBuf,
}

/// Trait for implementing conversation compaction strategies
pub trait CompactStrategy: Send + Sync {
    /// Unique identifier for this strategy
    fn name(&self) -> &str;

    /// Generate the prompt used to request summary from the LLM
    fn generate_prompt(&self) -> &str;

    /// Build the compacted history after receiving the summary
    ///
    /// This is the extension point for different strategies:
    /// - Simple: just adds user messages and summary
    /// - FileRecovery: adds user messages, summary, AND recovered files
    fn build_compacted_history(
        &self,
        initial_context: Vec<ResponseItem>,
        user_messages: &[String],
        summary_text: &str,
        context: &CompactContext,
    ) -> Vec<ResponseItem>;
}

/// Registry for compact strategies
pub struct CompactStrategyRegistry {
    strategies: HashMap<String, Box<dyn CompactStrategy>>,
}

impl CompactStrategyRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            strategies: HashMap::new(),
        };

        // Register built-in strategies
        registry.register(Box::new(crate::compact_strategies::SimpleStrategy));
        registry.register(Box::new(
            crate::compact_strategies::FileRecoveryStrategy::new(),
        ));

        registry
    }

    fn register(&mut self, strategy: Box<dyn CompactStrategy>) {
        self.strategies
            .insert(strategy.name().to_string(), strategy);
    }

    pub fn get(&self, name: &str) -> Option<&dyn CompactStrategy> {
        self.strategies.get(name).map(std::convert::AsRef::as_ref)
    }
}

/// Global registry instance
static COMPACT_REGISTRY: LazyLock<CompactStrategyRegistry> =
    LazyLock::new(CompactStrategyRegistry::new);

/// Get a compact strategy by name, falling back to "simple" if not found
pub fn get_strategy(name: &str) -> &'static dyn CompactStrategy {
    COMPACT_REGISTRY.get(name).unwrap_or_else(|| {
        COMPACT_REGISTRY
            .get("simple")
            .expect("simple strategy must exist")
    })
}
