//! Dependency graph for repo map PageRank.
//!
//! Builds a directed graph of file dependencies based on symbol definitions
//! and references extracted via tree-sitter tags.

use std::collections::HashMap;
use std::collections::HashSet;

use petgraph::graph::DiGraph;
use petgraph::graph::NodeIndex;

use crate::tags::extractor::CodeTag;

/// Edge data for the dependency graph.
#[derive(Debug, Clone)]
pub struct EdgeData {
    /// Base weight for this edge
    pub weight: f64,
    /// Symbol name that created this edge
    pub symbol: String,
}

/// Dependency graph for PageRank-based file ranking.
///
/// Nodes are file paths, edges represent symbol references between files.
pub struct DependencyGraph {
    /// The underlying petgraph
    graph: DiGraph<String, EdgeData>,
    /// Map from filepath to node index
    node_indices: HashMap<String, NodeIndex>,
    /// Map from symbol name to defining files
    definitions: HashMap<String, Vec<(String, CodeTag)>>,
    /// Map from symbol name to referencing files
    references: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    /// Create a new empty dependency graph.
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_indices: HashMap::new(),
            definitions: HashMap::new(),
            references: HashMap::new(),
        }
    }

    /// Clear the graph for reuse.
    pub fn clear(&mut self) {
        self.graph.clear();
        self.node_indices.clear();
        self.definitions.clear();
        self.references.clear();
    }

    /// Get or create a node for a file path.
    fn get_or_create_node(&mut self, filepath: &str) -> NodeIndex {
        if let Some(&idx) = self.node_indices.get(filepath) {
            idx
        } else {
            let idx = self.graph.add_node(filepath.to_string());
            self.node_indices.insert(filepath.to_string(), idx);
            idx
        }
    }

    /// Add tags from a file to the graph.
    ///
    /// Separates definitions and references for later edge building.
    pub fn add_file_tags(&mut self, filepath: &str, tags: &[CodeTag]) {
        // Ensure node exists
        self.get_or_create_node(filepath);

        for tag in tags {
            if tag.is_definition {
                self.definitions
                    .entry(tag.name.clone())
                    .or_default()
                    .push((filepath.to_string(), tag.clone()));
            } else {
                self.references
                    .entry(tag.name.clone())
                    .or_default()
                    .push(filepath.to_string());
            }
        }
    }

    /// Build weighted edges based on symbol references.
    ///
    /// Edge direction: referencing file -> defining file
    /// (PageRank will flow importance to files that are referenced)
    pub fn build_edges(
        &mut self,
        chat_files: &HashSet<String>,
        mentioned_idents: &HashSet<String>,
        chat_file_weight: f32,
        mentioned_ident_weight: f32,
        private_symbol_weight: f32,
    ) {
        // First, collect all edges to create (to avoid borrow conflicts)
        let mut edges_to_add: Vec<(String, String, f64, String)> = Vec::new();

        // For each symbol with both definitions and references
        for (symbol, def_files) in &self.definitions {
            let ref_files = match self.references.get(symbol) {
                Some(refs) => refs,
                None => continue, // No references to this symbol
            };

            // Calculate symbol-level weight multipliers
            let mut weight = 1.0_f64;

            // Private symbol penalty (underscore prefix)
            if symbol.starts_with('_') {
                weight *= private_symbol_weight as f64;
            }

            // Mentioned identifier boost
            if mentioned_idents.contains(symbol) {
                weight *= mentioned_ident_weight as f64;
            }

            // Multi-defined penalty (>5 files = utility/noise)
            if def_files.len() > 5 {
                weight *= 0.1;
            }

            // High-frequency reference dampening (sqrt)
            if ref_files.len() > 10 {
                weight *= (10.0_f64 / ref_files.len() as f64).sqrt();
            }

            // Collect edges from referencing files to defining files
            for ref_file in ref_files {
                for (def_file, _tag) in def_files {
                    // Skip self-references
                    if ref_file == def_file {
                        continue;
                    }

                    // Apply chat file boost to edges from chat files
                    let mut edge_weight = weight;
                    if chat_files.contains(ref_file) {
                        edge_weight *= chat_file_weight as f64;
                    }

                    edges_to_add.push((
                        ref_file.clone(),
                        def_file.clone(),
                        edge_weight,
                        symbol.clone(),
                    ));
                }
            }
        }

        // Now add all edges to the graph
        for (ref_file, def_file, edge_weight, symbol) in edges_to_add {
            let ref_idx = self.get_or_create_node(&ref_file);
            let def_idx = self.get_or_create_node(&def_file);

            self.graph.add_edge(
                ref_idx,
                def_idx,
                EdgeData {
                    weight: edge_weight,
                    symbol,
                },
            );
        }
    }

    /// Build personalization vector for PageRank.
    ///
    /// Chat files get higher initial probability.
    pub fn build_personalization(&self, chat_files: &HashSet<String>) -> HashMap<String, f64> {
        let mut personalization = HashMap::new();
        let node_count = self.graph.node_count();

        if node_count == 0 {
            return personalization;
        }

        // Default uniform distribution
        let default_prob = 1.0 / node_count as f64;

        // Chat files get 50x boost in initial probability
        let chat_boost = 50.0;
        let chat_count = chat_files.len();
        let non_chat_count = node_count.saturating_sub(chat_count);

        // Calculate probabilities that sum to 1.0
        let (chat_prob, non_chat_prob) = if chat_count > 0 && non_chat_count > 0 {
            // Total probability = chat_count * chat_prob + non_chat_count * non_chat_prob = 1.0
            // With chat_prob = chat_boost * non_chat_prob
            let non_chat_prob = 1.0 / (chat_count as f64 * chat_boost + non_chat_count as f64);
            let chat_prob = chat_boost * non_chat_prob;
            (chat_prob, non_chat_prob)
        } else if chat_count > 0 {
            (1.0 / chat_count as f64, 0.0)
        } else {
            (0.0, default_prob)
        };

        for (filepath, _idx) in &self.node_indices {
            let prob = if chat_files.contains(filepath) {
                chat_prob
            } else {
                non_chat_prob
            };
            personalization.insert(filepath.clone(), prob);
        }

        personalization
    }

    /// Get the underlying graph for PageRank computation.
    pub fn graph(&self) -> &DiGraph<String, EdgeData> {
        &self.graph
    }

    /// Get the definitions map for symbol lookup.
    pub fn definitions(&self) -> &HashMap<String, Vec<(String, CodeTag)>> {
        &self.definitions
    }

    /// Get file count in the graph.
    pub fn file_count(&self) -> usize {
        self.node_indices.len()
    }

    /// Get edge count in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::extractor::TagKind;

    fn make_def(name: &str, line: i32) -> CodeTag {
        CodeTag {
            name: name.to_string(),
            kind: TagKind::Function,
            start_line: line,
            end_line: line + 10,
            start_byte: line * 100,
            end_byte: (line + 10) * 100,
            signature: Some(format!("fn {}()", name)),
            docs: None,
            is_definition: true,
        }
    }

    fn make_ref(name: &str, line: i32) -> CodeTag {
        CodeTag {
            name: name.to_string(),
            kind: TagKind::Function,
            start_line: line,
            end_line: line,
            start_byte: line * 100,
            end_byte: line * 100,
            signature: None,
            docs: None,
            is_definition: false,
        }
    }

    #[test]
    fn test_build_graph() {
        let mut graph = DependencyGraph::new();

        // file_a.rs defines foo, references bar
        graph.add_file_tags("file_a.rs", &[make_def("foo", 10), make_ref("bar", 20)]);

        // file_b.rs defines bar, references foo
        graph.add_file_tags("file_b.rs", &[make_def("bar", 5), make_ref("foo", 15)]);

        assert_eq!(graph.file_count(), 2);
        assert_eq!(graph.definitions().len(), 2);

        // Build edges with default weights
        graph.build_edges(&HashSet::new(), &HashSet::new(), 50.0, 10.0, 0.1);

        // Should have 2 edges: a->b (bar ref) and b->a (foo ref)
        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn test_personalization() {
        let mut graph = DependencyGraph::new();
        graph.add_file_tags("a.rs", &[make_def("x", 1)]);
        graph.add_file_tags("b.rs", &[make_def("y", 1)]);
        graph.add_file_tags("c.rs", &[make_def("z", 1)]);

        let chat_files: HashSet<String> = ["a.rs".to_string()].into_iter().collect();
        let pers = graph.build_personalization(&chat_files);

        // Chat file should have higher probability
        assert!(pers["a.rs"] > pers["b.rs"]);
        assert!(pers["a.rs"] > pers["c.rs"]);

        // Probabilities should sum to ~1.0
        let sum: f64 = pers.values().sum();
        assert!((sum - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_private_symbol_penalty() {
        let mut graph = DependencyGraph::new();

        // _private symbol should get penalty
        graph.add_file_tags("a.rs", &[make_def("_private", 1)]);
        graph.add_file_tags("b.rs", &[make_ref("_private", 1)]);

        graph.build_edges(&HashSet::new(), &HashSet::new(), 50.0, 10.0, 0.1);

        // Edge should exist with reduced weight
        assert_eq!(graph.edge_count(), 1);
    }
}
