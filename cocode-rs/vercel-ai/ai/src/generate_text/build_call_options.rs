//! Shared call options builder.
//!
//! This module consolidates the duplicated call options building logic
//! that was in both `generate_text.rs` and `stream_text.rs`.

use tokio_util::sync::CancellationToken;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::LanguageModelV4Prompt;
use vercel_ai_provider::LanguageModelV4Tool;
use vercel_ai_provider::LanguageModelV4ToolChoice;

use crate::prompt::CallSettings;
use crate::types::ProviderOptions;

use super::output::Output;

/// Apply `CallSettings` fields to existing call options.
///
/// This mutates the provided `call_options` in place, setting max_tokens,
/// temperature, top_p, top_k, stop_sequences, frequency_penalty,
/// presence_penalty, seed, headers, and abort_signal from the settings.
pub fn apply_call_settings(
    call_options: &mut LanguageModelV4CallOptions,
    settings: &CallSettings,
    abort_signal: &Option<CancellationToken>,
) {
    if let Some(max_tokens) = settings.max_tokens {
        call_options.max_output_tokens = Some(max_tokens);
    }
    if let Some(temp) = settings.temperature {
        call_options.temperature = Some(temp);
    }
    if let Some(top_p) = settings.top_p {
        call_options.top_p = Some(top_p);
    }
    if let Some(top_k) = settings.top_k {
        call_options.top_k = Some(top_k);
    }
    if let Some(ref stop) = settings.stop_sequences {
        call_options.stop_sequences = Some(stop.clone());
    }
    if let Some(freq_penalty) = settings.frequency_penalty {
        call_options.frequency_penalty = Some(freq_penalty);
    }
    if let Some(pres_penalty) = settings.presence_penalty {
        call_options.presence_penalty = Some(pres_penalty);
    }
    if let Some(seed) = settings.seed {
        call_options.seed = Some(seed);
    }
    if let Some(ref headers) = settings.headers {
        call_options.headers = Some(headers.clone());
    }
    if let Some(signal) = abort_signal {
        call_options.abort_signal = Some(signal.clone());
    }
}

/// Build `LanguageModelV4CallOptions` from the shared set of parameters.
///
/// This function applies all settings fields (max_tokens, temperature, top_p,
/// top_k, stop_sequences, frequency_penalty, presence_penalty, seed, headers),
/// tools, tool_choice, abort_signal, provider_options, and output/response_format.
#[allow(clippy::too_many_arguments)]
pub fn build_call_options(
    settings: &CallSettings,
    tool_choice: &Option<LanguageModelV4ToolChoice>,
    abort_signal: &Option<CancellationToken>,
    provider_options: &Option<ProviderOptions>,
    output: &Option<Output>,
    messages: LanguageModelV4Prompt,
    tool_definitions: &Option<Vec<LanguageModelV4Tool>>,
) -> LanguageModelV4CallOptions {
    let mut call_options = LanguageModelV4CallOptions::new(messages);

    // Apply all settings + abort signal
    apply_call_settings(&mut call_options, settings, abort_signal);

    // Add tools
    if let Some(defs) = tool_definitions {
        call_options.tools = Some(defs.clone());
    }
    if let Some(choice) = tool_choice {
        call_options.tool_choice = Some(choice.clone());
    }

    // Add provider options (from CallSettings first, then from explicit options)
    let effective_provider_options = provider_options
        .as_ref()
        .or(settings.provider_options.as_ref());
    if let Some(opts) = effective_provider_options {
        call_options.provider_options = Some(opts.clone());
    }

    // Add response format for structured output
    if let Some(out) = output {
        call_options.response_format = Some(out.to_response_format());
    }

    call_options
}

/// Filter tool definitions to only include active tools.
pub fn filter_active_tools(
    tool_definitions: &Option<Vec<LanguageModelV4Tool>>,
    active_tools: &Option<Vec<String>>,
) -> Option<Vec<LanguageModelV4Tool>> {
    match (tool_definitions, active_tools) {
        (Some(defs), Some(active)) => {
            let filtered: Vec<LanguageModelV4Tool> = defs
                .iter()
                .filter(|d| active.iter().any(|name| d.name() == name))
                .cloned()
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some(filtered)
            }
        }
        (Some(defs), None) => Some(defs.clone()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "build_call_options.test.rs"]
mod tests;
