//! Code-fence syntax highlighting via syntect, mapped onto coco's themeable
//! palette.
//!
//! Unlike a stock syntect integration we do NOT use syntect's `.tmTheme`
//! palette (it is dropped at the dependency level). Instead we drive the raw
//! parser (`ParseState` + `ScopeStack`), classify each token by its TextMate
//! scope, and color it through `UiStyles.code_*`. That keeps code highlighting
//! governed by coco's hot-reloadable `Theme` and its capability downsampling.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

use coco_tui_ui::display::SyntaxHighlighting;
use coco_tui_ui::style::UiStyles;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Span;
use syntect::parsing::ParseState;
use syntect::parsing::Scope;
use syntect::parsing::ScopeStack;
use syntect::parsing::SyntaxReference;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Bundled syntax grammars, deserialized once on first highlight. Immutable —
/// the only acceptable process-global here (no mutable theme state).
/// The first-use deserialization costs tens of milliseconds and lands inside
/// whichever frame first highlights code, so it is logged for attribution.
fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(|| {
        let started = std::time::Instant::now();
        let set = SyntaxSet::load_defaults_newlines();
        tracing::info!(
            target: "tui::perf::init",
            duration_ms = started.elapsed().as_secs_f64() * 1000.0,
            "syntect syntax set loaded",
        );
        set
    })
}

/// Guardrails so a giant pasted blob cannot stall a frame on the parse.
const MAX_HIGHLIGHT_BYTES: usize = 512 * 1024;
const MAX_HIGHLIGHT_LINES: usize = 10_000;

/// Token classes coco colors. Mapped from TextMate scope prefixes.
#[derive(Debug, Clone, Copy)]
enum CodeToken {
    Keyword,
    String,
    Comment,
    Number,
    Function,
    Type,
    Operator,
    Plain,
}

impl CodeToken {
    fn style(self, styles: UiStyles<'_>) -> Style {
        match self {
            // No BOLD: a saturated keyword color plus bold reads as "loud"
            // (the old ANSI-Magenta+bold looked harsh red). Both of
            // claude-code's highlighters leave keywords unbolded.
            Self::Keyword => Style::default().fg(styles.code_keyword()),
            Self::String => Style::default().fg(styles.code_string()),
            Self::Comment => Style::default()
                .fg(styles.code_comment())
                .add_modifier(Modifier::ITALIC),
            Self::Number => Style::default().fg(styles.code_number()),
            Self::Function => Style::default().fg(styles.code_function()),
            Self::Type => Style::default().fg(styles.code_type()),
            Self::Operator => Style::default().fg(styles.code_operator()),
            Self::Plain => Style::default().fg(styles.text()),
        }
    }
}

/// Ordered (most-specific-first) scope-prefix → token table, built once.
fn scope_table() -> &'static [(Scope, CodeToken)] {
    static TABLE: OnceLock<Vec<(Scope, CodeToken)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        // Most-specific first. `Scope::new` only fails on malformed input; the
        // literals below are all valid, so `filter_map` keeps every entry while
        // avoiding an `unwrap`/`expect` on the parse result.
        const SPECS: [(&str, CodeToken); 18] = [
            ("comment", CodeToken::Comment),
            ("string", CodeToken::String),
            ("constant.character.escape", CodeToken::String),
            ("constant.numeric", CodeToken::Number),
            ("keyword.operator", CodeToken::Operator),
            ("punctuation.separator", CodeToken::Operator),
            ("punctuation.accessor", CodeToken::Operator),
            ("entity.name.function", CodeToken::Function),
            ("support.function", CodeToken::Function),
            ("variable.function", CodeToken::Function),
            ("entity.name.type", CodeToken::Type),
            ("entity.name.class", CodeToken::Type),
            ("storage.type", CodeToken::Type),
            ("support.type", CodeToken::Type),
            ("support.class", CodeToken::Type),
            ("storage.modifier", CodeToken::Keyword),
            ("constant.language", CodeToken::Keyword),
            ("keyword", CodeToken::Keyword),
        ];
        SPECS
            .iter()
            .filter_map(|(name, token)| Scope::new(name).ok().map(|scope| (scope, *token)))
            .collect()
    })
}

/// Classify a token by walking its scope stack from most- to least-specific.
fn classify(stack: &ScopeStack) -> CodeToken {
    let table = scope_table();
    for scope in stack.as_slice().iter().rev() {
        for (prefix, token) in table {
            if prefix.is_prefix_of(*scope) {
                return *token;
            }
        }
    }
    CodeToken::Plain
}

/// Resolve a fence language tag to a syntect syntax, with common aliases.
fn find_syntax<'s>(ss: &'s SyntaxSet, lang: &str) -> Option<&'s SyntaxReference> {
    let lang = lang.trim();
    if lang.is_empty() {
        return None;
    }
    if let Some(s) = ss
        .find_syntax_by_token(lang)
        .or_else(|| ss.find_syntax_by_extension(lang))
    {
        return Some(s);
    }
    let alias = match lang.to_ascii_lowercase().as_str() {
        "rs" => "Rust",
        "py" | "python3" => "Python",
        "js" | "jsx" | "node" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        "sh" | "shell" | "zsh" | "bash" => "Bourne Again Shell (bash)",
        "yml" => "YAML",
        "rb" => "Ruby",
        "golang" => "Go",
        "cpp" | "c++" | "cxx" => "C++",
        "cs" | "c#" | "csharp" => "C#",
        "md" => "Markdown",
        "tf" => "Terraform",
        "dockerfile" => "Dockerfile",
        _ => return None,
    };
    ss.find_syntax_by_name(alias)
        .or_else(|| ss.find_syntax_by_token(alias))
}

/// Pre-compile the lazily-built syntect machinery for the grammars most
/// likely to appear in tool output and assistant markdown.
///
/// syntect deserializes the `SyntaxSet` once (~1ms, logged under
/// `tui::perf::init`) but compiles each grammar's regexes lazily on first
/// parse — tens of milliseconds per grammar, which otherwise lands inside the
/// first frame that renders a code block in that language. Run this from a
/// background thread at startup: it warms the same process-global `SyntaxSet`,
/// so every later caller parses against compiled regexes. Skipping it only
/// costs first-use latency, never correctness.
pub fn prewarm_highlighting() {
    const SNIPPET: &str = "# t *m* `c` fn x() { let y: i32 = 1; } [l](u)\n";
    const LANGS: &[&str] = &[
        "md", "rs", "sh", "json", "toml", "py", "ts", "js", "yaml", "diff", "go",
    ];
    let started = std::time::Instant::now();
    let ss = syntax_set();
    let mut warmed = 0usize;
    for lang in LANGS {
        let grammar_started = std::time::Instant::now();
        let Some(syntax_ref) = find_syntax(ss, lang) else {
            tracing::debug!(
                target: "tui::perf::init",
                lang,
                "syntect prewarm skipped unknown grammar",
            );
            continue;
        };
        let mut state = ParseState::new(syntax_ref);
        for line in LinesWithEndings::from(SNIPPET) {
            let _ = state.parse_line(line, ss);
        }
        warmed += 1;
        tracing::debug!(
            target: "tui::perf::init",
            lang,
            grammar = syntax_ref.name.as_str(),
            duration_ms = grammar_started.elapsed().as_secs_f64() * 1000.0,
            "syntect grammar prewarmed",
        );
    }
    tracing::info!(
        target: "tui::perf::init",
        duration_ms = started.elapsed().as_secs_f64() * 1000.0,
        grammars = warmed,
        "syntect grammars prewarmed",
    );
}

/// Per-code-block highlighted result: per-line styled spans, shared via `Arc`
/// so a cache hit is a refcount bump rather than a deep clone.
pub(crate) type Highlighted = Arc<Vec<Vec<Span<'static>>>>;

/// Bounded LRU of highlighted code blocks, keyed on `(content, language,
/// theme)` — deliberately **not** terminal width: syntect highlighting is
/// width-independent, so a reflow (resize / display-toggle), which re-renders
/// every transcript cell, reuses these instead of re-tokenizing through syntect
/// — the dominant reflow cost. Mirrors jcode's `HIGHLIGHT_CACHE`.
const HIGHLIGHT_CACHE_CAP: usize = 256;

#[derive(Default)]
struct HighlightCache {
    map: HashMap<u64, Highlighted>,
    lru: VecDeque<u64>,
}

impl HighlightCache {
    fn touch(&mut self, key: u64) {
        if let Some(pos) = self.lru.iter().position(|&k| k == key) {
            self.lru.remove(pos);
        }
        self.lru.push_back(key);
    }

    fn get(&mut self, key: u64) -> Option<Highlighted> {
        let hit = self.map.get(&key).map(Arc::clone)?;
        self.touch(key);
        Some(hit)
    }

    fn put(&mut self, key: u64, value: Highlighted) {
        self.map.insert(key, value);
        self.touch(key);
        while self.lru.len() > HIGHLIGHT_CACHE_CAP {
            if let Some(evicted) = self.lru.pop_front() {
                self.map.remove(&evicted);
            }
        }
    }
}

fn highlight_cache() -> &'static Mutex<HighlightCache> {
    static CACHE: OnceLock<Mutex<HighlightCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HighlightCache::default()))
}

/// Highlight a code block into per-line styled spans.
///
/// Returns `None` to request the plain fallback (highlighting disabled,
/// unknown language, oversized input, or a parser error) — the caller then
/// renders the code verbatim. Never panics on bad input. Successful results are
/// memoized width-independently (see [`HighlightCache`]).
pub(crate) fn highlight_code(
    code: &str,
    lang: &str,
    styles: UiStyles<'_>,
    syntax: SyntaxHighlighting,
) -> Option<Highlighted> {
    if syntax.is_disabled() {
        return None;
    }
    if code.len() > MAX_HIGHLIGHT_BYTES
        || code.bytes().filter(|&b| b == b'\n').count() > MAX_HIGHLIGHT_LINES
    {
        return None;
    }
    // Key on the inputs that change the highlighted spans — content, language,
    // and the active theme's palette — but NOT width. A lock-poison or a fresh
    // cache simply recomputes; the cache never affects correctness.
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        code.hash(&mut h);
        lang.hash(&mut h);
        styles.theme_hash().hash(&mut h);
        h.finish()
    };
    if let Some(hit) = highlight_cache().lock().ok().and_then(|mut c| c.get(key)) {
        return Some(hit);
    }
    let highlighted = Arc::new(highlight_uncached(code, lang, styles)?);
    if let Ok(mut cache) = highlight_cache().lock() {
        cache.put(key, Arc::clone(&highlighted));
    }
    Some(highlighted)
}

/// The syntect parse + scope-classify pass — the work the cache avoids on a hit.
fn highlight_uncached(
    code: &str,
    lang: &str,
    styles: UiStyles<'_>,
) -> Option<Vec<Vec<Span<'static>>>> {
    let ss = syntax_set();
    let syntax_ref = find_syntax(ss, lang)?;
    let mut state = ParseState::new(syntax_ref);
    let mut stack = ScopeStack::new();
    let mut out: Vec<Vec<Span<'static>>> = Vec::new();
    for line in LinesWithEndings::from(code) {
        let ops = state.parse_line(line, ss).ok()?;
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut idx = 0usize;
        for (offset, op) in &ops {
            if *offset > idx {
                push_piece(&mut spans, &line[idx..*offset], &stack, styles);
                idx = *offset;
            }
            stack.apply(op).ok()?;
        }
        if idx < line.len() {
            push_piece(&mut spans, &line[idx..], &stack, styles);
        }
        out.push(spans);
    }
    Some(out)
}

fn push_piece(
    spans: &mut Vec<Span<'static>>,
    piece: &str,
    stack: &ScopeStack,
    styles: UiStyles<'_>,
) {
    let content = piece.trim_end_matches(['\n', '\r']);
    if content.is_empty() {
        return;
    }
    let token = classify(stack);
    spans.push(Span::styled(content.to_string(), token.style(styles)));
}

#[cfg(test)]
#[path = "highlight.test.rs"]
mod tests;
