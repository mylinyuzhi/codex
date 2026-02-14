//! Prompt handler: produces a modified prompt from a template.
//!
//! The template can contain `$ARGUMENTS` which is replaced with the JSON
//! representation of the arguments value.
//!
//! ## Two Modes of Operation
//!
//! 1. **Template Mode** (current implementation): Simply expands `$ARGUMENTS`
//!    in the template and returns `ModifyInput`.
//!
//! 2. **LLM Verification Mode** (future): Queries an LLM to verify whether
//!    the action should proceed, expecting a JSON response like:
//!    ```json
//!    { "ok": true }
//!    { "ok": false, "reason": "Not allowed because..." }
//!    ```
//!
//! LLM verification mode requires hyper-sdk integration and will be
//! implemented when an LLM client interface is available.

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tracing::debug;

use crate::context::HookContext;
use crate::result::HookResult;

/// Response format expected from LLM verification.
///
/// When a prompt hook uses LLM verification mode, the model should return
/// a JSON object with this structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmVerificationResponse {
    /// Whether the action is approved.
    pub ok: bool,
    /// Reason for rejection (if ok is false).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Configuration for LLM-based prompt verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptVerificationConfig {
    /// System prompt to use for the verification LLM call.
    pub system_prompt: String,
    /// Model to use for verification (if None, uses default).
    pub model: Option<String>,
    /// Maximum tokens for the response.
    pub max_tokens: i32,
}

impl Default for PromptVerificationConfig {
    fn default() -> Self {
        Self {
            system_prompt: String::from(
                "You are a verification system. Analyze the request and respond with JSON: \
                 { \"ok\": true } to approve or { \"ok\": false, \"reason\": \"...\" } to reject.",
            ),
            model: None,
            max_tokens: 100,
        }
    }
}

/// Handles hooks that inject prompt templates or perform LLM verification.
pub struct PromptHandler;

impl PromptHandler {
    /// Template-mode execution: replaces `$ARGUMENTS` in the template with the
    /// serialized JSON of `arguments`, then returns a `ModifyInput` result.
    ///
    /// This is the only mode currently implemented. LLM Verification Mode
    /// (triggered when `HookHandler::Prompt { model: Some(..) }`) is not yet
    /// supported — the `model` field is silently ignored and execution always
    /// falls through to template expansion. Full LLM verification requires an
    /// `Arc<dyn Model>` (from `hyper-sdk`) or an `ApiClient` (from `core/api`)
    /// to be injected into `HookRegistry`. Use `prepare_verification_request`
    /// and `parse_verification_response` to build the full flow once LLM
    /// access is available.
    pub fn execute(template: &str, arguments: &Value) -> HookResult {
        let args_str = match serde_json::to_string(arguments) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to serialize arguments for prompt template: {e}");
                String::from("null")
            }
        };

        let expanded = template.replace("$ARGUMENTS", &args_str);
        debug!(template, expanded = %expanded, "Prompt hook expanded");

        HookResult::ModifyInput {
            new_input: Value::String(expanded),
        }
    }

    /// LLM verification mode: queries an LLM to verify the action.
    ///
    /// This method prepares the verification request but does not actually
    /// call the LLM. The caller must provide an LLM query function.
    ///
    /// # Arguments
    /// * `template` - Template with `$ARGUMENTS` placeholder
    /// * `ctx` - Hook execution context
    /// * `config` - Verification configuration
    ///
    /// # Returns
    /// A tuple of (system_prompt, user_message) to send to the LLM.
    pub fn prepare_verification_request(
        template: &str,
        ctx: &HookContext,
        config: &PromptVerificationConfig,
    ) -> (String, String) {
        let ctx_json = serde_json::to_string_pretty(ctx).unwrap_or_else(|_| "{}".to_string());
        let user_message = template.replace("$ARGUMENTS", &ctx_json);

        (config.system_prompt.clone(), user_message)
    }

    /// Parses an LLM verification response.
    ///
    /// # Arguments
    /// * `response` - The raw response text from the LLM
    ///
    /// # Returns
    /// * `HookResult::Continue` if approved
    /// * `HookResult::Reject` if rejected with reason
    pub fn parse_verification_response(response: &str) -> HookResult {
        // Try to extract JSON from the response
        let trimmed = response.trim();

        // Try to parse as-is first
        if let Ok(resp) = serde_json::from_str::<LlmVerificationResponse>(trimmed) {
            return Self::response_to_result(resp);
        }

        // Try to find JSON in the response (LLM might add explanation around it)
        if let Some(start) = trimmed.find('{')
            && let Some(end) = trimmed.rfind('}')
        {
            let json_str = &trimmed[start..=end];
            if let Ok(resp) = serde_json::from_str::<LlmVerificationResponse>(json_str) {
                return Self::response_to_result(resp);
            }
        }

        // Failed to parse - log and continue
        tracing::warn!(
            response = %response,
            "Failed to parse LLM verification response, allowing action"
        );
        HookResult::Continue
    }

    fn response_to_result(resp: LlmVerificationResponse) -> HookResult {
        if resp.ok {
            HookResult::Continue
        } else {
            HookResult::Reject {
                reason: resp
                    .reason
                    .unwrap_or_else(|| "Verification rejected by hook".to_string()),
            }
        }
    }
}

#[cfg(test)]
#[path = "prompt.test.rs"]
mod tests;
