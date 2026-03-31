//! LSP tool for language server protocol operations.
//!
//! Provides IDE-like features through LSP: go to definition, find references,
//! hover documentation, document symbols, workspace symbols, and more.

use super::prompts;
use crate::context::ToolContext;
use crate::error::Result;
use crate::error::tool_error;
use crate::tool::Tool;
use async_trait::async_trait;
use cocode_file_ignore::IgnoreConfig;
use cocode_file_ignore::IgnoreService;
use cocode_file_ignore::PathChecker;
use cocode_lsp::SymbolKind;
use cocode_protocol::ConcurrencySafety;
use cocode_protocol::ToolOutput;
use serde_json::Value;
use std::path::Path;

/// Tool for LSP operations.
///
/// This is a read-only, concurrency-safe tool that provides language
/// intelligence features through Language Server Protocol.
pub struct LspTool;

impl LspTool {
    /// Create a new LSP tool.
    pub fn new() -> Self {
        Self
    }
}

impl Default for LspTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &str {
        cocode_protocol::ToolName::Lsp.as_str()
    }

    fn description(&self) -> &str {
        prompts::LSP_DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "description": "The LSP operation to perform",
                    "enum": [
                        "goToDefinition",
                        "findReferences",
                        "hover",
                        "documentSymbol",
                        "workspaceSymbol",
                        "goToImplementation",
                        "goToTypeDefinition",
                        "goToDeclaration",
                        "getCallHierarchy",
                        "getDiagnostics"
                    ]
                },
                "filePath": {
                    "type": "string",
                    "description": "The absolute path to the file"
                },
                "symbolName": {
                    "type": "string",
                    "description": "The name of the symbol to query (AI-friendly)"
                },
                "symbolKind": {
                    "type": "string",
                    "description": "The kind of symbol (e.g., 'function', 'struct', 'trait')"
                },
                "line": {
                    "type": "integer",
                    "description": "0-indexed line number for position-based queries"
                },
                "character": {
                    "type": "integer",
                    "description": "0-indexed character offset for position-based queries"
                },
                "query": {
                    "type": "string",
                    "description": "Search query for workspaceSymbol operation"
                },
                "includeDeclaration": {
                    "type": "boolean",
                    "description": "Include declaration in references (default: true)",
                    "default": true
                },
                "direction": {
                    "type": "string",
                    "description": "Direction for call hierarchy: 'incoming' or 'outgoing'",
                    "enum": ["incoming", "outgoing"]
                }
            },
            "required": ["operation"]
        })
    }

    fn concurrency_safety(&self) -> ConcurrencySafety {
        ConcurrencySafety::Safe
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn max_result_size_chars(&self) -> i32 {
        100_000
    }

    fn feature_gate(&self) -> Option<cocode_protocol::Feature> {
        Some(cocode_protocol::Feature::Lsp)
    }

    async fn execute(&self, input: Value, ctx: &mut ToolContext) -> Result<ToolOutput> {
        let manager = ctx.services.lsp_manager.as_ref().ok_or_else(|| {
            tool_error::ExecutionFailedSnafu {
                message: "LSP feature not enabled. No LSP server manager available.",
            }
            .build()
        })?;

        let operation = input["operation"].as_str().ok_or_else(|| {
            tool_error::InvalidInputSnafu {
                message: "operation must be a string",
            }
            .build()
        })?;

        // Most operations require a file path
        let file_path = input["filePath"].as_str();

        // Parse symbol name and kind for symbol-based queries
        let symbol_name = input["symbolName"].as_str();
        let symbol_kind = input["symbolKind"]
            .as_str()
            .and_then(SymbolKind::from_str_loose);

        // Parse position for position-based queries
        let line = input["line"].as_i64().map(|n| n as u32);
        let character = input["character"].as_i64().map(|n| n as u32);

        // Build ignore checker for filtering cross-file results
        let ignore_checker = build_ignore_checker(&ctx.env.cwd);

        let result = match operation {
            "goToDefinition" => {
                let path = require_file_path(file_path)?;
                let path = ctx.resolve_path(path);
                let client = manager
                    .get_client(&path)
                    .await
                    .map_err(lsp_err_to_tool_err)?;

                let locations = if let Some(symbol) = symbol_name {
                    client
                        .definition(&path, symbol, symbol_kind)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else if let (Some(l), Some(c)) = (line, character) {
                    let position = cocode_lsp::lsp_types_reexport::Position::new(l, c);
                    client
                        .definition_at_position(&path, position)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else {
                    return Err(tool_error::InvalidInputSnafu {
                        message: "goToDefinition requires symbolName or line+character",
                    }
                    .build());
                };

                format_locations(&filter_locations(&ignore_checker, &locations))
            }

            "findReferences" => {
                let path = require_file_path(file_path)?;
                let path = ctx.resolve_path(path);
                let client = manager
                    .get_client(&path)
                    .await
                    .map_err(lsp_err_to_tool_err)?;
                let include_declaration =
                    super::input_helpers::bool_or(&input, "includeDeclaration", true);

                let locations = if let Some(symbol) = symbol_name {
                    client
                        .references(&path, symbol, symbol_kind, include_declaration)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else if let (Some(l), Some(c)) = (line, character) {
                    let position = cocode_lsp::lsp_types_reexport::Position::new(l, c);
                    client
                        .references_at_position(&path, position, include_declaration)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else {
                    return Err(tool_error::InvalidInputSnafu {
                        message: "findReferences requires symbolName or line+character",
                    }
                    .build());
                };

                format_locations(&filter_locations(&ignore_checker, &locations))
            }

            "hover" => {
                let path = require_file_path(file_path)?;
                let path = ctx.resolve_path(path);
                let client = manager
                    .get_client(&path)
                    .await
                    .map_err(lsp_err_to_tool_err)?;

                let hover_result = if let Some(symbol) = symbol_name {
                    client
                        .hover(&path, symbol, symbol_kind)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else if let (Some(l), Some(c)) = (line, character) {
                    let position = cocode_lsp::lsp_types_reexport::Position::new(l, c);
                    client
                        .hover_at_position(&path, position)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else {
                    return Err(tool_error::InvalidInputSnafu {
                        message: "hover requires symbolName or line+character",
                    }
                    .build());
                };

                hover_result.unwrap_or_else(|| "No hover information available".to_string())
            }

            "documentSymbol" => {
                let path = require_file_path(file_path)?;
                let path = ctx.resolve_path(path);
                let client = manager
                    .get_client(&path)
                    .await
                    .map_err(lsp_err_to_tool_err)?;

                let symbols = client
                    .document_symbols(&path)
                    .await
                    .map_err(lsp_err_to_tool_err)?;

                format_document_symbols(&symbols)
            }

            "workspaceSymbol" => {
                let query = input["query"].as_str().unwrap_or("");
                // For workspace symbol, we need any file to get a client
                let path = file_path
                    .map(|p| ctx.resolve_path(p))
                    .unwrap_or(ctx.env.cwd.clone());
                let client = manager
                    .get_client(&path)
                    .await
                    .map_err(lsp_err_to_tool_err)?;

                let symbols = client
                    .workspace_symbol(query)
                    .await
                    .map_err(lsp_err_to_tool_err)?;

                format_workspace_symbols(&filter_workspace_symbols(&ignore_checker, &symbols))
            }

            "goToImplementation" => {
                let path = require_file_path(file_path)?;
                let path = ctx.resolve_path(path);
                let client = manager
                    .get_client(&path)
                    .await
                    .map_err(lsp_err_to_tool_err)?;

                let locations = if let Some(symbol) = symbol_name {
                    client
                        .implementation(&path, symbol, symbol_kind)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else if let (Some(l), Some(c)) = (line, character) {
                    let position = cocode_lsp::lsp_types_reexport::Position::new(l, c);
                    client
                        .implementation_at_position(&path, position)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else {
                    return Err(tool_error::InvalidInputSnafu {
                        message: "goToImplementation requires symbolName or line+character",
                    }
                    .build());
                };

                format_locations(&filter_locations(&ignore_checker, &locations))
            }

            "goToTypeDefinition" => {
                let path = require_file_path(file_path)?;
                let path = ctx.resolve_path(path);
                let client = manager
                    .get_client(&path)
                    .await
                    .map_err(lsp_err_to_tool_err)?;

                let locations = if let Some(symbol) = symbol_name {
                    client
                        .type_definition(&path, symbol, symbol_kind)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else if let (Some(l), Some(c)) = (line, character) {
                    let position = cocode_lsp::lsp_types_reexport::Position::new(l, c);
                    client
                        .type_definition_at_position(&path, position)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else {
                    return Err(tool_error::InvalidInputSnafu {
                        message: "goToTypeDefinition requires symbolName or line+character",
                    }
                    .build());
                };

                format_locations(&filter_locations(&ignore_checker, &locations))
            }

            "goToDeclaration" => {
                let path = require_file_path(file_path)?;
                let path = ctx.resolve_path(path);
                let client = manager
                    .get_client(&path)
                    .await
                    .map_err(lsp_err_to_tool_err)?;

                let locations = if let Some(symbol) = symbol_name {
                    client
                        .declaration(&path, symbol, symbol_kind)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else if let (Some(l), Some(c)) = (line, character) {
                    let position = cocode_lsp::lsp_types_reexport::Position::new(l, c);
                    client
                        .declaration_at_position(&path, position)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else {
                    return Err(tool_error::InvalidInputSnafu {
                        message: "goToDeclaration requires symbolName or line+character",
                    }
                    .build());
                };

                format_locations(&filter_locations(&ignore_checker, &locations))
            }

            "getCallHierarchy" => {
                let path = require_file_path(file_path)?;
                let path = ctx.resolve_path(path);
                let client = manager
                    .get_client(&path)
                    .await
                    .map_err(lsp_err_to_tool_err)?;
                let direction = input["direction"].as_str().unwrap_or("incoming");

                let items = if let Some(symbol) = symbol_name {
                    client
                        .prepare_call_hierarchy(&path, symbol, symbol_kind)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else if let (Some(l), Some(c)) = (line, character) {
                    let position = cocode_lsp::lsp_types_reexport::Position::new(l, c);
                    client
                        .prepare_call_hierarchy_at_position(&path, position)
                        .await
                        .map_err(lsp_err_to_tool_err)?
                } else {
                    return Err(tool_error::InvalidInputSnafu {
                        message: "getCallHierarchy requires symbolName or line+character",
                    }
                    .build());
                };

                if let Some(item) = items.into_iter().next() {
                    let item_name = item.name.clone();

                    match direction {
                        "incoming" => {
                            let calls = client
                                .incoming_calls(item)
                                .await
                                .map_err(lsp_err_to_tool_err)?;
                            let calls = filter_incoming_calls(&ignore_checker, &calls);
                            format_incoming_calls(&item_name, &calls)
                        }
                        "outgoing" => {
                            let calls = client
                                .outgoing_calls(item)
                                .await
                                .map_err(lsp_err_to_tool_err)?;
                            let calls = filter_outgoing_calls(&ignore_checker, &calls);
                            format_outgoing_calls(&item_name, &calls)
                        }
                        _ => {
                            return Err(tool_error::InvalidInputSnafu {
                                message: "direction must be 'incoming' or 'outgoing'",
                            }
                            .build());
                        }
                    }
                } else {
                    "No call hierarchy available for this symbol".to_string()
                }
            }

            "getDiagnostics" => {
                let path = require_file_path(file_path)?;
                let path = ctx.resolve_path(path);

                // Get diagnostics from the manager's diagnostics store
                let diagnostics = manager.diagnostics();
                let file_diagnostics = diagnostics.get_file(&path).await;

                if file_diagnostics.is_empty() {
                    "No diagnostics for this file".to_string()
                } else {
                    format_diagnostics(&file_diagnostics)
                }
            }

            _ => {
                return Err(tool_error::InvalidInputSnafu {
                    message: format!("Unknown operation: {operation}"),
                }
                .build());
            }
        };

        Ok(ToolOutput::text(result))
    }
}

fn require_file_path(file_path: Option<&str>) -> Result<&str> {
    file_path.ok_or_else(|| {
        tool_error::InvalidInputSnafu {
            message: "filePath is required for this operation",
        }
        .build()
    })
}

fn lsp_err_to_tool_err(err: cocode_lsp::LspErr) -> crate::error::ToolError {
    let message = match &err {
        cocode_lsp::LspErr::NoServerForExtension { ext } => {
            if let Some(builtin) = cocode_lsp::BuiltinServer::find_by_extension(ext) {
                format!(
                    "No LSP server configured for '{ext}'. \
                     Built-in server '{id}' supports this extension.\n\
                     Install: {hint}\n\
                     Then add to .codex/lsp_servers.json: \
                     {{ \"servers\": {{ \"{id}\": {{}} }} }}",
                    id = builtin.id,
                    hint = builtin.install_hint,
                )
            } else {
                format!(
                    "No LSP server configured for '{ext}'. \
                     No built-in server supports this extension.\n\
                     To add a custom server, create .codex/lsp_servers.json:\n\
                     {{ \"servers\": {{ \"my-server\": {{ \"command\": \"my-lsp\", \
                     \"args\": [\"--stdio\"], \"file_extensions\": [\"{ext}\"] }} }} }}"
                )
            }
        }
        _ => err.to_string(),
    };
    tool_error::ExecutionFailedSnafu { message }.build()
}

fn format_locations(locations: &[&cocode_lsp::Location]) -> String {
    if locations.is_empty() {
        return "No results found".to_string();
    }

    let mut output = String::new();
    output.push_str(&format!("Found {} location(s):\n\n", locations.len()));

    for (i, loc) in locations.iter().enumerate() {
        let path = url_to_path(&loc.uri);
        let line = loc.range.start.line + 1; // Convert to 1-indexed
        let col = loc.range.start.character + 1;
        output.push_str(&format!("{}. {}:{}:{}\n", i + 1, path, line, col));
    }

    output
}

fn format_document_symbols(symbols: &[cocode_lsp::symbols::ResolvedSymbol]) -> String {
    if symbols.is_empty() {
        return "No symbols found in this file".to_string();
    }

    let mut output = String::new();
    output.push_str(&format!("Found {} symbol(s):\n\n", symbols.len()));

    for sym in symbols {
        let kind_name = sym.kind.display_name();
        let line = sym.position.line + 1; // Convert to 1-indexed
        output.push_str(&format!("- {} {} (line {})\n", kind_name, sym.name, line));
    }

    output
}

fn format_workspace_symbols(symbols: &[&cocode_lsp::SymbolInformation]) -> String {
    if symbols.is_empty() {
        return "No symbols found matching query".to_string();
    }

    let mut output = String::new();
    output.push_str(&format!("Found {} symbol(s):\n\n", symbols.len()));

    for sym in symbols {
        let path = url_to_path(&sym.location.uri);
        let line = sym.location.range.start.line + 1; // Convert to 1-indexed
        let kind_str = format!("{:?}", sym.kind).to_lowercase();
        output.push_str(&format!(
            "- {} {} ({}:{})\n",
            kind_str, sym.name, path, line
        ));
    }

    output
}

fn format_incoming_calls(target: &str, calls: &[&cocode_lsp::CallHierarchyIncomingCall]) -> String {
    if calls.is_empty() {
        return format!("No incoming calls to '{target}'");
    }

    let mut output = String::new();
    output.push_str(&format!(
        "Incoming calls to '{}' ({} caller(s)):\n\n",
        target,
        calls.len()
    ));

    for call in calls {
        let path = url_to_path(&call.from.uri);
        let line = call.from.selection_range.start.line + 1;
        output.push_str(&format!("- {} ({}:{})\n", call.from.name, path, line));
    }

    output
}

fn format_outgoing_calls(source: &str, calls: &[&cocode_lsp::CallHierarchyOutgoingCall]) -> String {
    if calls.is_empty() {
        return format!("No outgoing calls from '{source}'");
    }

    let mut output = String::new();
    output.push_str(&format!(
        "Outgoing calls from '{}' ({} callee(s)):\n\n",
        source,
        calls.len()
    ));

    for call in calls {
        let path = url_to_path(&call.to.uri);
        let line = call.to.selection_range.start.line + 1;
        output.push_str(&format!("- {} ({}:{})\n", call.to.name, path, line));
    }

    output
}

fn format_diagnostics(diagnostics: &[cocode_lsp::DiagnosticEntry]) -> String {
    let mut output = String::new();
    output.push_str(&format!("Found {} diagnostic(s):\n\n", diagnostics.len()));

    for diag in diagnostics {
        let severity = match diag.severity {
            cocode_lsp::DiagnosticSeverityLevel::Error => "ERROR",
            cocode_lsp::DiagnosticSeverityLevel::Warning => "WARN",
            cocode_lsp::DiagnosticSeverityLevel::Info => "INFO",
            cocode_lsp::DiagnosticSeverityLevel::Hint => "HINT",
        };
        output.push_str(&format!(
            "[{}] Line {}: {}\n",
            severity, diag.line, diag.message
        ));
    }

    output
}

fn url_to_path(url: &cocode_lsp::lsp_types_reexport::Url) -> String {
    url.to_file_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| url.to_string())
}

/// Build an ignore checker for filtering LSP results.
///
/// Respects `.gitignore`, `.ignore`, global gitignore, and default
/// patterns (node_modules, .git, build dirs, etc.). Includes hidden files
/// since LSP results from dotfiles are typically intentional.
fn build_ignore_checker(cwd: &Path) -> PathChecker {
    let config = IgnoreConfig::default().with_hidden(/*include=*/ true);
    IgnoreService::new(config).create_path_checker(cwd)
}

/// Check if a URI points to an ignored file.
fn is_uri_ignored(checker: &PathChecker, uri: &cocode_lsp::lsp_types_reexport::Url) -> bool {
    uri.to_file_path()
        .map(|p| checker.is_ignored(&p))
        .unwrap_or(false) // Keep non-file URIs
}

/// Filter locations, removing those in ignored files.
fn filter_locations<'a>(
    checker: &PathChecker,
    locations: &'a [cocode_lsp::Location],
) -> Vec<&'a cocode_lsp::Location> {
    locations
        .iter()
        .filter(|loc| !is_uri_ignored(checker, &loc.uri))
        .collect()
}

/// Filter workspace symbols, removing those in ignored files.
fn filter_workspace_symbols<'a>(
    checker: &PathChecker,
    symbols: &'a [cocode_lsp::SymbolInformation],
) -> Vec<&'a cocode_lsp::SymbolInformation> {
    symbols
        .iter()
        .filter(|sym| !is_uri_ignored(checker, &sym.location.uri))
        .collect()
}

/// Filter incoming calls, removing those from ignored files.
fn filter_incoming_calls<'a>(
    checker: &PathChecker,
    calls: &'a [cocode_lsp::CallHierarchyIncomingCall],
) -> Vec<&'a cocode_lsp::CallHierarchyIncomingCall> {
    calls
        .iter()
        .filter(|call| !is_uri_ignored(checker, &call.from.uri))
        .collect()
}

/// Filter outgoing calls, removing those to ignored files.
fn filter_outgoing_calls<'a>(
    checker: &PathChecker,
    calls: &'a [cocode_lsp::CallHierarchyOutgoingCall],
) -> Vec<&'a cocode_lsp::CallHierarchyOutgoingCall> {
    calls
        .iter()
        .filter(|call| !is_uri_ignored(checker, &call.to.uri))
        .collect()
}

#[cfg(test)]
#[path = "lsp.test.rs"]
mod tests;
