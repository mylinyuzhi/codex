//! Full LSP tool implementation ported from TS LSPTool/.
//!
//! TS: tools/LSPTool/LSPTool.ts, formatters.ts, schemas.ts
//!
//! Provides code intelligence via Language Server Protocol:
//! go-to-definition, find-references, hover, document symbols,
//! workspace symbols, go-to-implementation, call hierarchy,
//! and diagnostics listing.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── LSP operation enum ──

/// All supported LSP operations (matches TS discriminated union).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LspOperation {
    GoToDefinition,
    FindReferences,
    Hover,
    DocumentSymbol,
    WorkspaceSymbol,
    GoToImplementation,
    PrepareCallHierarchy,
    IncomingCalls,
    OutgoingCalls,
    Diagnostics,
}

impl LspOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::GoToDefinition => "goToDefinition",
            Self::FindReferences => "findReferences",
            Self::Hover => "hover",
            Self::DocumentSymbol => "documentSymbol",
            Self::WorkspaceSymbol => "workspaceSymbol",
            Self::GoToImplementation => "goToImplementation",
            Self::PrepareCallHierarchy => "prepareCallHierarchy",
            Self::IncomingCalls => "incomingCalls",
            Self::OutgoingCalls => "outgoingCalls",
            Self::Diagnostics => "diagnostics",
        }
    }

    /// Map operation to LSP method string.
    pub fn lsp_method(&self) -> &'static str {
        match self {
            Self::GoToDefinition => "textDocument/definition",
            Self::FindReferences => "textDocument/references",
            Self::Hover => "textDocument/hover",
            Self::DocumentSymbol => "textDocument/documentSymbol",
            Self::WorkspaceSymbol => "workspace/symbol",
            Self::GoToImplementation => "textDocument/implementation",
            Self::PrepareCallHierarchy => "textDocument/prepareCallHierarchy",
            Self::IncomingCalls => "textDocument/prepareCallHierarchy",
            Self::OutgoingCalls => "textDocument/prepareCallHierarchy",
            Self::Diagnostics => "textDocument/diagnostic",
        }
    }

    /// Whether this operation requires a file position (line + character).
    pub fn requires_position(&self) -> bool {
        match self {
            Self::DocumentSymbol | Self::WorkspaceSymbol | Self::Diagnostics => false,
            _ => true,
        }
    }
}

// ── LSP data structures ──

/// A location returned by the LSP server (file URI + range).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspLocation {
    pub uri: String,
    pub range: LspRange,
}

/// Start/end position pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

/// Zero-based line and character offset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspPosition {
    pub line: i32,
    pub character: i32,
}

/// Symbol kind (LSP spec values 1-26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolKind(pub i32);

impl SymbolKind {
    pub fn label(&self) -> &'static str {
        match self.0 {
            1 => "File",
            2 => "Module",
            3 => "Namespace",
            4 => "Package",
            5 => "Class",
            6 => "Method",
            7 => "Property",
            8 => "Field",
            9 => "Constructor",
            10 => "Enum",
            11 => "Interface",
            12 => "Function",
            13 => "Variable",
            14 => "Constant",
            15 => "String",
            16 => "Number",
            17 => "Boolean",
            18 => "Array",
            19 => "Object",
            20 => "Key",
            21 => "Null",
            22 => "EnumMember",
            23 => "Struct",
            24 => "Event",
            25 => "Operator",
            26 => "TypeParameter",
            _ => "Unknown",
        }
    }
}

/// A symbol in a document (hierarchical, with optional children).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: LspRange,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub children: Vec<DocumentSymbol>,
}

/// Flat symbol with location (workspace symbol format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolInformation {
    pub name: String,
    pub kind: SymbolKind,
    pub location: LspLocation,
    #[serde(default, rename = "containerName")]
    pub container_name: Option<String>,
}

/// Hover contents from LSP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoverResult {
    pub contents: HoverContents,
    #[serde(default)]
    pub range: Option<LspRange>,
}

/// Hover content can be a plain string or markup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoverContents {
    String(String),
    Markup { kind: String, value: String },
    Array(Vec<HoverContents>),
}

impl HoverContents {
    pub fn to_text(&self) -> String {
        match self {
            Self::String(s) => s.clone(),
            Self::Markup { value, .. } => value.clone(),
            Self::Array(items) => items.iter().map(Self::to_text).collect::<Vec<_>>().join("\n\n"),
        }
    }
}

/// Call hierarchy item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallHierarchyItem {
    pub name: String,
    pub kind: SymbolKind,
    pub uri: String,
    pub range: LspRange,
    #[serde(default)]
    pub detail: Option<String>,
}

/// Incoming call (who calls this function).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingCall {
    pub from: CallHierarchyItem,
    #[serde(default, rename = "fromRanges")]
    pub from_ranges: Vec<LspRange>,
}

/// Outgoing call (what this function calls).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingCall {
    pub to: CallHierarchyItem,
    #[serde(default, rename = "fromRanges")]
    pub from_ranges: Vec<LspRange>,
}

/// LSP diagnostic severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticSeverity(pub i32);

impl DiagnosticSeverity {
    pub fn label(&self) -> &'static str {
        match self.0 {
            1 => "Error",
            2 => "Warning",
            3 => "Information",
            4 => "Hint",
            _ => "Unknown",
        }
    }
}

/// A single diagnostic (error/warning).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub range: LspRange,
    pub severity: Option<DiagnosticSeverity>,
    pub message: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub code: Option<Value>,
}

/// Structured output from an LSP operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspOutput {
    pub operation: String,
    pub result: String,
    pub file_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_count: Option<i32>,
}

/// Maximum LSP file size for analysis (10 MB).
const MAX_LSP_FILE_SIZE_BYTES: u64 = 10_000_000;

// ── URI utilities ──

/// Convert a file:// URI to a filesystem path, decoding percent-encoding.
pub fn uri_to_file_path(uri: &str) -> String {
    let mut path = uri.strip_prefix("file://").unwrap_or(uri).to_string();

    // Windows: file:///C:/path becomes /C:/path — strip leading slash
    if path.len() >= 3 && path.as_bytes()[0] == b'/' && path.as_bytes()[2] == b':' {
        path = path[1..].to_string();
    }

    urlencoding::decode(&path)
        .map(|s| s.into_owned())
        .unwrap_or(path)
}

/// Format a URI as a display path, using relative paths when shorter.
pub fn format_uri(uri: &str, cwd: Option<&str>) -> String {
    if uri.is_empty() {
        return "<unknown location>".to_string();
    }

    let file_path = uri_to_file_path(uri);

    if let Some(cwd) = cwd {
        if let Ok(rel) = pathdiff::diff_paths(&file_path, cwd) {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if rel_str.len() < file_path.len() && !rel_str.starts_with("../../") {
                return rel_str;
            }
        }
    }

    file_path.replace('\\', "/")
}

/// Format a location as "path:line:col" (1-based).
fn format_location(loc: &LspLocation, cwd: Option<&str>) -> String {
    let path = format_uri(&loc.uri, cwd);
    let line = loc.range.start.line + 1;
    let character = loc.range.start.character + 1;
    format!("{path}:{line}:{character}")
}

// ── Formatters (ported from TS formatters.ts) ──

/// Format go-to-definition / go-to-implementation results.
pub fn format_definition_result(locations: &[LspLocation], cwd: Option<&str>) -> String {
    let valid: Vec<_> = locations.iter().filter(|l| !l.uri.is_empty()).collect();
    if valid.is_empty() {
        return "No definition found. The cursor may not be on a symbol, \
                or the definition is in an external library not indexed by the LSP server."
            .to_string();
    }
    if valid.len() == 1 {
        return format!("Defined in {}", format_location(valid[0], cwd));
    }
    let list: Vec<String> = valid.iter().map(|l| format!("  {}", format_location(l, cwd))).collect();
    format!("Found {} definitions:\n{}", valid.len(), list.join("\n"))
}

/// Format find-references results, grouped by file.
pub fn format_references_result(locations: &[LspLocation], cwd: Option<&str>) -> String {
    let valid: Vec<_> = locations.iter().filter(|l| !l.uri.is_empty()).collect();
    if valid.is_empty() {
        return "No references found. The symbol may have no usages, \
                or the LSP server has not fully indexed the workspace."
            .to_string();
    }
    if valid.len() == 1 {
        return format!("Found 1 reference:\n  {}", format_location(valid[0], cwd));
    }

    let by_file = group_locations_by_file(&valid, cwd);
    let mut lines = vec![format!(
        "Found {} references across {} files:",
        valid.len(),
        by_file.len()
    )];
    for (path, locs) in &by_file {
        lines.push(format!("\n{path}:"));
        for loc in locs {
            let line = loc.range.start.line + 1;
            let ch = loc.range.start.character + 1;
            lines.push(format!("  Line {line}:{ch}"));
        }
    }
    lines.join("\n")
}

/// Format hover information.
pub fn format_hover_result(hover: Option<&HoverResult>) -> String {
    let Some(hover) = hover else {
        return "No hover information available. The cursor may not be on a symbol, \
                or the LSP server has not fully indexed the file."
            .to_string();
    };
    let content = hover.contents.to_text();
    if let Some(range) = &hover.range {
        let line = range.start.line + 1;
        let ch = range.start.character + 1;
        format!("Hover info at {line}:{ch}:\n\n{content}")
    } else {
        content
    }
}

/// Format document symbols (hierarchical outline).
pub fn format_document_symbols(symbols: &[DocumentSymbol]) -> String {
    if symbols.is_empty() {
        return "No symbols found in document.".to_string();
    }
    let mut lines = vec!["Document symbols:".to_string()];
    for sym in symbols {
        format_symbol_tree(sym, /*indent*/ 0, &mut lines);
    }
    lines.join("\n")
}

fn format_symbol_tree(sym: &DocumentSymbol, indent: i32, lines: &mut Vec<String>) {
    let prefix = "  ".repeat(indent as usize);
    let kind = sym.kind.label();
    let line_num = sym.range.start.line + 1;
    let mut entry = format!("{prefix}{} ({kind})", sym.name);
    if let Some(detail) = &sym.detail {
        entry.push(' ');
        entry.push_str(detail);
    }
    entry.push_str(&format!(" - Line {line_num}"));
    lines.push(entry);
    for child in &sym.children {
        format_symbol_tree(child, indent + 1, lines);
    }
}

/// Count total symbols including nested children.
pub fn count_symbols(symbols: &[DocumentSymbol]) -> i32 {
    let mut count = symbols.len() as i32;
    for sym in symbols {
        count += count_symbols(&sym.children);
    }
    count
}

/// Format workspace symbols (flat list grouped by file).
pub fn format_workspace_symbols(symbols: &[SymbolInformation], cwd: Option<&str>) -> String {
    let valid: Vec<_> = symbols.iter().filter(|s| !s.location.uri.is_empty()).collect();
    if valid.is_empty() {
        return "No symbols found in workspace.".to_string();
    }

    let mut by_file: Vec<(String, Vec<&SymbolInformation>)> = Vec::new();
    let mut file_map: HashMap<String, usize> = HashMap::new();
    for sym in &valid {
        let path = format_uri(&sym.location.uri, cwd);
        if let Some(&idx) = file_map.get(&path) {
            by_file[idx].1.push(sym);
        } else {
            file_map.insert(path.clone(), by_file.len());
            by_file.push((path, vec![sym]));
        }
    }

    let mut lines = vec![format!(
        "Found {} {} in workspace:",
        valid.len(),
        if valid.len() == 1 { "symbol" } else { "symbols" }
    )];
    for (path, syms) in &by_file {
        lines.push(format!("\n{path}:"));
        for sym in syms {
            let kind = sym.kind.label();
            let line = sym.location.range.start.line + 1;
            let mut entry = format!("  {} ({kind}) - Line {line}", sym.name);
            if let Some(container) = &sym.container_name {
                entry.push_str(&format!(" in {container}"));
            }
            lines.push(entry);
        }
    }
    lines.join("\n")
}

/// Format call hierarchy items.
pub fn format_call_hierarchy(items: &[CallHierarchyItem], cwd: Option<&str>) -> String {
    if items.is_empty() {
        return "No call hierarchy item found at this position".to_string();
    }
    if items.len() == 1 {
        return format!("Call hierarchy item: {}", format_call_item(&items[0], cwd));
    }
    let mut lines = vec![format!("Found {} call hierarchy items:", items.len())];
    for item in items {
        lines.push(format!("  {}", format_call_item(item, cwd)));
    }
    lines.join("\n")
}

fn format_call_item(item: &CallHierarchyItem, cwd: Option<&str>) -> String {
    let path = format_uri(&item.uri, cwd);
    let line = item.range.start.line + 1;
    let kind = item.kind.label();
    let mut result = format!("{} ({kind}) - {path}:{line}", item.name);
    if let Some(detail) = &item.detail {
        result.push_str(&format!(" [{detail}]"));
    }
    result
}

/// Format incoming calls result.
pub fn format_incoming_calls(calls: &[IncomingCall], cwd: Option<&str>) -> String {
    if calls.is_empty() {
        return "No incoming calls found (nothing calls this function)".to_string();
    }

    let call_word = if calls.len() == 1 { "call" } else { "calls" };
    let mut lines = vec![format!("Found {} incoming {call_word}:", calls.len())];

    let mut by_file: Vec<(String, Vec<&IncomingCall>)> = Vec::new();
    let mut file_map: HashMap<String, usize> = HashMap::new();
    for call in calls {
        let path = format_uri(&call.from.uri, cwd);
        if let Some(&idx) = file_map.get(&path) {
            by_file[idx].1.push(call);
        } else {
            file_map.insert(path.clone(), by_file.len());
            by_file.push((path, vec![call]));
        }
    }

    for (path, file_calls) in &by_file {
        lines.push(format!("\n{path}:"));
        for call in file_calls {
            let kind = call.from.kind.label();
            let line = call.from.range.start.line + 1;
            let mut entry = format!("  {} ({kind}) - Line {line}", call.from.name);
            if !call.from_ranges.is_empty() {
                let sites: Vec<String> = call
                    .from_ranges
                    .iter()
                    .map(|r| format!("{}:{}", r.start.line + 1, r.start.character + 1))
                    .collect();
                entry.push_str(&format!(" [calls at: {}]", sites.join(", ")));
            }
            lines.push(entry);
        }
    }
    lines.join("\n")
}

/// Format outgoing calls result.
pub fn format_outgoing_calls(calls: &[OutgoingCall], cwd: Option<&str>) -> String {
    if calls.is_empty() {
        return "No outgoing calls found (this function calls nothing)".to_string();
    }

    let call_word = if calls.len() == 1 { "call" } else { "calls" };
    let mut lines = vec![format!("Found {} outgoing {call_word}:", calls.len())];

    let mut by_file: Vec<(String, Vec<&OutgoingCall>)> = Vec::new();
    let mut file_map: HashMap<String, usize> = HashMap::new();
    for call in calls {
        let path = format_uri(&call.to.uri, cwd);
        if let Some(&idx) = file_map.get(&path) {
            by_file[idx].1.push(call);
        } else {
            file_map.insert(path.clone(), by_file.len());
            by_file.push((path, vec![call]));
        }
    }

    for (path, file_calls) in &by_file {
        lines.push(format!("\n{path}:"));
        for call in file_calls {
            let kind = call.to.kind.label();
            let line = call.to.range.start.line + 1;
            let mut entry = format!("  {} ({kind}) - Line {line}", call.to.name);
            if !call.from_ranges.is_empty() {
                let sites: Vec<String> = call
                    .from_ranges
                    .iter()
                    .map(|r| format!("{}:{}", r.start.line + 1, r.start.character + 1))
                    .collect();
                entry.push_str(&format!(" [called from: {}]", sites.join(", ")));
            }
            lines.push(entry);
        }
    }
    lines.join("\n")
}

/// Format diagnostics for a file.
pub fn format_diagnostics(diagnostics: &[LspDiagnostic], file_path: &str) -> String {
    if diagnostics.is_empty() {
        return format!("No diagnostics for {file_path}");
    }
    let mut lines = vec![format!(
        "Found {} {} in {file_path}:",
        diagnostics.len(),
        if diagnostics.len() == 1 { "diagnostic" } else { "diagnostics" }
    )];
    for diag in diagnostics {
        let line = diag.range.start.line + 1;
        let ch = diag.range.start.character + 1;
        let severity = diag.severity.map(|s| s.label()).unwrap_or("Unknown");
        let mut entry = format!("  {severity} at {line}:{ch}: {}", diag.message);
        if let Some(source) = &diag.source {
            entry.push_str(&format!(" ({source})"));
        }
        if let Some(code) = &diag.code {
            match code {
                Value::String(s) => entry.push_str(&format!(" [{s}]")),
                Value::Number(n) => entry.push_str(&format!(" [{n}]")),
                _ => {}
            }
        }
        lines.push(entry);
    }
    lines.join("\n")
}

// ── Helpers ──

/// Group locations by file URI, preserving insertion order.
fn group_locations_by_file<'a>(
    locations: &[&'a LspLocation],
    cwd: Option<&str>,
) -> Vec<(String, Vec<&'a LspLocation>)> {
    let mut by_file: Vec<(String, Vec<&'a LspLocation>)> = Vec::new();
    let mut file_map: HashMap<String, usize> = HashMap::new();
    for loc in locations {
        let path = format_uri(&loc.uri, cwd);
        if let Some(&idx) = file_map.get(&path) {
            by_file[idx].1.push(loc);
        } else {
            file_map.insert(path.clone(), by_file.len());
            by_file.push((path, vec![loc]));
        }
    }
    by_file
}

/// Count unique files from a set of locations.
pub fn count_unique_files(locations: &[LspLocation]) -> i32 {
    let uris: HashSet<_> = locations.iter().map(|l| l.uri.as_str()).collect();
    uris.len() as i32
}

/// Validate that a file exists, is a regular file, and is within size limits.
pub fn validate_lsp_file(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }
    if !path.is_file() {
        return Err(format!("Path is not a file: {}", path.display()));
    }
    if let Ok(meta) = path.metadata() {
        if meta.len() > MAX_LSP_FILE_SIZE_BYTES {
            let mb = meta.len() / 1_000_000;
            return Err(format!(
                "File too large for LSP analysis ({mb}MB exceeds 10MB limit)"
            ));
        }
    }
    Ok(())
}

/// Build LSP request params from operation, file URI, and position.
pub fn build_lsp_params(
    operation: LspOperation,
    file_uri: &str,
    line: Option<i32>,
    character: Option<i32>,
) -> Value {
    // Convert from 1-based (user-facing) to 0-based (LSP protocol)
    let position = serde_json::json!({
        "line": line.unwrap_or(1) - 1,
        "character": character.unwrap_or(1) - 1,
    });

    match operation {
        LspOperation::DocumentSymbol => serde_json::json!({
            "textDocument": { "uri": file_uri }
        }),
        LspOperation::WorkspaceSymbol => serde_json::json!({
            "query": ""
        }),
        LspOperation::Diagnostics => serde_json::json!({
            "textDocument": { "uri": file_uri }
        }),
        LspOperation::FindReferences => serde_json::json!({
            "textDocument": { "uri": file_uri },
            "position": position,
            "context": { "includeDeclaration": true }
        }),
        _ => serde_json::json!({
            "textDocument": { "uri": file_uri },
            "position": position,
        }),
    }
}

/// Convert a file path to a file:// URI.
pub fn path_to_file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        // Windows drive path
        format!("file:///{normalized}")
    }
}

#[cfg(test)]
#[path = "lsp.test.rs"]
mod tests;
