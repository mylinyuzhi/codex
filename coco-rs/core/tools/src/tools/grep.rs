//! Grep tool — regex-based content search backed by the ripgrep core libraries.
//!
//! # Safety & concurrency model
//!
//! This tool sets the following flags on the [`Tool`] trait, matching the
//! Claude Code TypeScript `GrepTool` exactly:
//!
//! - `is_read_only(_) = true` — the tool only reads files and emits text.
//! - `is_concurrency_safe(_) = true` — two Grep calls may execute in parallel
//!   without interference. There is no shared mutable state; each call owns
//!   its own [`GrepSearchParams`] and sink buffer.
//! - `is_destructive` uses the default (`false`).
//! - `interrupt_behavior` uses the default ([`InterruptBehavior::Block`]).
//! - `requires_user_interaction` uses the default — the tool never triggers
//!   permission prompts, so headless/background execution is safe.
//!
//! # Execution pipeline
//!
//! The `StreamingToolExecutor` batches safe, read-only tools into a
//! `ConcurrentSafe` batch and dispatches each via `tokio::spawn`. Inside this
//! tool, the CPU-bound walk + regex search runs inside
//! [`tokio::task::spawn_blocking`] so the async executor thread is not blocked.
//! A [`tokio::time::timeout`] wraps the blocking future to enforce the
//! 20-second (configurable via `COCO_GLOB_TIMEOUT_SECONDS`) budget.
//!
//! # Cancellation
//!
//! `ctx.cancel` is cloned into the blocking closure and checked once per file
//! between searcher invocations. Mid-file cancellation is not supported by
//! `grep-searcher`, so a very large file will finish before the worker yields;
//! in practice this is bounded by the same timeout. The async side is
//! additionally wrapped by `tokio::select!` in the executor, so the caller
//! observes cancellation promptly even if the blocking worker is still
//! winding down.
//!
//! # Worktree isolation
//!
//! `ctx.cwd_override` is honored when set (used by worktree-isolated
//! subagents). Relative `path` arguments are joined against that base.

use coco_file_ignore::IgnoreConfig;
use coco_file_ignore::IgnoreService;
use coco_messages::ToolResult;
use coco_tool_runtime::DescriptionOptions;
use coco_tool_runtime::SearchReadInfo;
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolError;
use coco_tool_runtime::ToolResultContentPart;
use coco_tool_runtime::ToolUseContext;
use coco_tool_runtime::ValidationResult;
use coco_types::ToolId;
use coco_types::ToolName;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::BinaryDetection;
use grep_searcher::Searcher;
use grep_searcher::SearcherBuilder;
use grep_searcher::Sink;
use grep_searcher::SinkContext;
use grep_searcher::SinkMatch;
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use tokio_util::sync::CancellationToken;

use crate::input_types::GrepOutputMode;

/// Default head_limit when unspecified (TS: DEFAULT_HEAD_LIMIT = 250).
const DEFAULT_HEAD_LIMIT: usize = 250;

/// Maximum column width for content lines (TS: --max-columns 500).
const MAX_COLUMN_WIDTH: usize = 500;

/// Absolute cap on in-memory matches to avoid unbounded memory usage.
const MAX_IN_MEMORY_MATCHES: usize = 100_000;

/// Tool description shown to the model. Byte-for-byte copy of TS Claude Code
/// `tools/GrepTool/prompt.ts::getDescription()`.
const GREP_DESCRIPTION: &str = "\
A powerful search tool built on ripgrep

  Usage:
  - ALWAYS use Grep for search tasks. NEVER invoke `grep` or `rg` as a Bash command. The Grep tool has been optimized for correct permissions and access.
  - Supports full regex syntax (e.g., \"log.*Error\", \"function\\s+\\w+\")
  - Filter files with glob parameter (e.g., \"*.js\", \"**/*.tsx\") or type parameter (e.g., \"js\", \"py\", \"rust\")
  - Output modes: \"content\" shows matching lines, \"files_with_matches\" shows only file paths (default), \"count\" shows match counts
  - Use Agent tool for open-ended searches requiring multiple rounds
  - Pattern syntax: Uses ripgrep (not grep) - literal braces need escaping (use `interface\\{\\}` to find `interface{}` in Go code)
  - Multiline matching: By default patterns match within single lines only. For cross-line patterns like `struct \\{[\\s\\S]*?field`, use `multiline: true`
";

// ---------------------------------------------------------------------------
// Structured match data (from cocode-rs ContextAwareSink pattern)
// ---------------------------------------------------------------------------

/// A single entry from grep search: a match line, context line, or group break.
///
/// `file_path` is an `Arc<str>` so repeated matches from the same file share a
/// single allocation — cloning only bumps a refcount. Context break sentinels
/// reuse a shared empty `Arc<str>` and are detected via `is_break`.
#[derive(Debug, Clone)]
struct GrepMatchLine {
    file_path: Arc<str>,
    line_number: u64,
    line_content: String,
    is_context: bool,
    /// Sentinel: true means this is a `--` separator between non-contiguous
    /// context groups within the same file.
    is_break: bool,
}

/// Sink that collects match lines, context lines, and group breaks per file.
struct ContextAwareSink<'a> {
    matches: &'a mut Vec<GrepMatchLine>,
    file_path: Arc<str>,
    limit: usize,
}

impl Sink for ContextAwareSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, io::Error> {
        if self.matches.len() >= self.limit {
            return Ok(false);
        }
        self.matches.push(GrepMatchLine {
            file_path: Arc::clone(&self.file_path),
            line_number: mat.line_number().unwrap_or(0),
            line_content: decode_sink_bytes(mat.bytes()),
            is_context: false,
            is_break: false,
        });
        Ok(true)
    }

    fn context(&mut self, _searcher: &Searcher, ctx: &SinkContext<'_>) -> Result<bool, io::Error> {
        if self.matches.len() >= self.limit {
            return Ok(false);
        }
        self.matches.push(GrepMatchLine {
            file_path: Arc::clone(&self.file_path),
            line_number: ctx.line_number().unwrap_or(0),
            line_content: decode_sink_bytes(ctx.bytes()),
            is_context: true,
            is_break: false,
        });
        Ok(true)
    }

    fn context_break(&mut self, _searcher: &Searcher) -> Result<bool, io::Error> {
        if self.matches.len() >= self.limit {
            return Ok(false);
        }
        self.matches.push(GrepMatchLine {
            // Break sentinel carries no file — reuse the current file's Arc to
            // avoid allocating a new one (it's ignored by consumers anyway).
            file_path: Arc::clone(&self.file_path),
            line_number: 0,
            line_content: String::new(),
            is_context: false,
            is_break: true,
        });
        Ok(true)
    }
}

/// Decode sink bytes as UTF-8 (lossy), trim trailing newline/whitespace, and
/// cap at [`MAX_COLUMN_WIDTH`] (TS: `--max-columns 500`).
fn decode_sink_bytes(bytes: &[u8]) -> String {
    let raw = String::from_utf8_lossy(bytes);
    let trimmed = raw.trim_end();
    truncate_to_width(trimmed, MAX_COLUMN_WIDTH)
}

/// Truncate a string to a max character width, preserving char boundaries.
fn truncate_to_width(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // Find a char boundary at or before max
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}[...]", &s[..end])
}

// ---------------------------------------------------------------------------
// Search parameters (all owned, Send-safe for spawn_blocking)
// ---------------------------------------------------------------------------

struct GrepSearchParams {
    pattern: String,
    case_insensitive: bool,
    multiline: bool,
    before_context: usize,
    after_context: usize,
    search_path: PathBuf,
    glob_filter: Option<String>,
    type_filter: Option<String>,
    max_results: usize,
    /// Base directory for computing relative paths.
    base_dir: PathBuf,
}

/// Content-mode format options. Parsed from input at call time and
/// passed through to `format_content()` so the formatter can honor
/// `-n: false` (TS `GrepTool.ts:357-360`: when `show_line_numbers` is
/// false, the `-n` flag is omitted from ripgrep and output lines are
/// emitted without the line-number segment).
#[derive(Debug, Clone, Copy)]
struct ContentFormatOptions {
    show_line_numbers: bool,
}

impl Default for ContentFormatOptions {
    fn default() -> Self {
        Self {
            // TS default is `true` (`GrepTool.ts:68-70`
            // `'-n': semanticBoolean(z.boolean().optional()) ... Defaults
            // to true`).
            show_line_numbers: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Tool implementation
// ---------------------------------------------------------------------------

/// Typed input for [`GrepTool`].
///
/// Wire-shape preserves TS `GrepTool.ts` exactly: dashed flag names
/// `-A` / `-B` / `-C` / `-i` / `-n` come straight from the ripgrep CLI
/// vocabulary and round through `#[serde(rename)]`. The Rust idents
/// use descriptive snake_case (`before_context_short`, etc.).
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct GrepInput {
    /// The regular expression pattern to search for in file contents
    #[serde(default)]
    pub pattern: String,
    /// File or directory to search in (rg PATH). Defaults to current
    /// working directory.
    #[serde(default)]
    pub path: Option<String>,
    /// Glob pattern to filter files (e.g. "*.js", "*.{ts,tsx}") -
    /// maps to rg --glob
    #[serde(default)]
    pub glob: Option<String>,
    /// Output mode: "content" shows matching lines (supports
    /// -A/-B/-C context, -n line numbers, head_limit),
    /// "files_with_matches" shows file paths (supports head_limit),
    /// "count" shows match counts (supports head_limit). Defaults to
    /// "files_with_matches".
    #[serde(default)]
    pub output_mode: Option<GrepOutputMode>,
    /// Number of lines to show before each match (rg -B). Requires
    /// output_mode: "content", ignored otherwise.
    #[serde(default, rename = "-B")]
    pub before_context_short: Option<i64>,
    /// Number of lines to show after each match (rg -A). Requires
    /// output_mode: "content", ignored otherwise.
    #[serde(default, rename = "-A")]
    pub after_context_short: Option<i64>,
    /// Alias for context.
    #[serde(default, rename = "-C")]
    pub context_short: Option<i64>,
    /// Number of lines to show before and after each match (rg -C).
    /// Requires output_mode: "content", ignored otherwise.
    #[serde(default)]
    pub context: Option<i64>,
    /// Show line numbers in output (rg -n). Requires output_mode:
    /// "content", ignored otherwise. Defaults to true.
    #[serde(default, rename = "-n")]
    pub show_line_numbers: Option<bool>,
    /// Case insensitive search (rg -i)
    #[serde(default, rename = "-i")]
    pub case_insensitive: bool,
    /// File type to search (rg --type). Common types: js, py, rust,
    /// go, java, etc. More efficient than `glob` for standard file
    /// types. Wire key is `type`; Rust ident is `file_type` to avoid
    /// the keyword collision.
    #[serde(default, rename = "type")]
    pub file_type: Option<String>,
    /// Limit output to first N lines/entries, equivalent to "| head -N".
    /// Works across all output modes. Defaults to 250 when
    /// unspecified. Pass 0 for unlimited (use sparingly — large
    /// result sets waste context).
    #[serde(default)]
    pub head_limit: Option<i64>,
    /// Skip first N lines/entries before applying head_limit,
    /// equivalent to "| tail -n +N | head -N". Works across all
    /// output modes. Defaults to 0.
    #[serde(default)]
    pub offset: Option<i64>,
    /// Enable multiline mode where . matches newlines and patterns
    /// can span lines (rg -U --multiline-dotall). Default: false.
    #[serde(default)]
    pub multiline: bool,
}

/// Grep tool — content search using ripgrep core libraries.
pub struct GrepTool;

#[async_trait::async_trait]
impl Tool for GrepTool {
    type Input = GrepInput;
    coco_tool_runtime::impl_runtime_schema!(GrepInput);
    /// Output is the pre-formatted user-visible text (content /
    /// files_with_matches / count modes all build their final string
    /// inside `execute`). Rendered unwrapped so the model sees the
    /// raw lines without JSON escaping.
    type Output = String;

    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::Grep)
    }

    fn name(&self) -> &str {
        ToolName::Grep.as_str()
    }

    fn description(&self, _input: &GrepInput, _options: &DescriptionOptions) -> String {
        GREP_DESCRIPTION.into()
    }

    /// Grep never modifies state (TS: `isReadOnly() = true`).
    fn is_read_only(&self, _input: &GrepInput) -> bool {
        true
    }
    fn is_always_read_only(&self) -> bool {
        true
    }

    /// Safe to run in parallel with other concurrency-safe tools. The
    /// `StreamingToolExecutor` batches consecutive safe tools and dispatches
    /// them via `tokio::spawn` up to `COCO_MAX_TOOL_USE_CONCURRENCY`
    /// (default 10). TS: `isConcurrencySafe() = true`.
    fn is_concurrency_safe(&self, _input: &GrepInput) -> bool {
        true
    }

    /// Result persistence threshold — matches TS `maxResultSizeChars: 20_000`.
    fn max_result_size_bound(&self) -> coco_tool_runtime::ResultSizeBound {
        coco_tool_runtime::ResultSizeBound::Chars(20_000)
    }

    /// `Self::Output = String` — emit unwrapped (no JSON escape).
    /// TS parity: `GrepTool.ts::mapToolResultToToolResultBlockParam`.
    fn render_for_model(&self, out: &String) -> Vec<ToolResultContentPart> {
        vec![ToolResultContentPart::Text {
            text: out.clone(),
            provider_options: None,
        }]
    }

    fn get_activity_description(&self, input: &GrepInput) -> Option<String> {
        if input.pattern.is_empty() {
            return None;
        }
        Some(format!("Searching for {pattern}", pattern = input.pattern))
    }

    fn is_search_or_read_command(&self, _input: &GrepInput) -> Option<SearchReadInfo> {
        Some(SearchReadInfo {
            is_search: true,
            ..SearchReadInfo::default()
        })
    }

    /// R6-T20: refuse to search a root the user has marked as ignored.
    /// Individual files under the root are filtered during the walk by
    /// `is_read_ignored_with_matcher` inside `search_one_file`.
    async fn check_permissions(
        &self,
        input: &GrepInput,
        ctx: &ToolUseContext,
    ) -> coco_types::ToolCheckResult {
        let Some(path) = input.path.as_deref() else {
            return coco_types::ToolCheckResult::Passthrough;
        };
        let matcher = crate::tools::read_permissions::file_read_ignore_matcher_from_patterns(
            &ctx.tool_config.file_read_ignore_patterns,
        );
        crate::tools::read_permissions::check_read_permission_with_matcher(
            Path::new(path),
            &matcher,
            ctx,
        )
    }

    fn validate_input(&self, input: &GrepInput, _ctx: &ToolUseContext) -> ValidationResult {
        if input.pattern.is_empty() {
            return ValidationResult::invalid("missing required field: pattern");
        }
        ValidationResult::Valid
    }

    async fn execute(
        &self,
        input: GrepInput,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<String>, ToolError> {
        if input.pattern.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "missing pattern".into(),
                error_code: None,
            });
        }

        // Resolve the working directory. Worktree-isolated agents set
        // `ctx.cwd_override`; otherwise we fall back to the process CWD.
        // Relative `path` arguments are resolved against this base.
        let cwd = ctx
            .cwd_override
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/"));

        let search_path = match input.path.as_deref() {
            Some(p) => {
                let path = Path::new(p);
                if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    cwd.join(p)
                }
            }
            None => cwd.clone(),
        };

        if !search_path.exists() {
            return Err(ToolError::ExecutionFailed {
                message: format!("search path does not exist: {}", search_path.display()),
                display_data: None,
                source: None,
            });
        }

        let output_mode = input
            .output_mode
            .unwrap_or(GrepOutputMode::FilesWithMatches);

        let case_insensitive = input.case_insensitive;
        let multiline = input.multiline;

        // Context precedence (TS): context > -C > separate -B/-A
        let context_both = input
            .context
            .or(input.context_short)
            .filter(|n| *n >= 0)
            .map(|n| n as usize);
        let before_context = context_both
            .or_else(|| {
                input
                    .before_context_short
                    .filter(|n| *n >= 0)
                    .map(|n| n as usize)
            })
            .unwrap_or(0);
        let after_context = context_both
            .or_else(|| {
                input
                    .after_context_short
                    .filter(|n| *n >= 0)
                    .map(|n| n as usize)
            })
            .unwrap_or(0);

        // head_limit: None→250, Some(0)→unlimited
        let effective_limit = match input.head_limit {
            Some(0) => usize::MAX,
            Some(n) if n > 0 => n as usize,
            _ => DEFAULT_HEAD_LIMIT,
        };

        let offset = input
            .offset
            .filter(|n| *n > 0)
            .map(|n| n as usize)
            .unwrap_or(0);

        // TS `GrepTool.ts:68` `-n: semanticBoolean(z.boolean().optional())`
        // defaults to `true`. Passing `-n: false` suppresses line numbers
        // in content-mode output. R5-T13.
        let show_line_numbers = input.show_line_numbers.unwrap_or(true);
        let content_opts = ContentFormatOptions { show_line_numbers };

        let glob_filter = input.glob.clone();
        let type_filter = input.file_type.clone();
        let pattern = input.pattern.clone();

        let params = GrepSearchParams {
            pattern,
            case_insensitive,
            multiline,
            before_context,
            after_context,
            search_path,
            glob_filter,
            type_filter,
            // Cap at MAX_IN_MEMORY_MATCHES (not the user's limit) so the format
            // phase can see when the user's limit was exceeded and emit a
            // truncation footer. For typical usage MAX_IN_MEMORY_MATCHES is
            // orders of magnitude larger than head_limit.
            max_results: MAX_IN_MEMORY_MATCHES,
            base_dir: cwd,
        };

        let timeout_secs = ctx.tool_config.glob_timeout_seconds.max(1) as u64;

        let cancel = ctx.cancel.clone();
        let read_ignore_matcher =
            crate::tools::read_permissions::file_read_ignore_matcher_from_patterns(
                &ctx.tool_config.file_read_ignore_patterns,
            );
        let search_future = tokio::task::spawn_blocking(move || {
            run_grep_search(&params, &cancel, &read_ignore_matcher)
        });

        let result = tokio::time::timeout(Duration::from_secs(timeout_secs), search_future)
            .await
            .map_err(|_| ToolError::Timeout {
                timeout_ms: (timeout_secs * 1000) as i64,
            })?
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("grep search task failed: {e}"),
                display_data: None,
                source: None,
            })?
            .map_err(|e| ToolError::InvalidInput {
                message: e,
                error_code: None,
            })?;

        let result_text =
            format_grep_output(&result, output_mode, offset, effective_limit, content_opts);

        Ok(ToolResult {
            data: result_text,
            new_messages: vec![],
            app_state_patch: None,
            permission_updates: Vec::new(),
            display_data: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Core search (synchronous, runs inside spawn_blocking)
// ---------------------------------------------------------------------------

/// Result of a grep search: matches + auxiliary mtimes for files_with_matches sort.
struct GrepSearchResult {
    matches: Vec<GrepMatchLine>,
    /// Map from relative file path → mtime. Populated for each file that produced
    /// at least one match. Used by [`format_files_with_matches`] to sort without
    /// additional filesystem I/O in the async context.
    file_mtimes: HashMap<Arc<str>, SystemTime>,
}

fn run_grep_search(
    params: &GrepSearchParams,
    cancel: &CancellationToken,
    read_ignore_matcher: &globset::GlobSet,
) -> Result<GrepSearchResult, String> {
    // Build regex matcher via grep-regex (ripgrep's core library)
    let mut matcher_builder = RegexMatcherBuilder::new();
    matcher_builder.case_insensitive(params.case_insensitive);
    if params.multiline {
        matcher_builder.multi_line(true).dot_matches_new_line(true);
    }
    let matcher = matcher_builder
        .build(&params.pattern)
        .map_err(|e| format!("invalid regex pattern: {e}"))?;

    // Build searcher with binary detection and context support
    let mut searcher_builder = SearcherBuilder::new();
    searcher_builder
        .line_number(true)
        .binary_detection(BinaryDetection::quit(0));
    if params.before_context > 0 {
        searcher_builder.before_context(params.before_context);
    }
    if params.after_context > 0 {
        searcher_builder.after_context(params.after_context);
    }
    if params.multiline {
        searcher_builder.multi_line(true);
    }

    let mut matches: Vec<GrepMatchLine> = Vec::new();
    let mut file_mtimes: HashMap<Arc<str>, SystemTime> = HashMap::new();

    if params.search_path.is_file() {
        search_one_file(
            &params.search_path,
            &params.base_dir,
            &matcher,
            &searcher_builder,
            params.max_results,
            &mut matches,
            &mut file_mtimes,
        );
    } else {
        let walker_builder = build_directory_walker(params);
        for entry in walker_builder.build().flatten() {
            if cancel.is_cancelled() || matches.len() >= params.max_results {
                break;
            }
            if entry.file_type().is_none_or(|t| !t.is_file()) {
                continue;
            }

            // R6-T20: skip files matching file-read ignore patterns.
            // TS `GrepTool.ts:412-427` passes these patterns to ripgrep
            // via `--glob '!...'`; coco-rs filters them per-entry here
            // so we don't have to round-trip through the globset/walker
            // override system.
            if crate::tools::read_permissions::is_read_ignored_with_matcher(
                entry.path(),
                read_ignore_matcher,
            ) {
                continue;
            }

            search_one_file(
                entry.path(),
                &params.base_dir,
                &matcher,
                &searcher_builder,
                params.max_results,
                &mut matches,
                &mut file_mtimes,
            );
        }
    }

    Ok(GrepSearchResult {
        matches,
        file_mtimes,
    })
}

/// Search a single file and append match/context lines to `matches`.
/// Records mtime the first time the file produces a real match (used by
/// [`format_files_with_matches`] for sorting).
fn search_one_file(
    abs_path: &Path,
    base_dir: &Path,
    matcher: &grep_regex::RegexMatcher,
    searcher_builder: &SearcherBuilder,
    max_results: usize,
    matches: &mut Vec<GrepMatchLine>,
    file_mtimes: &mut HashMap<Arc<str>, SystemTime>,
) {
    let rel_path: Arc<str> = Arc::from(
        abs_path
            .strip_prefix(base_dir)
            .unwrap_or(abs_path)
            .to_string_lossy()
            .as_ref(),
    );

    let before_len = matches.len();
    let mut searcher = searcher_builder.build();
    let mut sink = ContextAwareSink {
        matches,
        file_path: Arc::clone(&rel_path),
        limit: max_results,
    };
    // Silently skip unreadable/binary files
    let _ = searcher.search_path(matcher, abs_path, &mut sink);

    // If this file produced any real matches, record its mtime for the sort.
    let produced_match = matches[before_len..]
        .iter()
        .any(|m| !m.is_context && !m.is_break);
    if produced_match {
        file_mtimes.entry(rel_path).or_insert_with(|| {
            abs_path
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        });
    }
}

/// Build the directory walker with VCS exclusion and type/glob filter applied.
fn build_directory_walker(params: &GrepSearchParams) -> ignore::WalkBuilder {
    let ignore_config = IgnoreConfig::default().with_hidden(true);
    let ignore_service = IgnoreService::new(ignore_config);
    let mut walker_builder = ignore_service.create_walk_builder(&params.search_path);

    // Exclude VCS directories (TS: --glob !.git etc.)
    const VCS_EXCLUDES: &[&str] = &["!.git", "!.svn", "!.hg", "!.bzr", "!.jj", "!.sl"];
    let mut override_builder = ignore::overrides::OverrideBuilder::new(&params.search_path);
    let all_added = VCS_EXCLUDES
        .iter()
        .all(|pat| override_builder.add(pat).is_ok());
    if all_added && let Ok(built) = override_builder.build() {
        walker_builder.overrides(built);
    }

    // Apply type filter or glob filter
    if let Some(ref type_name) = params.type_filter {
        let mut types_builder = ignore::types::TypesBuilder::new();
        types_builder.add_defaults();
        types_builder.select(type_name);
        if let Ok(types) = types_builder.build() {
            walker_builder.types(types);
        }
    } else if let Some(ref glob_pat) = params.glob_filter {
        let mut types_builder = ignore::types::TypesBuilder::new();
        for pat in split_glob_pattern(glob_pat) {
            let _ = types_builder.add("custom", &pat);
        }
        types_builder.select("custom");
        if let Ok(types) = types_builder.build() {
            walker_builder.types(types);
        }
    }

    walker_builder
}

/// Split a glob filter string into individual patterns, matching TS GrepTool
/// exactly: first split on whitespace, then for each whitespace-segment, if
/// it contains a `{...}` brace expression keep it intact, otherwise split
/// further on commas. This lets users pass combined filters like
/// `"*.js *.ts"`, `"*.js,*.ts"`, or `"*.{js,ts}"`.
fn split_glob_pattern(pattern: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for segment in pattern.split_whitespace() {
        if segment.contains('{') && segment.contains('}') {
            out.push(segment.to_string());
        } else {
            out.extend(
                segment
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from),
            );
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Output formatting (TS-compatible flat format)
//
// Per Claude Code TS GrepTool.mapToolResultToToolResultBlockParam:
//   • files_with_matches: "Found N file(s){limit_info}\npath1\npath2..." /
//                         "No files found" when empty
//   • content:            bare ripgrep lines (path:lineno:content) joined by \n,
//                         optional trailing "\n\n[Showing results with
//                         pagination = limit: X, offset: Y]" block
//   • count:              path:count lines + "\n\nFound N total
//                         occurrence(s) across M file(s).{ with pagination = …}"
// ---------------------------------------------------------------------------

fn format_grep_output(
    result: &GrepSearchResult,
    output_mode: GrepOutputMode,
    offset: usize,
    effective_limit: usize,
    content_opts: ContentFormatOptions,
) -> String {
    // Empty result short-circuit for `files_with_matches`. For `content` and
    // `count` the per-mode formatters handle emptiness themselves so that TS
    // semantics (e.g. count mode appends "Found 0 total occurrences across 0
    // files." even with no matches) are preserved.
    if result.matches.is_empty() && matches!(output_mode, GrepOutputMode::FilesWithMatches) {
        return "No files found".to_string();
    }

    match output_mode {
        GrepOutputMode::FilesWithMatches => {
            format_files_with_matches(result, offset, effective_limit)
        }
        GrepOutputMode::Content => {
            format_content(&result.matches, offset, effective_limit, content_opts)
        }
        GrepOutputMode::Count => format_count(&result.matches, offset, effective_limit),
    }
}

/// Format the comma-joined pagination hint. Empty when not truncated and
/// `offset == 0`, matching TS [`formatLimitInfo`].
fn format_limit_info(applied_limit: Option<usize>, applied_offset: usize) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(2);
    if let Some(limit) = applied_limit {
        parts.push(format!("limit: {limit}"));
    }
    if applied_offset > 0 {
        parts.push(format!("offset: {applied_offset}"));
    }
    parts.join(", ")
}

fn format_files_with_matches(
    result: &GrepSearchResult,
    offset: usize,
    effective_limit: usize,
) -> String {
    // Collect unique matched files in discovery order, skipping context/break lines.
    let mut seen: HashSet<&str> = HashSet::new();
    let mut unique_paths: Vec<&str> = Vec::new();
    for m in result
        .matches
        .iter()
        .filter(|m| !m.is_context && !m.is_break)
    {
        let path: &str = &m.file_path;
        if seen.insert(path) {
            unique_paths.push(path);
        }
    }

    // Sort newest first. mtimes were captured during the walk (in
    // spawn_blocking), so this closure does NO filesystem I/O. Falls back to
    // lexicographic tiebreaker for deterministic output when mtimes collide.
    unique_paths.sort_by(|a, b| {
        let a_time = result.file_mtimes.get(*a).copied();
        let b_time = result.file_mtimes.get(*b).copied();
        b_time.cmp(&a_time).then_with(|| a.cmp(b))
    });

    let after_offset: Vec<&str> = unique_paths.into_iter().skip(offset).collect();
    let was_truncated = after_offset.len() > effective_limit;
    let display: Vec<&str> = after_offset.into_iter().take(effective_limit).collect();

    if display.is_empty() {
        return "No files found".to_string();
    }

    let count = display.len();
    let file_word = if count == 1 { "file" } else { "files" };
    let applied_limit = was_truncated.then_some(effective_limit);
    let limit_info = format_limit_info(applied_limit, offset);

    let header = if limit_info.is_empty() {
        format!("Found {count} {file_word}")
    } else {
        format!("Found {count} {file_word} {limit_info}")
    };
    format!("{header}\n{}", display.join("\n"))
}

fn format_content(
    matches: &[GrepMatchLine],
    offset: usize,
    effective_limit: usize,
    opts: ContentFormatOptions,
) -> String {
    // Build output lines in TS flat format. When `-n: true` (default) the
    // format is `path:linenum:content` / `path-linenum-content`. When
    // `-n: false` the line number segment is dropped entirely, yielding
    // `path:content` / `path-content`. Context breaks are `--` in both
    // cases. Matches TS `GrepTool.ts:357-360` which only appends `-n` to
    // ripgrep's args when `show_line_numbers` is true.
    let mut lines: Vec<String> = Vec::with_capacity(matches.len());
    for m in matches {
        if m.is_break {
            lines.push("--".to_string());
        } else {
            let sep = if m.is_context { '-' } else { ':' };
            if opts.show_line_numbers {
                lines.push(format!(
                    "{}{sep}{}{sep}{}",
                    m.file_path, m.line_number, m.line_content
                ));
            } else {
                lines.push(format!("{}{sep}{}", m.file_path, m.line_content));
            }
        }
    }

    let after_offset: Vec<String> = lines.into_iter().skip(offset).collect();
    let was_truncated = after_offset.len() > effective_limit;
    let display: Vec<String> = after_offset.into_iter().take(effective_limit).collect();

    let applied_limit = was_truncated.then_some(effective_limit);
    let limit_info = format_limit_info(applied_limit, offset);

    // TS parity: if body is empty, substitute the literal "No matches found"
    // and still append the pagination block if applicable (e.g. offset > 0).
    // See TS GrepTool.ts lines 267-277.
    let body = if display.is_empty() {
        "No matches found".to_string()
    } else {
        display.join("\n")
    };

    if limit_info.is_empty() {
        body
    } else {
        format!("{body}\n\n[Showing results with pagination = {limit_info}]")
    }
}

fn format_count(matches: &[GrepMatchLine], offset: usize, effective_limit: usize) -> String {
    // Group actual matches by file path, preserving insertion order.
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for m in matches.iter().filter(|m| !m.is_context && !m.is_break) {
        let path: &str = &m.file_path;
        let count = counts.entry(path).or_insert(0);
        if *count == 0 {
            order.push(path);
        }
        *count += 1;
    }

    // Discover (file, count) pairs in insertion order, then slice by offset/limit.
    // Keeping the tuple avoids re-parsing counts out of formatted lines later.
    let after_offset: Vec<(&str, usize)> = order
        .into_iter()
        .filter_map(|f| counts.get(f).copied().map(|c| (f, c)))
        .skip(offset)
        .collect();
    let was_truncated = after_offset.len() > effective_limit;
    let display: Vec<(&str, usize)> = after_offset.into_iter().take(effective_limit).collect();

    let total_matches: usize = display.iter().map(|(_, n)| *n).sum();
    let num_files = display.len();

    // TS parity: empty count body uses the literal "No matches found" in
    // place of the file list, and the summary is still appended. See TS
    // GrepTool.ts lines 280-291.
    let body = if display.is_empty() {
        "No matches found".to_string()
    } else {
        display
            .iter()
            .map(|(f, n)| format!("{f}:{n}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let occurrence_word = if total_matches == 1 {
        "occurrence"
    } else {
        "occurrences"
    };
    let file_word = if num_files == 1 { "file" } else { "files" };

    let applied_limit = was_truncated.then_some(effective_limit);
    let limit_info = format_limit_info(applied_limit, offset);
    let summary = format!(
        "\n\nFound {total_matches} total {occurrence_word} across {num_files} {file_word}."
    );
    if limit_info.is_empty() {
        format!("{body}{summary}")
    } else {
        format!("{body}{summary} with pagination = {limit_info}")
    }
}

#[cfg(test)]
#[path = "grep.test.rs"]
mod tests;
