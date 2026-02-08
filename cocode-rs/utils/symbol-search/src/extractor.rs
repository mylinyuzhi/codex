//! Symbol extraction using tree-sitter-tags.

use tree_sitter_tags::TagsContext;

use crate::SymbolKind;
use crate::languages::SymbolLanguage;

/// An extracted symbol tag.
#[derive(Debug, Clone)]
pub struct SymbolTag {
    /// Symbol name.
    pub name: String,
    /// Kind of symbol.
    pub kind: SymbolKind,
    /// Start line (1-indexed for display).
    pub line: i32,
    /// Whether this is a definition (vs reference).
    pub is_definition: bool,
}

/// Tag extractor using tree-sitter-tags.
pub struct SymbolExtractor {
    context: TagsContext,
}

impl Default for SymbolExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl SymbolExtractor {
    pub fn new() -> Self {
        Self {
            context: TagsContext::new(),
        }
    }

    /// Extract symbol tags from source code.
    pub fn extract(
        &mut self,
        source: &str,
        language: SymbolLanguage,
    ) -> anyhow::Result<Vec<SymbolTag>> {
        let config = language.tags_configuration()?;
        let source_bytes = source.as_bytes();

        let (tags, _errors) = self
            .context
            .generate_tags(&config, source_bytes, None)
            .map_err(|e| anyhow::anyhow!("Failed to generate tags: {e:?}"))?;

        let mut result = Vec::new();

        for tag in tags {
            let tag = match tag {
                Ok(t) => t,
                Err(_) => continue,
            };

            let name_range = tag.name_range;
            let name = std::str::from_utf8(&source_bytes[name_range.start..name_range.end])
                .unwrap_or("")
                .to_string();

            if name.is_empty() {
                continue;
            }

            let syntax_type = config.syntax_type_name(tag.syntax_type_id);
            let line = source[..tag.range.start].lines().count() as i32 + 1;

            result.push(SymbolTag {
                name,
                kind: SymbolKind::from_syntax_type(syntax_type),
                line,
                is_definition: tag.is_definition,
            });
        }

        Ok(result)
    }

    /// Extract symbol tags from a file.
    pub fn extract_file(&mut self, path: &std::path::Path) -> anyhow::Result<Vec<SymbolTag>> {
        let source = std::fs::read_to_string(path)?;
        let language = SymbolLanguage::from_path(path)
            .ok_or_else(|| anyhow::anyhow!("Unsupported language: {}", path.display()))?;
        self.extract(&source, language)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rust() {
        let source = r#"
struct Point {
    x: i32,
    y: i32,
}

fn main() {
    let p = Point { x: 1, y: 2 };
}
"#;
        let mut extractor = SymbolExtractor::new();
        let tags = extractor
            .extract(source, SymbolLanguage::Rust)
            .expect("extract failed");

        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"Point"), "Should contain Point");
        assert!(names.contains(&"main"), "Should contain main");
    }

    #[test]
    fn test_extract_python() {
        let source = r#"
class Calculator:
    def add(self, a, b):
        return a + b

def main():
    calc = Calculator()
"#;
        let mut extractor = SymbolExtractor::new();
        let tags = extractor
            .extract(source, SymbolLanguage::Python)
            .expect("extract failed");

        let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"Calculator"));
        assert!(names.contains(&"main"));
    }

    #[test]
    fn test_line_numbers_are_1_indexed() {
        let source = "fn foo() {}\nfn bar() {}\n";
        let mut extractor = SymbolExtractor::new();
        let tags = extractor
            .extract(source, SymbolLanguage::Rust)
            .expect("extract failed");

        let foo = tags
            .iter()
            .find(|t| t.name == "foo")
            .expect("foo not found");
        assert_eq!(foo.line, 1);

        let bar = tags
            .iter()
            .find(|t| t.name == "bar")
            .expect("bar not found");
        assert_eq!(bar.line, 2);
    }
}
