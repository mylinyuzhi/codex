//! Tree renderer for repo map output.
//!
//! Formats ranked symbols as a tree structure with file paths
//! and line numbers for LLM context.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;

use super::RankedSymbol;

/// Tree renderer for repo map output.
pub struct TreeRenderer {
    /// Include line numbers in output
    show_line_numbers: bool,
    /// Include signatures in output
    show_signatures: bool,
}

impl TreeRenderer {
    /// Create a new tree renderer with default settings.
    pub fn new() -> Self {
        Self {
            show_line_numbers: true,
            show_signatures: true,
        }
    }

    /// Create a renderer with custom settings.
    #[allow(dead_code)]
    pub fn with_options(show_line_numbers: bool, show_signatures: bool) -> Self {
        Self {
            show_line_numbers,
            show_signatures,
        }
    }

    /// Render ranked symbols as a tree.
    ///
    /// # Arguments
    /// * `symbols` - Ranked symbols sorted by rank descending
    /// * `chat_files` - Files in chat context (highlighted)
    /// * `count` - Number of symbols to include
    /// * `workspace_root` - Workspace root for relative path display
    pub fn render(
        &self,
        symbols: &[RankedSymbol],
        chat_files: &HashSet<String>,
        count: i32,
        workspace_root: &Path,
    ) -> String {
        // Take top N symbols
        let symbols_to_render = &symbols[..symbols.len().min(count as usize)];

        // Group by file
        let mut file_symbols: HashMap<String, Vec<&RankedSymbol>> = HashMap::new();
        for sym in symbols_to_render {
            // Determine file path from tag (using name as fallback)
            // In real usage, we'd need to track filepath with the symbol
            let filepath = self.extract_filepath_from_symbol(sym, workspace_root);
            file_symbols.entry(filepath).or_default().push(sym);
        }

        // Sort files by their highest-ranked symbol
        let mut file_order: Vec<(String, f64)> = file_symbols
            .iter()
            .map(|(path, syms)| {
                let max_rank = syms.iter().map(|s| s.rank).fold(0.0_f64, f64::max);
                (path.clone(), max_rank)
            })
            .collect();
        file_order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Render output
        let mut output = String::new();

        for (filepath, _rank) in file_order {
            let syms = match file_symbols.get(&filepath) {
                Some(s) => s,
                None => continue,
            };

            // File header (highlight chat files)
            let is_chat_file = chat_files.contains(&filepath);
            if is_chat_file {
                output.push_str(&format!("{}:  [chat]\n", filepath));
            } else {
                output.push_str(&format!("{}:\n", filepath));
            }

            // Sort symbols by line number within file
            let mut sorted_syms: Vec<&&RankedSymbol> = syms.iter().collect();
            sorted_syms.sort_by_key(|s| s.tag.start_line);

            // Render each symbol
            for sym in sorted_syms {
                self.render_symbol(&mut output, sym);
            }

            output.push('\n');
        }

        output.trim_end().to_string()
    }

    /// Render symbols without file context (for token counting).
    pub fn render_symbols(&self, symbols: &[RankedSymbol], count: i32) -> String {
        if symbols.is_empty() || count <= 0 {
            return String::new();
        }

        let mut output = String::new();
        let symbols_to_render = &symbols[..symbols.len().min(count as usize)];

        // Group by a derived filepath (from signature context if available)
        let mut current_file: Option<String> = None;

        for sym in symbols_to_render {
            let filepath = format!("file_{}.rs", sym.tag.start_line / 100);

            // Add file header if changed
            if current_file.as_ref() != Some(&filepath) {
                if current_file.is_some() {
                    output.push('\n');
                }
                output.push_str(&format!("{}:\n", filepath));
                current_file = Some(filepath);
            }

            self.render_symbol(&mut output, sym);
        }

        output
    }

    /// Render a single symbol.
    fn render_symbol(&self, output: &mut String, sym: &RankedSymbol) {
        let tag = &sym.tag;

        if self.show_line_numbers {
            output.push_str(&format!("│{:>4}: ", tag.start_line));
        } else {
            output.push_str("│  ");
        }

        if self.show_signatures && tag.signature.is_some() {
            output.push_str(tag.signature.as_ref().unwrap());
        } else {
            // Fallback to kind + name
            output.push_str(&format!("{:?} {}", tag.kind, tag.name));
        }

        output.push('\n');
    }

    /// Extract filepath from symbol (placeholder - real implementation
    /// would track this with the symbol).
    fn extract_filepath_from_symbol(&self, sym: &RankedSymbol, _workspace_root: &Path) -> String {
        // In actual usage, filepath should be stored with RankedSymbol
        // For now, derive from line number as a placeholder
        format!("src/file_{}.rs", sym.tag.start_line / 100)
    }
}

impl Default for TreeRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::extractor::CodeTag;
    use crate::tags::extractor::TagKind;

    fn make_symbol(name: &str, line: i32, signature: &str) -> RankedSymbol {
        RankedSymbol {
            tag: CodeTag {
                name: name.to_string(),
                kind: TagKind::Function,
                start_line: line,
                end_line: line + 10,
                start_byte: line * 100,
                end_byte: (line + 10) * 100,
                signature: Some(signature.to_string()),
                docs: None,
                is_definition: true,
            },
            rank: 1.0 / (line as f64),
        }
    }

    #[test]
    fn test_render_empty() {
        let renderer = TreeRenderer::new();
        let output = renderer.render_symbols(&[], 10);
        assert!(output.is_empty());
    }

    #[test]
    fn test_render_single_symbol() {
        let renderer = TreeRenderer::new();
        let symbols = vec![make_symbol("foo", 10, "fn foo() -> i32")];

        let output = renderer.render_symbols(&symbols, 1);

        assert!(output.contains("fn foo() -> i32"));
        assert!(output.contains("10:"));
    }

    #[test]
    fn test_render_multiple_symbols() {
        let renderer = TreeRenderer::new();
        let symbols = vec![
            make_symbol("foo", 10, "fn foo()"),
            make_symbol("bar", 20, "fn bar()"),
            make_symbol("baz", 30, "fn baz()"),
        ];

        let output = renderer.render_symbols(&symbols, 3);

        assert!(output.contains("fn foo()"));
        assert!(output.contains("fn bar()"));
        assert!(output.contains("fn baz()"));
    }

    #[test]
    fn test_render_with_count_limit() {
        let renderer = TreeRenderer::new();
        let symbols = vec![
            make_symbol("foo", 10, "fn foo()"),
            make_symbol("bar", 20, "fn bar()"),
            make_symbol("baz", 30, "fn baz()"),
        ];

        let output = renderer.render_symbols(&symbols, 2);

        assert!(output.contains("fn foo()"));
        assert!(output.contains("fn bar()"));
        assert!(!output.contains("fn baz()"));
    }

    #[test]
    fn test_render_without_line_numbers() {
        let renderer = TreeRenderer::with_options(false, true);
        let symbols = vec![make_symbol("foo", 10, "fn foo()")];

        let output = renderer.render_symbols(&symbols, 1);

        assert!(output.contains("fn foo()"));
        assert!(!output.contains("10:"));
    }

    #[test]
    fn test_render_full_tree() {
        let renderer = TreeRenderer::new();
        let symbols = vec![
            make_symbol("process", 100, "fn process(req: Request) -> Response"),
            make_symbol("handle", 150, "fn handle(data: &[u8])"),
        ];

        let chat_files: HashSet<String> = ["src/file_1.rs".to_string()].into_iter().collect();
        let output = renderer.render(&symbols, &chat_files, 2, Path::new("/project"));

        // Should have file headers and symbol lines
        assert!(output.contains(".rs:"));
        assert!(output.contains("fn process"));
    }
}
