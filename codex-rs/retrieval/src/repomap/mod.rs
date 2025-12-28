//! Repo map module for PageRank-based context generation.
//!
//! Provides intelligent codebase context generation for LLMs using:
//! - Tree-sitter tag extraction (definitions and references)
//! - PageRank-based file/symbol importance ranking
//! - Token-budgeted output generation
//! - 3-level caching (SQLite, in-memory LRU, TTL)
//!
//! Inspired by Aider's repo map feature.

pub mod budget;
pub mod cache;
pub mod graph;
pub mod pagerank;
pub mod renderer;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::config::RepoMapConfig;
use crate::error::Result;
use crate::storage::SqliteStore;
use crate::tags::extractor::CodeTag;

pub use budget::TokenBudgeter;
pub use cache::RepoMapCache;
pub use graph::DependencyGraph;
pub use pagerank::PageRanker;
pub use renderer::TreeRenderer;

/// Ranked file with PageRank score.
#[derive(Debug, Clone)]
pub struct RankedFile {
    /// File path (relative to workspace root)
    pub filepath: String,
    /// PageRank score
    pub rank: f64,
    /// Ranked symbols in this file
    pub symbols: Vec<RankedSymbol>,
}

/// Ranked symbol with PageRank score.
#[derive(Debug, Clone)]
pub struct RankedSymbol {
    /// The code tag (reused from existing extractor)
    pub tag: CodeTag,
    /// PageRank score for this symbol
    pub rank: f64,
}

/// Repo map generation request.
#[derive(Debug, Clone)]
pub struct RepoMapRequest {
    /// Files currently in chat context (get 50x weight boost)
    pub chat_files: Vec<PathBuf>,
    /// Other repository files to include
    pub other_files: Vec<PathBuf>,
    /// File names mentioned by user
    pub mentioned_fnames: HashSet<String>,
    /// Identifiers mentioned by user (10x weight boost)
    pub mentioned_idents: HashSet<String>,
    /// Maximum tokens for output
    pub max_tokens: i32,
}

impl Default for RepoMapRequest {
    fn default() -> Self {
        Self {
            chat_files: Vec::new(),
            other_files: Vec::new(),
            mentioned_fnames: HashSet::new(),
            mentioned_idents: HashSet::new(),
            max_tokens: 1024,
        }
    }
}

/// Repo map generation result.
#[derive(Debug, Clone)]
pub struct RepoMapResult {
    /// Rendered tree output
    pub content: String,
    /// Actual token count
    pub tokens: i32,
    /// Number of files included
    pub files_included: i32,
    /// Generation time in milliseconds
    pub generation_time_ms: i64,
}

/// Main repo map service.
///
/// Coordinates tag extraction, graph building, PageRank ranking,
/// token budgeting, and tree rendering.
pub struct RepoMapService {
    config: RepoMapConfig,
    cache: RepoMapCache,
    graph: DependencyGraph,
    ranker: PageRanker,
    budgeter: TokenBudgeter,
    renderer: TreeRenderer,
    workspace_root: PathBuf,
}

impl RepoMapService {
    /// Create a new repo map service.
    pub fn new(
        config: RepoMapConfig,
        db: Arc<SqliteStore>,
        workspace_root: PathBuf,
    ) -> Result<Self> {
        let cache = RepoMapCache::new(db, config.cache_ttl_secs);
        let graph = DependencyGraph::new();
        let ranker = PageRanker::new(
            config.damping_factor,
            config.max_iterations,
            config.tolerance,
        );
        let budgeter = TokenBudgeter::new()?;
        let renderer = TreeRenderer::new();

        Ok(Self {
            config,
            cache,
            graph,
            ranker,
            budgeter,
            renderer,
            workspace_root,
        })
    }

    /// Generate a repo map for the given request.
    pub async fn generate(&mut self, request: &RepoMapRequest) -> Result<RepoMapResult> {
        let start = Instant::now();

        // Collect all files
        let mut all_files: Vec<PathBuf> = request.chat_files.clone();
        all_files.extend(request.other_files.clone());

        // Build file sets for personalization
        let chat_file_set: HashSet<String> = request
            .chat_files
            .iter()
            .filter_map(|p| p.to_str().map(String::from))
            .collect();

        // Extract tags for all files (using cache where possible)
        let file_tags = self.extract_tags_for_files(&all_files).await?;

        // Build dependency graph
        self.graph.clear();
        for (filepath, tags) in &file_tags {
            self.graph.add_file_tags(filepath, tags);
        }

        // Build weighted edges with personalization
        self.graph.build_edges(
            &chat_file_set,
            &request.mentioned_idents,
            self.config.chat_file_weight,
            self.config.mentioned_ident_weight,
            self.config.private_symbol_weight,
        );

        // Run PageRank
        let personalization = self.graph.build_personalization(&chat_file_set);
        let file_ranks = self.ranker.rank(self.graph.graph(), &personalization)?;

        // Distribute ranks to symbols
        let ranked_symbols = self
            .ranker
            .distribute_to_definitions(&file_ranks, self.graph.definitions());

        // Determine token budget
        let max_tokens = if request.chat_files.is_empty() {
            (request.max_tokens as f32 * self.config.map_mul_no_files) as i32
        } else {
            request.max_tokens
        };

        // Find optimal tag count via binary search
        let optimal_count =
            self.budgeter
                .find_optimal_count(&ranked_symbols, &self.renderer, max_tokens);

        // Render the tree
        let content = self.renderer.render(
            &ranked_symbols,
            &chat_file_set,
            optimal_count,
            &self.workspace_root,
        );

        // Count tokens in final output
        let tokens = self.budgeter.count_tokens(&content);

        let generation_time_ms = start.elapsed().as_millis() as i64;

        Ok(RepoMapResult {
            content,
            tokens,
            files_included: file_tags.len() as i32,
            generation_time_ms,
        })
    }

    /// Get ranked files without rendering (for search integration).
    pub async fn get_ranked_files(
        &mut self,
        chat_files: &[PathBuf],
        other_files: &[PathBuf],
        mentioned_idents: &HashSet<String>,
    ) -> Result<Vec<RankedFile>> {
        let mut all_files: Vec<PathBuf> = chat_files.to_vec();
        all_files.extend(other_files.to_vec());

        let chat_file_set: HashSet<String> = chat_files
            .iter()
            .filter_map(|p| p.to_str().map(String::from))
            .collect();

        let file_tags = self.extract_tags_for_files(&all_files).await?;

        self.graph.clear();
        for (filepath, tags) in &file_tags {
            self.graph.add_file_tags(filepath, tags);
        }

        self.graph.build_edges(
            &chat_file_set,
            mentioned_idents,
            self.config.chat_file_weight,
            self.config.mentioned_ident_weight,
            self.config.private_symbol_weight,
        );

        let personalization = self.graph.build_personalization(&chat_file_set);
        let file_ranks = self.ranker.rank(self.graph.graph(), &personalization)?;

        // Group by file
        let mut ranked_files: Vec<RankedFile> = file_ranks
            .iter()
            .map(|(filepath, rank)| {
                let symbols = file_tags
                    .get(filepath)
                    .map(|tags| {
                        tags.iter()
                            .filter(|t| t.is_definition)
                            .map(|tag| RankedSymbol {
                                tag: tag.clone(),
                                rank: *rank,
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                RankedFile {
                    filepath: filepath.clone(),
                    rank: *rank,
                    symbols,
                }
            })
            .collect();

        // Sort by rank descending
        ranked_files.sort_by(|a, b| {
            b.rank
                .partial_cmp(&a.rank)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(ranked_files)
    }

    /// Extract mentions from a user message.
    ///
    /// Returns (mentioned_fnames, mentioned_idents).
    pub fn extract_mentions(message: &str) -> (HashSet<String>, HashSet<String>) {
        let mut fnames = HashSet::new();
        let mut idents = HashSet::new();

        // Split on non-word characters
        for word in
            message.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.' && c != '/')
        {
            let word = word.trim();
            if word.is_empty() {
                continue;
            }

            // Check if it looks like a file path
            if word.contains('/') || word.contains('.') {
                if let Some(fname) = word.split('/').last() {
                    if fname.contains('.') {
                        fnames.insert(fname.to_string());
                    }
                }
            }

            // Check if it's a valid identifier
            if word.len() >= 3
                && word
                    .chars()
                    .next()
                    .map(|c| c.is_alphabetic() || c == '_')
                    .unwrap_or(false)
            {
                idents.insert(word.to_string());
            }
        }

        (fnames, idents)
    }

    /// Extract tags for files (using cache).
    async fn extract_tags_for_files(
        &self,
        files: &[PathBuf],
    ) -> Result<std::collections::HashMap<String, Vec<CodeTag>>> {
        let mut result = std::collections::HashMap::new();

        for file in files {
            let filepath = file.to_string_lossy().to_string();

            // Try cache first
            if let Some(cached_tags) = self.cache.get_tags(&filepath).await? {
                result.insert(filepath, cached_tags);
                continue;
            }

            // Extract tags from file
            let mut extractor = crate::tags::extractor::TagExtractor::new();
            match extractor.extract_file(file) {
                Ok(tags) => {
                    // Cache the tags
                    self.cache.put_tags(&filepath, &tags).await?;
                    result.insert(filepath, tags);
                }
                Err(e) => {
                    tracing::debug!(file = %filepath, error = %e, "Failed to extract tags");
                    // Continue with other files
                }
            }
        }

        Ok(result)
    }
}
