use coco_tool::DescriptionOptions;
use coco_tool::SideQueryRequest;
use coco_tool::Tool;
use coco_tool::ToolError;
use coco_tool::ToolUseContext;
use coco_types::ToolId;
use coco_types::ToolInputSchema;
use coco_types::ToolName;
use coco_types::ToolResult;
use serde_json::Value;
use std::collections::HashMap;

/// Maximum markdown length passed to the extraction side-query.
///
/// TS: `WebFetchTool/utils.ts:128` `MAX_MARKDOWN_LENGTH = 100_000`.
/// We truncate the html2text output to this size before handing it to
/// the extraction model to keep side-query token cost bounded.
const MAX_FETCH_LENGTH: usize = 100_000;

/// Max width in columns for html2text's line wrapping.
///
/// TS: `WebFetchTool/utils.ts:20` sets `maxLineWidth: 120`. Matches TS
/// exactly so the extracted markdown renders consistently across both
/// implementations.
const HTML2TEXT_LINE_WIDTH: usize = 120;

/// System prompt for the WebFetch extraction side-query.
///
/// TS: `WebFetchTool/utils.ts:498-502`. The secondary model (Haiku in TS,
/// whatever the user configured as side_query in coco-rs) receives the
/// markdown body + user's question and extracts the relevant answer.
/// Keeping this byte-compatible lets us produce similar output quality.
const WEB_FETCH_EXTRACT_SYSTEM: &str = "\
You are a helpful assistant extracting answers from web page content. \
The user will provide a web page (in markdown format) followed by a \
specific prompt. Your job is to answer the prompt using ONLY information \
from the provided content. \
If the content does not contain enough information to answer the prompt, \
say so clearly rather than guessing. Be concise.";

/// Max HTTP response body size. TS: `WebFetchTool/utils.ts:112`
/// `MAX_HTTP_CONTENT_LENGTH = 10 * 1024 * 1024` (10 MB). Prevents a
/// misbehaving server from flooding memory via a huge response.
const MAX_HTTP_CONTENT_LENGTH: u64 = 10 * 1024 * 1024;

/// Fetch timeout for the primary request. TS: `utils.ts:116`
/// `FETCH_TIMEOUT_MS = 60_000`. Long enough for slow origin servers,
/// short enough that the model doesn't stall forever on a stuck fetch.
const WEB_FETCH_TIMEOUT_SECS: u64 = 60;

/// WebFetch URL cache TTL: 15 minutes. Matches TS
/// `WebFetchTool/utils.ts:63` `CACHE_TTL_MS = 15 * 60 * 1000`.
const WEB_FETCH_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// WebFetch URL cache max entries. TS uses a 50MB size budget; we use
/// a simpler entry count because Rust's approximation of JS heap
/// accounting is non-trivial. 128 entries at up to 100K chars each
/// gives ~12.8MB upper bound on cache content, well under TS's 50MB.
const WEB_FETCH_CACHE_MAX_ENTRIES: usize = 128;

/// Cached WebFetch entry. Stores the extracted markdown + its freshness
/// timestamp. Shape matches the subset of TS `CacheEntry` that we
/// actually use for the return value.
#[derive(Clone)]
struct CachedWebFetch {
    /// Extracted markdown (after turndown conversion).
    markdown: String,
    /// Content-Type from the upstream response.
    content_type: String,
    /// Whether the markdown was truncated at MAX_FETCH_LENGTH.
    was_truncated: bool,
    /// When the entry was inserted — used for TTL check.
    inserted_at: std::time::Instant,
}

/// In-process WebFetch URL cache. Parallel to the `SEARCH_CACHE` used
/// by WebSearch — same `LazyLock<Mutex<Vec<...>>>` LRU pattern.
/// Session-scoped only.
static WEB_FETCH_CACHE: std::sync::LazyLock<std::sync::Mutex<Vec<(String, CachedWebFetch)>>> =
    std::sync::LazyLock::new(|| {
        std::sync::Mutex::new(Vec::with_capacity(WEB_FETCH_CACHE_MAX_ENTRIES))
    });

fn web_fetch_cache_get(url: &str) -> Option<CachedWebFetch> {
    let mut cache = WEB_FETCH_CACHE.lock().ok()?;
    // Expire old entries on every access (cheap since the cache is small).
    let now = std::time::Instant::now();
    cache.retain(|(_, entry)| now.duration_since(entry.inserted_at) < WEB_FETCH_CACHE_TTL);
    cache
        .iter()
        .find_map(|(k, v)| if k == url { Some(v.clone()) } else { None })
}

fn web_fetch_cache_set(url: String, entry: CachedWebFetch) {
    if let Ok(mut cache) = WEB_FETCH_CACHE.lock() {
        // LRU eviction: if at capacity, drop the oldest entry.
        if cache.len() >= WEB_FETCH_CACHE_MAX_ENTRIES {
            cache.remove(0);
        }
        // Deduplicate: if an entry for the same URL exists, remove it
        // so the refreshed entry is the newest.
        cache.retain(|(k, _)| k != &url);
        cache.push((url, entry));
    }
}

/// Test-only cache reset. Isolates cached fixtures between tests.
#[cfg(test)]
pub(super) fn clear_web_fetch_cache() {
    if let Ok(mut cache) = WEB_FETCH_CACHE.lock() {
        cache.clear();
    }
}

/// Custom User-Agent header.
///
/// TS: `utils/http.ts:56-58` `getWebFetchUserAgent()` returns
/// `` `Claude-User (${getClaudeCodeUserAgent()}; +https://support.anthropic.com/)` ``
/// where `getClaudeCodeUserAgent()` at `utils/userAgent.ts:9` returns
/// `` `claude-code/${MACRO.VERSION}` ``. The `Claude-User` prefix is
/// Anthropic's publicly documented agent for user-initiated fetches —
/// site operators match this in robots.txt to distinguish CLI traffic
/// from server-side automation.
///
/// We mirror the exact format so robots.txt rules targeting
/// `Claude-User` apply identically to coco-rs fetches. The version
/// string uses the coco-rs release version rather than the TS
/// claude-code version, which is fine because the prefix is what
/// site operators match against.
const WEB_FETCH_USER_AGENT: &str =
    "Claude-User (claude-code/coco-rs; +https://support.anthropic.com/)";

/// Preapproved hosts — URLs whose (hostname, pathname) match this list
/// skip the optional permission gate. Byte-for-byte port of TS
/// `tools/WebFetchTool/preapproved.ts:14-131`.
///
/// **Matching semantics** (exactly TS, stricter than a suffix match):
///
/// - Entries without `/` are **exact hostname match only**. `docs.python.org`
///   does NOT match `subdomain.docs.python.org` — we only allow the entry
///   itself. This is intentional: some entries (e.g. `nuget.org`,
///   `huggingface.co`, `www.kaggle.com`) allow user uploads, and allowing
///   arbitrary subdomains could enable data exfiltration to attacker-
///   controlled subdomains.
///
/// - Entries containing `/` are **host + path-prefix** entries. For example
///   `github.com/anthropics` matches `https://github.com/anthropics` and
///   `https://github.com/anthropics/claude-code` but NOT
///   `https://github.com/anthropics-evil/malware`. Segment boundary is
///   enforced (exact match or `prefix + /`).
///
/// TS code comment at `preapproved.ts:1-12` explains the security tradeoff:
/// these entries are for WebFetch GET only — the sandbox does NOT inherit
/// this list for network restrictions.
///
/// TODO(B2.6 follow-up): wire `is_preapproved_host` into the permission
/// evaluator at the query-engine layer so matching hosts bypass the
/// approval UI. Until that wiring lands, this is read only by tests —
/// the `#[allow(dead_code)]` marker is intentional.
#[allow(dead_code)]
const PREAPPROVED_WEB_HOSTS: &[&str] = &[
    // Anthropic (5)
    "platform.claude.com",
    "code.claude.com",
    "modelcontextprotocol.io",
    "github.com/anthropics",
    "agentskills.io",
    // Top Programming Languages (13)
    "docs.python.org",
    "en.cppreference.com",
    "docs.oracle.com",
    "learn.microsoft.com",
    "developer.mozilla.org",
    "go.dev",
    "pkg.go.dev",
    "www.php.net",
    "docs.swift.org",
    "kotlinlang.org",
    "ruby-doc.org",
    "doc.rust-lang.org",
    "www.typescriptlang.org",
    // Web & JavaScript Frameworks/Libraries (16)
    "react.dev",
    "angular.io",
    "vuejs.org",
    "nextjs.org",
    "expressjs.com",
    "nodejs.org",
    "bun.sh",
    "jquery.com",
    "getbootstrap.com",
    "tailwindcss.com",
    "d3js.org",
    "threejs.org",
    "redux.js.org",
    "webpack.js.org",
    "jestjs.io",
    "reactrouter.com",
    // Python Frameworks & Libraries (11)
    "docs.djangoproject.com",
    "flask.palletsprojects.com",
    "fastapi.tiangolo.com",
    "pandas.pydata.org",
    "numpy.org",
    "www.tensorflow.org",
    "pytorch.org",
    "scikit-learn.org",
    "matplotlib.org",
    "requests.readthedocs.io",
    "jupyter.org",
    // PHP Frameworks (3)
    "laravel.com",
    "symfony.com",
    "wordpress.org",
    // Java Frameworks & Libraries (5)
    "docs.spring.io",
    "hibernate.org",
    "tomcat.apache.org",
    "gradle.org",
    "maven.apache.org",
    // .NET & C# Frameworks (4)
    "asp.net",
    "dotnet.microsoft.com",
    "nuget.org",
    "blazor.net",
    // Mobile Development (4)
    "reactnative.dev",
    "docs.flutter.dev",
    "developer.apple.com",
    "developer.android.com",
    // Data Science & Machine Learning (4)
    "keras.io",
    "spark.apache.org",
    "huggingface.co",
    "www.kaggle.com",
    // Databases (7)
    "www.mongodb.com",
    "redis.io",
    "www.postgresql.org",
    "dev.mysql.com",
    "www.sqlite.org",
    "graphql.org",
    "prisma.io",
    // Cloud & DevOps (10)
    "docs.aws.amazon.com",
    "cloud.google.com",
    "kubernetes.io",
    "www.docker.com",
    "www.terraform.io",
    "www.ansible.com",
    "vercel.com/docs",
    "docs.netlify.com",
    "devcenter.heroku.com",
    // Testing & Monitoring (2)
    "cypress.io",
    "selenium.dev",
    // Game Development (2)
    "docs.unity.com",
    "docs.unrealengine.com",
    // Other Essential Tools (3)
    "git-scm.com",
    "nginx.org",
    "httpd.apache.org",
];

/// Check whether a `(hostname, pathname)` pair matches the preapproved
/// list. Implements the exact TS `preapproved.ts:154-166`
/// `isPreapprovedHost` semantics: exact hostname match via a set, plus
/// path-prefix matching with segment boundary enforcement.
///
/// `hostname` should be the URL's host (lowercased), `pathname` should
/// be the path portion starting with `/` (or empty for root).
#[allow(dead_code)]
pub(super) fn is_preapproved_host(hostname: &str, pathname: &str) -> bool {
    if hostname.is_empty() {
        return false;
    }
    let host_lower = hostname.to_lowercase();
    for entry in PREAPPROVED_WEB_HOSTS {
        match entry.split_once('/') {
            None => {
                // Hostname-only entry: exact match required.
                if host_lower == *entry {
                    return true;
                }
            }
            Some((host, path_rest)) => {
                // Path-scoped entry: host exact, path with segment boundary.
                if host_lower != host {
                    continue;
                }
                let path_prefix = format!("/{path_rest}");
                // Segment boundary: exact match OR prefix followed by '/'.
                // Prevents "/anthropics-evil/malware" from matching
                // "/anthropics".
                if pathname == path_prefix || pathname.starts_with(&format!("{path_prefix}/")) {
                    return true;
                }
            }
        }
    }
    false
}

/// Convenience wrapper that extracts hostname + pathname from a full
/// URL and calls [`is_preapproved_host`]. Used by tests and the future
/// permission-layer integration.
#[allow(dead_code)]
pub(super) fn is_preapproved_url(url: &str) -> bool {
    let host = extract_host(url);
    let pathname = extract_pathname(url);
    is_preapproved_host(&host, &pathname)
}

/// Extract the pathname portion of a URL (everything from the first `/`
/// after the host, up to `?` or `#`). Returns `"/"` when the URL has no
/// explicit path (bare host).
#[allow(dead_code)]
fn extract_pathname(url: &str) -> String {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    // Strip userinfo if present (user:pass@host).
    let after_userinfo = without_scheme
        .split_once('@')
        .map(|(_, rest)| rest)
        .unwrap_or(without_scheme);
    // Find the path start.
    match after_userinfo.find('/') {
        Some(start) => {
            // Stop at query/fragment.
            let path = &after_userinfo[start..];
            let end = path.find(['?', '#']).unwrap_or(path.len());
            path[..end].to_string()
        }
        None => "/".to_string(),
    }
}

/// Redirect handling decision for the cross-origin SSRF guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RedirectDecision {
    /// No redirect (or same-origin redirect we can auto-follow).
    Allow,
    /// Cross-origin redirect — the model should re-fetch explicitly with
    /// the new URL. Prevents open-redirect exploitation of the fetcher.
    CrossOrigin { new_url: String },
}

/// Decide whether a redirect from `from_url` to `to_url` is safe to
/// auto-follow.
///
/// Byte-for-byte port of TS `WebFetchTool/utils.ts:212-243`
/// `isPermittedRedirect`. A redirect is only permitted when ALL FOUR
/// checks pass:
///
/// 1. **Protocol match** — `https://...` → `http://...` is blocked.
///    Otherwise an attacker could downgrade a TLS-secured fetch to
///    plaintext and MITM it.
///
/// 2. **Port match** — `example.com:443` → `example.com:9999` is blocked.
///    This is the SSRF bug the round-2 verification caught: without this
///    check, a malicious server could redirect to a non-default port on
///    the same host and coco-rs would follow it blindly.
///
/// 3. **No redirect userinfo** — `https://user:pass@example.com/`
///    shouldn't appear in a Location header; if it does, the server is
///    attempting credential injection.
///
/// 4. **Host equivalence** (after stripping `www.`) — allows
///    `example.com` ↔ `www.example.com` toggling, blocks subdomain
///    redirects like `example.com` → `docs.example.com`.
///
/// Any violation returns `CrossOrigin { new_url }` so the tool surfaces
/// a structured message asking the model to re-fetch with the new URL.
///
/// TS explicitly comments at `utils.ts:249-253`: "Do not automatically
/// follow redirects because following redirects could allow for an
/// attacker to exploit an open redirect vulnerability."
pub(super) fn check_redirect(from_url: &str, to_url: &str) -> RedirectDecision {
    let reject = || RedirectDecision::CrossOrigin {
        new_url: to_url.to_string(),
    };

    // Check 1: protocol must match. Extract scheme from both URLs.
    let from_scheme = extract_scheme(from_url);
    let to_scheme = extract_scheme(to_url);
    if from_scheme != to_scheme || from_scheme.is_empty() {
        return reject();
    }

    // Check 2: port must match. Compare normalized host:port tuples.
    let (from_host, from_port) = split_host_port(from_url, &from_scheme);
    let (to_host, to_port) = split_host_port(to_url, &to_scheme);
    if from_port != to_port {
        return reject();
    }

    // Check 3: redirect target must not carry userinfo (`user:pass@host`).
    // This catches credential-injection attempts via the Location header.
    if has_userinfo(to_url) {
        return reject();
    }

    // Check 4: host equivalence after stripping `www.`.
    if from_host.is_empty() || to_host.is_empty() {
        return reject();
    }
    let from_lower = from_host.to_lowercase();
    let to_lower = to_host.to_lowercase();
    let normalize = |h: &str| -> String { h.strip_prefix("www.").unwrap_or(h).to_string() };
    if normalize(&from_lower) == normalize(&to_lower) {
        return RedirectDecision::Allow;
    }

    reject()
}

/// Extract the scheme (e.g. `"https"`) from a URL. Empty string if
/// there's no explicit `scheme://` prefix.
pub(super) fn extract_scheme(url: &str) -> String {
    url.split_once("://")
        .map(|(scheme, _)| scheme.to_lowercase())
        .unwrap_or_default()
}

/// Split a URL into its host and explicit port (if any).
///
/// Returns `(host, port)` where port is `None` when not specified.
/// Port is derived from the URL text — we do NOT fall back to the
/// scheme's default (443/80), because two URLs `https://example.com`
/// and `https://example.com:443` should compare equal. We achieve this
/// by normalizing both to `None`.
///
/// # IPv6 literal handling (T1 fix)
///
/// Per RFC 3986, IPv6 addresses in URLs MUST be enclosed in brackets:
/// `http://[::1]:8080/`. The brackets are part of the host, and any
/// port comes AFTER the closing bracket. A naive `rsplit_once(':')`
/// would split on a colon inside the IPv6 address, producing a
/// malformed host like `[::` and a bogus port.
///
/// This function detects bracketed IPv6 explicitly:
/// - `[::1]` → host=`[::1]`, port=None
/// - `[::1]:8080` → host=`[::1]`, port=Some(8080)
/// - `[::1]:443` under `https` → host=`[::1]`, port=None (default)
///
/// Bare (unbracketed) IPv6 like `::1` is technically not a valid URL
/// host — we'll still try to parse it with the standard rsplit path
/// which will produce wrong results, but the input was malformed to
/// begin with.
///
/// # Port parse failure (T1 fix)
///
/// When the trailing `:N` suffix is all digits but fails to parse as
/// `u16` (e.g. `:99999`, over the 65535 cap), we strip the bad port
/// and return `port=None`. Previously we returned the whole
/// `host:99999` string as the host, causing silent corruption and
/// false host-comparison results.
pub(super) fn split_host_port(url: &str, scheme: &str) -> (String, Option<u16>) {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    // Strip any userinfo segment before we look for host:port.
    let after_userinfo = without_scheme
        .split_once('@')
        .map(|(_, rest)| rest)
        .unwrap_or(without_scheme);
    // Host portion stops at the first `/`, `?`, or `#`.
    let host_end = after_userinfo
        .find(['/', '?', '#'])
        .unwrap_or(after_userinfo.len());
    let host_with_port = &after_userinfo[..host_end];

    // Bracketed IPv6 literal fast path. The host is `[...]` (including
    // brackets) and any port comes after `]:`.
    let (raw_host, explicit_port) = if host_with_port.starts_with('[') {
        if let Some(bracket_end) = host_with_port.find(']') {
            // Host is everything up to and including the `]`.
            let host = &host_with_port[..=bracket_end];
            let after_bracket = &host_with_port[bracket_end + 1..];
            let port = if let Some(port_str) = after_bracket.strip_prefix(':') {
                // T1: on parse failure (e.g. port > 65535), return None
                // instead of keeping the bad suffix attached to the host.
                port_str.parse::<u16>().ok()
            } else {
                // No `:port` after the bracket — just the bracketed
                // host. Anything else after the bracket that isn't a
                // colon is malformed; we ignore it.
                None
            };
            (host, port)
        } else {
            // Unterminated `[` — malformed URL, pass through as-is.
            (host_with_port, None)
        }
    } else {
        // Non-bracketed host. Split on the LAST `:` for the port. We
        // only accept the split as a port if the text after `:` is
        // all ASCII digits — this prevents misinterpretation in bare
        // IPv6 or other colon-containing junk.
        match host_with_port.rsplit_once(':') {
            Some((h, port_str))
                if !port_str.is_empty() && port_str.bytes().all(|b| b.is_ascii_digit()) =>
            {
                // T1: if the all-digit suffix fails to parse as u16
                // (e.g. `:99999`, above the 65535 cap), the host is
                // still stripped back to `h` and port is None — NOT
                // host=`h:99999`. This avoids silent corruption.
                (h, port_str.parse::<u16>().ok())
            }
            // No `:port` suffix OR the text after the last `:` isn't
            // all digits (e.g. a `::` inside a bare IPv6 literal).
            Some(_) | None => (host_with_port, None),
        }
    };

    // Normalize explicit default-port to None so that `example.com` and
    // `example.com:443` under `https` compare equal.
    let normalized_port = match (scheme, explicit_port) {
        ("https", Some(443)) => None,
        ("http", Some(80)) => None,
        other => other.1,
    };

    (raw_host.to_string(), normalized_port)
}

/// Check whether a URL contains a userinfo segment like `user:pass@`.
///
/// TS at `utils.ts:228-230` rejects redirects with userinfo to prevent
/// credential-injection attacks via the Location header.
pub(super) fn has_userinfo(url: &str) -> bool {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    // Userinfo, if present, comes before the first `@` and before the
    // host path boundary. `@` after a `/` is in the path, not userinfo.
    let boundary = without_scheme
        .find(['/', '?', '#'])
        .unwrap_or(without_scheme.len());
    let authority = &without_scheme[..boundary];
    authority.contains('@')
}

pub struct WebFetchTool;

#[async_trait::async_trait]
impl Tool for WebFetchTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::WebFetch)
    }
    fn name(&self) -> &str {
        ToolName::WebFetch.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        // R7-T25: byte-aligned port of TS `WebFetchTool/prompt.ts:3-21`
        // `DESCRIPTION`. Includes the MCP-preference hint, the
        // 15-minute cache note, and the cross-origin redirect handling
        // guidance — all of which inform model behavior when the
        // fetch encounters edge cases.
        "
- Fetches content from a specified URL and processes it using an AI model
- Takes a URL and a prompt as input
- Fetches the URL content, converts HTML to markdown
- Processes the content with the prompt using a small, fast model
- Returns the model's response about the content
- Use this tool when you need to retrieve and analyze web content

Usage notes:
  - IMPORTANT: If an MCP-provided web fetch tool is available, prefer using that tool instead of this one, as it may have fewer restrictions.
  - The URL must be a fully-formed valid URL
  - HTTP URLs will be automatically upgraded to HTTPS
  - The prompt should describe what information you want to extract from the page
  - This tool is read-only and does not modify any files
  - Results may be summarized if the content is very large
  - Includes a self-cleaning 15-minute cache for faster responses when repeatedly accessing the same URL
  - When a URL redirects to a different host, the tool will inform you and provide the redirect URL in a special format. You should then make a new WebFetch request with the redirect URL to fetch the content.
  - For GitHub URLs, prefer using the gh CLI via Bash instead (e.g., gh pr view, gh issue view, gh api).
"
        .into()
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "url".into(),
            serde_json::json!({"type": "string", "description": "The URL to fetch content from"}),
        );
        p.insert(
            "prompt".into(),
            serde_json::json!({"type": "string", "description": "The prompt to run on the fetched content"}),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn get_activity_description(&self, input: &Value) -> Option<String> {
        let url = input.get("url").and_then(|v| v.as_str())?;
        let truncated: String = url.chars().take(47).collect();
        let display = if truncated.len() < url.len() {
            format!("Fetching {truncated}...")
        } else {
            format!("Fetching {url}")
        };
        Some(display)
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if url.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "url parameter is required".into(),
                error_code: None,
            });
        }

        // TS `WebFetchTool.ts:26` uses zod `.url()` which rejects malformed
        // input before reaching the network. coco-rs previously only checked
        // for emptiness, so a typo like "https//example.com" (missing the
        // colon) would tunnel into reqwest and surface as a confusing
        // `[NETWORK_ERROR]`. Validate up-front via reqwest's re-export of
        // the `url` crate so the failure is clear and synchronous.
        // Also enforces a scheme — `file://` and `data://` are caught here
        // because they're not http/https; matches TS behavior of rejecting
        // anything that doesn't look like a fetchable web URL.
        let parsed = reqwest::Url::parse(url).map_err(|e| ToolError::InvalidInput {
            message: format!("invalid url '{url}': {e}"),
            error_code: None,
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(ToolError::InvalidInput {
                message: format!(
                    "url must use http or https scheme, got '{}://'",
                    parsed.scheme()
                ),
                error_code: None,
            });
        }

        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();

        if prompt.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "prompt parameter is required".into(),
                error_code: None,
            });
        }

        // Stage 0: cache lookup. TS `WebFetchTool/utils.ts:356-366`
        // checks `URL_CACHE.get(url)` first and short-circuits if a
        // fresh entry exists. This prevents redundant network + LLM
        // extraction work when the model re-fetches the same URL within
        // 15 minutes (e.g. revisiting a docs page after a tool-use loop).
        //
        // Note: the cache key is the ORIGINAL url parameter, not the
        // post-redirect URL, matching TS behavior at `utils.ts:469`.
        let (extraction_input, was_truncated, content_type) =
            if let Some(cached) = web_fetch_cache_get(url) {
                // Cache hit: skip Stage 1+2+3 and use the stored markdown.
                tracing::debug!("WebFetch cache hit for {url}");
                (cached.markdown, cached.was_truncated, cached.content_type)
            } else {
                // Cache miss: run the full fetch → markdown → truncate pipeline.
                //
                // Stage 1: HTTP fetch. `fetch_url` returns one of:
                //   - `Body { body, content_type }` — normal 2xx response
                //   - `CrossOriginRedirect { new_url }` — the origin
                //     redirected somewhere outside the same-origin
                //     allowlist; we surface this to the model so it can
                //     decide whether to fetch the new URL. Matches TS
                //     `WebFetchTool.tsx:227-235` SSRF guard.
                let (body, content_type) = match fetch_url(url).await {
                    Ok(FetchOutcome::Body { body, content_type }) => (body, content_type),
                    Ok(FetchOutcome::CrossOriginRedirect { new_url }) => {
                        return Ok(ToolResult {
                            data: serde_json::json!({
                                "url": url,
                                "prompt": prompt,
                                "redirect_blocked": true,
                                "new_url": new_url,
                                "message": format!(
                                    "The URL redirected to a different origin ({new_url}). \
                                     For security, WebFetch does not automatically follow cross-\
                                     origin redirects. If you want the redirected content, issue \
                                     a new WebFetch call with url={new_url}."
                                ),
                            }),
                            new_messages: vec![],
                        });
                    }
                    Err(e) => {
                        return Err(ToolError::ExecutionFailed {
                            message: format!("Failed to fetch {url}: {e}"),
                            source: None,
                        });
                    }
                };

                // Stage 2: HTML → markdown. TS `WebFetchTool/utils.ts:85-97,
                // 456-457` lazy-loads `turndown` and reuses one shared
                // instance. We use the Rust `html2text` crate which does
                // the same HTML-to-wrapped-plain-text conversion. Plain-
                // text bodies (JSON, text/plain, unknown) bypass the
                // conversion and are passed through as-is.
                let markdown = if is_html_content_type(&content_type) {
                    html_to_markdown(&body)
                } else {
                    body
                };

                // Stage 3: truncate to the extraction budget (100K chars).
                // TS: `utils.ts:128` `MAX_MARKDOWN_LENGTH = 100_000`.
                let (extraction_input, was_truncated) = if markdown.len() > MAX_FETCH_LENGTH {
                    // char_indices-safe truncation to avoid splitting mid-UTF-8.
                    let cut = markdown
                        .char_indices()
                        .take(MAX_FETCH_LENGTH)
                        .last()
                        .map(|(i, c)| i + c.len_utf8())
                        .unwrap_or(markdown.len());
                    (markdown[..cut].to_string(), true)
                } else {
                    (markdown, false)
                };

                // Populate the cache so subsequent fetches of the same
                // URL within the 15-min TTL are zero-cost. We cache the
                // POST-truncation markdown so we don't waste bytes on
                // content we'd only throw away again.
                web_fetch_cache_set(
                    url.to_string(),
                    CachedWebFetch {
                        markdown: extraction_input.clone(),
                        content_type: content_type.clone(),
                        was_truncated,
                        inserted_at: std::time::Instant::now(),
                    },
                );

                (extraction_input, was_truncated, content_type)
            };

        // `content_type` is now bound regardless of cache path. Used by
        // downstream tracing; keep it referenced so the warning-free build
        // stays clean.
        let _ = content_type;

        // Stage 4: LLM extraction pass via side-query. TS `utils.ts:498-
        // 514` calls `queryHaiku` with the user's prompt + truncated
        // markdown, returning the extracted answer. In coco-rs we delegate
        // to `ctx.side_query` (implemented by the inference layer) so the
        // extraction works with whichever provider the user configured.
        //
        // When side_query is the `NoOpSideQuery` stub (e.g. unit tests),
        // the extraction call errors out. We fall back to returning the
        // markdown directly with the user prompt attached — this matches
        // the old pre-B2.5 behavior and keeps tests green.
        let user_message =
            format!("{prompt}\n\n---\n\nWeb page content (markdown):\n\n{extraction_input}");
        let request =
            SideQueryRequest::simple(WEB_FETCH_EXTRACT_SYSTEM, &user_message, "web_fetch_extract");

        let extracted = match ctx.side_query.query(request).await {
            Ok(response) => response.text.unwrap_or_default(),
            Err(e) => {
                tracing::debug!(
                    "WebFetch extraction side-query unavailable ({e}); returning raw markdown"
                );
                // Fallback: surface the raw markdown so the main model can
                // do the extraction itself. No information is lost.
                return Ok(ToolResult {
                    data: serde_json::json!({
                        "url": url,
                        "prompt": prompt,
                        "content": extraction_input,
                        "truncated": was_truncated,
                        "extraction_mode": "raw",
                    }),
                    new_messages: vec![],
                });
            }
        };

        Ok(ToolResult {
            data: serde_json::json!({
                "url": url,
                "prompt": prompt,
                "extracted": extracted,
                "truncated": was_truncated,
                "extraction_mode": "llm",
            }),
            new_messages: vec![],
        })
    }
}

/// Check whether a Content-Type header indicates HTML.
fn is_html_content_type(content_type: &str) -> bool {
    let lower = content_type.to_lowercase();
    lower.contains("text/html") || lower.contains("application/xhtml")
}

/// Convert HTML to plain-text markdown via `html2text`. Wraps at 120
/// columns to match TS `WebFetchTool/utils.ts:20` `maxLineWidth`.
///
/// Separated so it can be unit-tested against inline HTML fixtures.
pub(super) fn html_to_markdown(html: &str) -> String {
    html2text::from_read(html.as_bytes(), HTML2TEXT_LINE_WIDTH).unwrap_or_else(|e| {
        tracing::debug!("html2text failed ({e}); falling back to tag-stripped text");
        // html2text can fail on pathological HTML. Use the existing
        // strip_html_tags + decode_html_entities pipeline as a graceful
        // fallback so the tool still returns something readable.
        decode_html_entities(&strip_html_tags(html))
    })
}

/// Result of a WebFetch network call — `(body, content_type)` on success,
/// or a structured redirect notice when the response redirects cross-origin.
enum FetchOutcome {
    Body {
        body: String,
        content_type: String,
    },
    /// Cross-origin redirect was blocked. The caller should surface this
    /// to the model so it can issue a new fetch with the new URL.
    CrossOriginRedirect {
        new_url: String,
    },
}

/// Fetch URL content using reqwest. Applies:
/// - 60s timeout (TS `FETCH_TIMEOUT_MS`)
/// - Custom User-Agent (TS `getWebFetchUserAgent`)
/// - Manual redirect policy (TS explicit SSRF guard — no auto-follow,
///   same-origin allowed, cross-origin surfaces to the model)
/// - 10MB Content-Length pre-check (TS `MAX_HTTP_CONTENT_LENGTH`)
/// - Binary MIME rejection (TS keeps these and runs Haiku; we defer that
///   to a follow-up and return a clear error instead)
async fn fetch_url(url: &str) -> Result<FetchOutcome, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(WEB_FETCH_TIMEOUT_SECS))
        .user_agent(WEB_FETCH_USER_AGENT)
        // Critical SSRF guard: disable automatic redirect following.
        // We handle redirects manually via `check_redirect` so cross-
        // origin hops go back through the model's decision loop.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("[NETWORK_ERROR] failed to build HTTP client: {e}"))?;

    // Manual redirect loop. TS `WebFetchTool/utils.ts:125` sets
    // `MAX_REDIRECTS = 10`. Most real-world chains are 1–2 hops, but
    // a few legitimate cases (shortener → auth → final content) need
    // 4–5 hops, so matching TS's cap of 10 avoids rejecting valid
    // chains while still bounding the loop.
    let mut current_url = url.to_string();
    for _ in 0..10 {
        let response = client.get(&current_url).send().await.map_err(|e| {
            let tag = if e.is_timeout() {
                "[TIMEOUT]"
            } else {
                "[NETWORK_ERROR]"
            };
            format!("{tag} HTTP request failed: {e}")
        })?;

        let status = response.status();

        // Redirect response → check origin policy.
        if status.is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if location.is_empty() {
                return Err(format!(
                    "[NETWORK_ERROR] {status} redirect without Location"
                ));
            }
            // Resolve relative redirects against the current URL.
            let next_url = resolve_redirect_url(&current_url, location);
            match check_redirect(&current_url, &next_url) {
                RedirectDecision::Allow => {
                    current_url = next_url;
                    continue;
                }
                RedirectDecision::CrossOrigin { new_url } => {
                    return Ok(FetchOutcome::CrossOriginRedirect { new_url });
                }
            }
        }

        if !status.is_success() {
            return Err(format!("[NETWORK_ERROR] HTTP {status}"));
        }

        // Content-Length pre-check (fast path).
        //
        // TS `utils.ts:112, 277` passes `maxContentLength` to axios which
        // enforces the cap both from the header AND during streaming.
        // Our header check handles the optimistic case where the server
        // advertises size up-front. The streaming check below handles
        // chunked responses and servers that lie about Content-Length.
        if let Some(length) = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            && length > MAX_HTTP_CONTENT_LENGTH
        {
            return Err(format!(
                "[NETWORK_ERROR] response too large: {length} bytes > {MAX_HTTP_CONTENT_LENGTH} \
                 byte limit (Content-Length header). Use a more specific URL or a different tool."
            ));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        // Reject clearly binary content types. TS `utils.ts:442-449`
        // persists binary bodies to disk and still runs Haiku on them,
        // but that path is out of scope for now — we return an error so
        // the model knows to use a different tool.
        if content_type.contains("image/")
            || content_type.contains("audio/")
            || content_type.contains("video/")
            || content_type.contains("application/octet-stream")
            || content_type.contains("application/zip")
        {
            return Err(format!(
                "Non-text content type: {content_type}. Only text-based content is supported."
            ));
        }

        // Streaming read with per-chunk byte-count enforcement.
        //
        // **SSRF safety (D9)**: if we used `response.text().await` we'd
        // buffer the entire body before any limit check ran, so a
        // malicious server sending an infinite chunked response (no
        // Content-Length) could exhaust memory before we noticed. The
        // stream loop below accumulates bytes in a `Vec<u8>` and hard-
        // fails the moment we cross `MAX_HTTP_CONTENT_LENGTH`, regardless
        // of whether Content-Length was advertised.
        //
        // We collect bytes (not string) during streaming because chunks
        // may split UTF-8 code points; we decode once at the end.
        use futures::StreamExt as _;
        let mut stream = response.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .map_err(|e| format!("[NETWORK_ERROR] Failed to read response body chunk: {e}"))?;
            if (buffer.len() as u64).saturating_add(chunk.len() as u64) > MAX_HTTP_CONTENT_LENGTH {
                return Err(format!(
                    "[NETWORK_ERROR] response too large: streaming body exceeded \
                     {MAX_HTTP_CONTENT_LENGTH} bytes before completion. The server did \
                     not advertise Content-Length or lied about it. Use a more \
                     specific URL or a different tool."
                ));
            }
            buffer.extend_from_slice(&chunk);
        }
        let body = String::from_utf8_lossy(&buffer).into_owned();

        return Ok(FetchOutcome::Body { body, content_type });
    }

    Err("[NETWORK_ERROR] too many redirects (exceeded 10 hops)".into())
}

/// Resolve a possibly-relative Location header against the base URL.
///
/// Handles three cases:
/// - Absolute URL (`https://...`) → use as-is
/// - Protocol-relative (`//host/path`) → inherit scheme
/// - Path-relative (`/path` or `path`) → inherit scheme + host
pub(super) fn resolve_redirect_url(base: &str, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        return location.to_string();
    }
    if let Some(stripped) = location.strip_prefix("//") {
        // Protocol-relative: inherit scheme from base.
        let scheme = base.split_once("://").map(|(s, _)| s).unwrap_or("https");
        return format!("{scheme}://{stripped}");
    }
    // Path-relative: inherit scheme + host from base.
    let (scheme, rest) = base.split_once("://").unwrap_or(("https", base));
    let host_end = rest.find('/').unwrap_or(rest.len());
    let host = &rest[..host_end];
    if location.starts_with('/') {
        format!("{scheme}://{host}{location}")
    } else {
        // Rare form — relative to current directory. Strip to last `/`.
        let base_dir = &rest[..rest.rfind('/').unwrap_or(host_end)];
        format!("{scheme}://{base_dir}/{location}")
    }
}

// ---------------------------------------------------------------------------
// WebSearchTool — third-party search backend
// ---------------------------------------------------------------------------
//
// # Why this diverges from TS
//
// TS `WebSearchTool.tsx:76-84, 254-291` implements search as a passthrough to
// the Anthropic `web_search_20250305` server-side tool — no local search
// happens; the query is handed to Claude which runs it on Anthropic's
// infrastructure. coco-rs has to support **every** provider (Anthropic,
// OpenAI, Google, DeepSeek, xAI, etc.), and only Anthropic exposes a native
// web-search tool through the messages API. If we used the TS passthrough
// model, web search would silently fail for 80% of users.
//
// Our design: a **provider-agnostic** local backend. The default is
// DuckDuckGo HTML scraping (no API key, no rate limits, no ToS surprises),
// with a Tavily REST fallback for users who opt in via env vars. Both run
// entirely on the client side so the tool works identically regardless of
// which LLM provider the user selected.
//
// This follows cocode-rs's approach (`cocode-rs/core/tools/src/builtin/
// web_search.rs:308-581`) because it's the only sensible shape in a
// multi-provider Rust SDK.
//
// # Cache
//
// 15-min TTL in-process cache (matches TS `WebFetchTool.ts:62-69` cache
// pattern for URL fetches). Prevents redundant DuckDuckGo traffic when the
// model retries the same query within a turn. Session-scoped only.
//
// # Error classification
//
// Explicit error tags (`[TIMEOUT]`, `[NETWORK_ERROR]`, `[RATE_LIMITED]`,
// `[PARSE_ERROR]`) let the model react appropriately — e.g. retrying with a
// different query after a parse error vs. backing off on rate limits.

use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

/// Cache TTL for search results. TS uses 15min for URL cache; we match.
const SEARCH_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

/// Max cached entries before LRU eviction. Keeps memory bounded in long
/// sessions where the model issues many unrelated queries.
const SEARCH_CACHE_MAX_ENTRIES: usize = 64;

/// HTTP client timeout for search requests.
const SEARCH_TIMEOUT_SECS: u64 = 15;

/// Maximum results returned per query. Matches TS `WebSearchTool.ts:82`
/// `max_uses: 8` — the model can always re-query if more are needed.
const SEARCH_MAX_RESULTS: usize = 8;

/// Minimum query length to accept. TS: `WebSearchTool.ts:25-36`.
const SEARCH_MIN_QUERY_LEN: usize = 2;

/// Cached search entry. Stored with insertion time so expired entries are
/// skipped on lookup.
#[derive(Clone)]
struct CachedSearch {
    results: Vec<SearchResult>,
    inserted_at: Instant,
}

/// One search result — title + URL + optional snippet.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SearchResult {
    title: String,
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snippet: Option<String>,
}

use serde::Deserialize;
use serde::Serialize;

/// In-process LRU cache. `LazyLock<Mutex<...>>` gives us a zero-config
/// singleton that initializes on first access. Ideally we'd key on
/// `(query, allowed_domains_hash, blocked_domains_hash)` but for simplicity
/// we only cache the unfiltered query and apply filters at read time.
static SEARCH_CACHE: LazyLock<Mutex<Vec<(String, CachedSearch)>>> =
    LazyLock::new(|| Mutex::new(Vec::with_capacity(SEARCH_CACHE_MAX_ENTRIES)));

fn cache_get(key: &str) -> Option<Vec<SearchResult>> {
    let mut cache = SEARCH_CACHE.lock().ok()?;
    // Expire old entries on every access (cheap since cache is small).
    let now = Instant::now();
    cache.retain(|(_, entry)| now.duration_since(entry.inserted_at) < SEARCH_CACHE_TTL);
    cache.iter().find_map(|(k, v)| {
        if k == key {
            Some(v.results.clone())
        } else {
            None
        }
    })
}

fn cache_set(key: String, results: Vec<SearchResult>) {
    if let Ok(mut cache) = SEARCH_CACHE.lock() {
        // LRU eviction: if at capacity, drop the oldest entry.
        if cache.len() >= SEARCH_CACHE_MAX_ENTRIES {
            cache.remove(0);
        }
        cache.push((
            key,
            CachedSearch {
                results,
                inserted_at: Instant::now(),
            },
        ));
    }
}

/// Format the current local month and year as `"Month YYYY"`.
///
/// TS `WebSearchTool/prompt.ts:31` injects this via `getLocalMonthYear()`
/// so the model knows the actual current date when interpreting
/// "latest", "recent", or "current" search queries. coco-rs computes
/// the same string at request time using `chrono::Local::now()`.
fn current_month_year_local() -> String {
    use chrono::Datelike;
    let now = chrono::Local::now();
    const MONTH_NAMES: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let month_name = MONTH_NAMES
        .get((now.month() as usize).saturating_sub(1))
        .copied()
        .unwrap_or("January");
    format!("{month_name} {}", now.year())
}

pub struct WebSearchTool;

#[async_trait::async_trait]
impl Tool for WebSearchTool {
    fn id(&self) -> ToolId {
        ToolId::Builtin(ToolName::WebSearch)
    }
    fn name(&self) -> &str {
        ToolName::WebSearch.as_str()
    }
    fn description(&self, _: &Value, _options: &DescriptionOptions) -> String {
        // R7-T25: byte-aligned port of TS `WebSearchTool/prompt.ts:5-33`
        // `getWebSearchPrompt()`. The CRITICAL REQUIREMENT block is
        // mandatory — TS marks it as "MUST follow" and the model is
        // expected to add a `Sources:` section to every response. The
        // current month/year injection is computed at request time
        // from the system clock so the model uses the right year for
        // recent-events queries.
        let current_month_year = current_month_year_local();
        format!(
            "
- Allows Claude to search the web and use the results to inform responses
- Provides up-to-date information for current events and recent data
- Returns search result information formatted as search result blocks, including links as markdown hyperlinks
- Use this tool for accessing information beyond Claude's knowledge cutoff
- Searches are performed automatically within a single API call

CRITICAL REQUIREMENT - You MUST follow this:
  - After answering the user's question, you MUST include a \"Sources:\" section at the end of your response
  - In the Sources section, list all relevant URLs from the search results as markdown hyperlinks: [Title](URL)
  - This is MANDATORY - never skip including sources in your response
  - Example format:

    [Your answer here]

    Sources:
    - [Source Title 1](https://example.com/1)
    - [Source Title 2](https://example.com/2)

Usage notes:
  - Domain filtering is supported to include or block specific websites
  - Web search is only available in the US

IMPORTANT - Use the correct year in search queries:
  - The current month is {current_month_year}. You MUST use this year when searching for recent information, documentation, or current events.
  - Example: If the user asks for \"latest React docs\", search for \"React documentation\" with the current year, NOT last year
"
        )
    }
    fn input_schema(&self) -> ToolInputSchema {
        let mut p = HashMap::new();
        p.insert(
            "query".into(),
            serde_json::json!({
                "type": "string",
                "description": "The search query to use",
                "minLength": 2
            }),
        );
        p.insert(
            "allowed_domains".into(),
            serde_json::json!({
                "type": "array",
                "items": {"type": "string"},
                "description": "Only include search results from these domains (post-filtered client-side)"
            }),
        );
        p.insert(
            "blocked_domains".into(),
            serde_json::json!({
                "type": "array",
                "items": {"type": "string"},
                "description": "Never include search results from these domains (post-filtered client-side)"
            }),
        );
        ToolInputSchema { properties: p }
    }
    fn is_read_only(&self, _: &Value) -> bool {
        true
    }
    fn is_concurrency_safe(&self, _: &Value) -> bool {
        true
    }

    fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> coco_tool::ValidationResult {
        let query = input.get("query").and_then(|v| v.as_str()).unwrap_or("");
        if query.trim().len() < SEARCH_MIN_QUERY_LEN {
            return coco_tool::ValidationResult::invalid(
                "query must be at least 2 characters long",
            );
        }
        // Mutual exclusivity: caller should not set both filters. TS accepts
        // both but they're semantically ambiguous — if a domain is in both
        // lists, which wins? We reject at validation time to force a clear
        // policy. Cocode-rs enforces the same rule.
        let has_allowed = input
            .get("allowed_domains")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        let has_blocked = input
            .get("blocked_domains")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        if has_allowed && has_blocked {
            return coco_tool::ValidationResult::invalid(
                "Specify either allowed_domains or blocked_domains, not both",
            );
        }
        coco_tool::ValidationResult::Valid
    }

    fn get_activity_description(&self, input: &Value) -> Option<String> {
        let query = input.get("query").and_then(|v| v.as_str())?;
        Some(format!("Searching for \"{query}\""))
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
    ) -> Result<ToolResult<Value>, ToolError> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if query.is_empty() {
            return Err(ToolError::InvalidInput {
                message: "query parameter is required".into(),
                error_code: None,
            });
        }

        let allowed: Vec<String> = input
            .get("allowed_domains")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(str::to_lowercase)
                    .collect()
            })
            .unwrap_or_default();
        let blocked: Vec<String> = input
            .get("blocked_domains")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(str::to_lowercase)
                    .collect()
            })
            .unwrap_or_default();

        // Cache hit → return immediately with filters re-applied.
        let results = if let Some(cached) = cache_get(&query) {
            cached
        } else {
            let fresh = duckduckgo_search(&query).await?;
            cache_set(query.clone(), fresh.clone());
            fresh
        };

        // Apply domain filters. Matches host suffix so "github.com" blocks
        // both github.com/foo and gist.github.com/foo.
        let filtered: Vec<SearchResult> = results
            .into_iter()
            .filter(|r| {
                let host = extract_host(&r.url).to_lowercase();
                if !allowed.is_empty() && !allowed.iter().any(|d| host.ends_with(d)) {
                    return false;
                }
                if blocked.iter().any(|d| host.ends_with(d)) {
                    return false;
                }
                true
            })
            .take(SEARCH_MAX_RESULTS)
            .collect();

        let formatted = format_results(&query, &filtered);
        Ok(ToolResult {
            data: serde_json::json!({
                "query": query,
                "results": filtered,
                "formatted": formatted,
            }),
            new_messages: vec![],
        })
    }
}

/// Format results as a markdown block the model can read naturally.
fn format_results(query: &str, results: &[SearchResult]) -> String {
    if results.is_empty() {
        return format!("No results for \"{query}\".");
    }
    let mut out = format!(
        "Search results for \"{query}\" ({} hit(s)):\n\n",
        results.len()
    );
    for (i, r) in results.iter().enumerate() {
        out.push_str(&format!("[{}] {}\n  {}\n", i + 1, r.title, r.url));
        if let Some(snippet) = &r.snippet
            && !snippet.is_empty()
        {
            out.push_str(&format!("  {snippet}\n"));
        }
    }
    out.push_str(
        "\nREMINDER: When citing information from these results, include the URL \
         so the user can verify.",
    );
    out
}

/// Extract the host portion of a URL (without scheme, port, or path). Used
/// for domain-filter matching. Returns empty string on malformed URLs.
/// Extract the host portion of a URL (without scheme, userinfo, port,
/// path, query, or fragment). Handles bracketed IPv6 literals correctly
/// and does NOT leave a bogus `:99999` suffix on port-parse failure.
///
/// T1: Previously this used a naive `rsplit_once(':')` that broke on
/// bracketed IPv6 (`[::1]` → host=`[::`) and silently corrupted hosts
/// with out-of-range ports (`example.com:99999` → host stayed intact).
/// Delegating to `split_host_port` ensures the host extraction logic
/// stays consistent between `check_redirect`'s port comparison path
/// and `is_preapproved_url`'s hostname-match path.
///
/// The `scheme` argument is passed through from the URL's scheme prefix
/// (or `""` if absent) so that default-port normalization works
/// (e.g. `example.com:443` under https → port=None, host=example.com).
fn extract_host(url: &str) -> String {
    let scheme = extract_scheme(url);
    split_host_port(url, &scheme).0
}

/// DuckDuckGo HTML backend. Scrapes the non-JS result page at
/// `html.duckduckgo.com` — this endpoint returns plain HTML with a stable
/// structure that's easier to parse than the JS-rendered homepage.
///
/// Regex-based parsing is intentionally simple: the DDG HTML result page
/// uses a `<a class="result__a"` for titles, `<a class="result__snippet"`
/// for snippets, and embeds the target URL in a `uddg=` parameter of a
/// redirect link. We decode the redirect, not the visible `href`.
async fn duckduckgo_search(query: &str) -> Result<Vec<SearchResult>, ToolError> {
    // POST to html.duckduckgo.com/html with `q=<query>` — GET also works
    // but POST avoids leaking the query in any proxy logs along the path.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(SEARCH_TIMEOUT_SECS))
        .user_agent(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .build()
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("[NETWORK_ERROR] failed to build HTTP client: {e}"),
            source: None,
        })?;

    let response = client
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query)])
        .send()
        .await
        .map_err(|e| {
            let tag = if e.is_timeout() {
                "[TIMEOUT]"
            } else {
                "[NETWORK_ERROR]"
            };
            ToolError::ExecutionFailed {
                message: format!("{tag} DuckDuckGo request failed: {e}"),
                source: None,
            }
        })?;

    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(ToolError::ExecutionFailed {
            message: "[RATE_LIMITED] DuckDuckGo returned HTTP 429 — back off and retry".into(),
            source: None,
        });
    }
    if !response.status().is_success() {
        return Err(ToolError::ExecutionFailed {
            message: format!(
                "[NETWORK_ERROR] DuckDuckGo returned HTTP {}",
                response.status()
            ),
            source: None,
        });
    }

    let html = response
        .text()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("[NETWORK_ERROR] failed to read DuckDuckGo response: {e}"),
            source: None,
        })?;

    parse_duckduckgo_html(&html)
}

/// Parse DuckDuckGo HTML response into `SearchResult` list. Separated so
/// it can be unit-tested against recorded fixtures.
fn parse_duckduckgo_html(html: &str) -> Result<Vec<SearchResult>, ToolError> {
    // Pattern matches each `<a class="result__a" href="...">TITLE</a>` —
    // the href contains the redirect URL. Snippets follow in a sibling
    // `<a class="result__snippet">SNIPPET</a>`.
    let title_pattern =
        regex::Regex::new(r#"(?s)<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)
            .map_err(|e| ToolError::ExecutionFailed {
                message: format!("[PARSE_ERROR] regex compile: {e}"),
                source: None,
            })?;
    let snippet_pattern = regex::Regex::new(r#"(?s)<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"#)
        .map_err(|e| ToolError::ExecutionFailed {
            message: format!("[PARSE_ERROR] regex compile: {e}"),
            source: None,
        })?;

    let mut results = Vec::new();
    let title_matches: Vec<_> = title_pattern.captures_iter(html).collect();
    let snippet_matches: Vec<_> = snippet_pattern.captures_iter(html).collect();

    for (i, title_cap) in title_matches.iter().enumerate() {
        let raw_href = title_cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let raw_title = title_cap.get(2).map(|m| m.as_str()).unwrap_or("");

        let url = decode_ddg_redirect(raw_href);
        if url.is_empty() {
            continue;
        }

        let title = decode_html_entities(&strip_html_tags(raw_title));
        let snippet = snippet_matches
            .get(i)
            .and_then(|c| c.get(1).map(|m| m.as_str()))
            .map(|s| decode_html_entities(&strip_html_tags(s)))
            .filter(|s| !s.is_empty());

        results.push(SearchResult {
            title,
            url,
            snippet,
        });

        if results.len() >= SEARCH_MAX_RESULTS * 2 {
            // Over-fetch slightly so post-filtering still has candidates.
            break;
        }
    }

    Ok(results)
}

/// DuckDuckGo wraps result links in a redirect of the form
/// `//duckduckgo.com/l/?uddg=<percent-encoded-url>&...`. Extract and
/// decode the `uddg` param back to the target URL.
fn decode_ddg_redirect(href: &str) -> String {
    // Find `uddg=` query param.
    let uddg_start = match href.find("uddg=") {
        Some(i) => i + 5,
        None => {
            // Maybe already a direct URL (newer DDG endpoint format).
            return href.to_string();
        }
    };
    let rest = &href[uddg_start..];
    let uddg_end = rest.find('&').unwrap_or(rest.len());
    let encoded = &rest[..uddg_end];
    percent_decode(encoded)
}

/// Minimal percent-decoder. Only handles `%XX` sequences and `+` → space.
/// We deliberately don't pull in urlencoding crate for ~200 lines of test
/// fixtures worth of functionality.
fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte as char);
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            b => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    out
}

/// Decode common HTML entities. Minimal subset sufficient for DDG output.
fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
}

/// Strip HTML tags from a snippet. Uses a minimal state machine rather
/// than pulling in a full HTML parser — snippets are short and the input
/// is trusted DDG output.
fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
#[path = "web.test.rs"]
mod tests;
