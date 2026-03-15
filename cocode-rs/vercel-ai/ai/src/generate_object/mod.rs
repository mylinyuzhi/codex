//! Generate structured object from a prompt.
//!
//! This module provides `generate_object` and `stream_object` functions
//! for generating structured output that conforms to a JSON schema.

mod inject_json_instruction;
mod output_strategy;
mod parse_validate;
mod repair_text;
mod validate_input;

use std::pin::Pin;
use std::sync::Arc;

use futures::StreamExt;
use serde::de::DeserializeOwned;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::JSONSchema;
use vercel_ai_provider::LanguageModelV4CallOptions;
use vercel_ai_provider::ResponseFormat;
use vercel_ai_provider::Usage;

use crate::error::AIError;
use crate::model::LanguageModel;
use crate::model::resolve_language_model;
use crate::prompt::CallSettings;
use crate::prompt::Prompt;
use crate::types::ProviderOptions;

pub use inject_json_instruction::inject_json_instruction;
pub use inject_json_instruction::inject_json_instruction_with_options;
pub use output_strategy::ObjectOutputStrategy as OutputStrategy;
pub use parse_validate::ParsedObjectResult;
pub use parse_validate::parse_and_validate;
pub use parse_validate::parse_json_value;
pub use parse_validate::validate_against_schema;
pub use repair_text::RepairTextFunction;
pub use repair_text::repair_json_text;
pub use validate_input::determine_generation_mode;
pub use validate_input::validate_object_generation_input;

/// Options for `generate_object`.
#[derive(Default)]
pub struct GenerateObjectOptions<T> {
    /// The model to use.
    pub model: LanguageModel,
    /// The prompt to send to the model.
    pub prompt: Prompt,
    /// The JSON schema for the output.
    pub schema: JSONSchema,
    /// Optional name for the schema.
    pub schema_name: Option<String>,
    /// Optional description for the schema.
    pub schema_description: Option<String>,
    /// The mode for structured output.
    pub mode: ObjectGenerationMode,
    /// Call settings.
    pub settings: CallSettings,
    /// Abort signal for cancellation.
    pub abort_signal: Option<CancellationToken>,
    /// Maximum retries for validation failures.
    pub max_retries: Option<u32>,
    /// Provider-specific options.
    pub provider_options: Option<ProviderOptions>,
    /// Callback called when generation finishes.
    #[allow(clippy::type_complexity)]
    pub on_finish: Option<Arc<dyn Fn(&GenerateObjectFinishEvent) + Send + Sync>>,
    /// Phantom data for the output type.
    _phantom: std::marker::PhantomData<T>,
}

/// Event data for the on_finish callback in generate_object.
#[derive(Debug, Clone)]
pub struct GenerateObjectFinishEvent {
    /// Token usage.
    pub usage: Usage,
    /// The finish reason.
    pub finish_reason: FinishReason,
    /// The raw JSON string.
    pub raw: String,
    /// Warnings from the provider.
    pub warnings: Vec<vercel_ai_provider::Warning>,
}

/// Mode for generating structured output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ObjectGenerationMode {
    /// Auto - let the SDK choose the best mode.
    #[default]
    Auto,
    /// JSON mode - request JSON output but no schema validation.
    Json,
    /// Tool mode - use tool calling for structured output.
    Tool,
    /// Grammar mode - use grammar-constrained generation.
    Grammar,
}

impl<T> GenerateObjectOptions<T> {
    /// Create new options with a model, prompt, and schema.
    pub fn new(
        model: impl Into<LanguageModel>,
        prompt: impl Into<Prompt>,
        schema: JSONSchema,
    ) -> Self {
        Self {
            model: model.into(),
            prompt: prompt.into(),
            schema,
            schema_name: None,
            schema_description: None,
            mode: ObjectGenerationMode::Auto,
            settings: CallSettings::default(),
            abort_signal: None,
            max_retries: None,
            provider_options: None,
            on_finish: None,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Set the schema name.
    pub fn with_schema_name(mut self, name: impl Into<String>) -> Self {
        self.schema_name = Some(name.into());
        self
    }

    /// Set the schema description.
    pub fn with_schema_description(mut self, description: impl Into<String>) -> Self {
        self.schema_description = Some(description.into());
        self
    }

    /// Set the generation mode.
    pub fn with_mode(mut self, mode: ObjectGenerationMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the call settings.
    pub fn with_settings(mut self, settings: CallSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Set the abort signal.
    pub fn with_abort_signal(mut self, signal: CancellationToken) -> Self {
        self.abort_signal = Some(signal);
        self
    }

    /// Set the maximum retries.
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = Some(max_retries);
        self
    }

    /// Set provider-specific options.
    pub fn with_provider_options(mut self, options: ProviderOptions) -> Self {
        self.provider_options = Some(options);
        self
    }

    /// Set the on_finish callback.
    pub fn with_on_finish<F>(mut self, callback: F) -> Self
    where
        F: Fn(&GenerateObjectFinishEvent) + Send + Sync + 'static,
    {
        self.on_finish = Some(Arc::new(callback));
        self
    }
}

/// Result of `generate_object`.
#[derive(Debug)]
pub struct GenerateObjectResult<T> {
    /// The generated object.
    pub object: T,
    /// The raw JSON string.
    pub raw: String,
    /// Token usage.
    pub usage: Usage,
    /// The finish reason.
    pub finish_reason: FinishReason,
    /// Warnings.
    pub warnings: Vec<vercel_ai_provider::Warning>,
}

impl<T> GenerateObjectResult<T> {
    /// Create a new generate object result.
    pub fn new(object: T, raw: String, usage: Usage, finish_reason: FinishReason) -> Self {
        Self {
            object,
            raw,
            usage,
            finish_reason,
            warnings: Vec::new(),
        }
    }

    /// Add warnings.
    pub fn with_warnings(mut self, warnings: Vec<vercel_ai_provider::Warning>) -> Self {
        self.warnings = warnings;
        self
    }
}

/// Generate a structured object from a prompt.
///
/// This function generates structured output that conforms to a JSON schema.
///
/// # Arguments
///
/// * `options` - The generation options including model, prompt, and schema.
///
/// # Returns
///
/// A `GenerateObjectResult<T>` containing the parsed object.
///
/// # Example
///
/// ```ignore
/// use vercel_ai::{generate_object, GenerateObjectOptions};
/// use serde::Deserialize;
///
/// #[derive(Deserialize)]
/// struct Person {
///     name: String,
///     age: u32,
/// }
///
/// let schema = serde_json::json!({
///     "type": "object",
///     "properties": {
///         "name": { "type": "string" },
///         "age": { "type": "integer" }
///     },
///     "required": ["name", "age"]
/// });
///
/// let result: GenerateObjectResult<Person> = generate_object(GenerateObjectOptions {
///     model: "gpt-4".into(),
///     prompt: Prompt::user("Generate a person"),
///     schema,
///     ..Default::default()
/// }).await?;
///
/// println!("Name: {}, Age: {}", result.object.name, result.object.age);
/// ```
pub async fn generate_object<T: DeserializeOwned>(
    options: GenerateObjectOptions<T>,
) -> Result<GenerateObjectResult<T>, AIError> {
    // Resolve the model
    let model = resolve_language_model(options.model)?;

    // Build the prompt
    let messages = options.prompt.to_model_prompt();

    // Build the response format
    let mut response_format = ResponseFormat::json_with_schema(options.schema.clone()).with_name(
        options
            .schema_name
            .clone()
            .unwrap_or_else(|| "output".to_string()),
    );

    if let Some(desc) = options.schema_description {
        response_format = response_format.with_description(desc);
    }

    // Build call options
    let mut call_options = LanguageModelV4CallOptions::new(messages);
    call_options.response_format = Some(response_format);

    // Apply settings
    if let Some(max_tokens) = options.settings.max_tokens {
        call_options.max_output_tokens = Some(max_tokens);
    }
    if let Some(temp) = options.settings.temperature {
        call_options.temperature = Some(temp);
    }
    if let Some(ref signal) = options.abort_signal {
        call_options.abort_signal = Some(signal.clone());
    }

    // Apply provider options
    if let Some(ref provider_opts) = options.provider_options {
        call_options.provider_options = Some(provider_opts.clone());
    }

    // Call the model
    let result = model.do_generate(call_options).await?;

    // Extract the text content
    let raw = extract_text(&result.content);

    // Parse the JSON
    let object: T = serde_json::from_str(&raw)
        .map_err(|e| AIError::SchemaValidation(format!("Failed to parse JSON: {e}")))?;

    let gen_result = GenerateObjectResult::new(
        object,
        raw.clone(),
        result.usage.clone(),
        result.finish_reason.clone(),
    )
    .with_warnings(result.warnings.clone());

    // Call on_finish callback
    if let Some(ref callback) = options.on_finish {
        callback(&GenerateObjectFinishEvent {
            usage: result.usage,
            finish_reason: result.finish_reason,
            raw,
            warnings: result.warnings,
        });
    }

    Ok(gen_result)
}

/// Extract text from content parts.
fn extract_text(content: &[vercel_ai_provider::AssistantContentPart]) -> String {
    content
        .iter()
        .filter_map(|part| match part {
            vercel_ai_provider::AssistantContentPart::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// A part of the object stream.
#[derive(Debug)]
pub enum ObjectStreamPart<T> {
    /// Partial object delta (for streaming).
    ObjectDelta {
        /// The partial object.
        delta: serde_json::Value,
    },
    /// Complete object.
    Object {
        /// The complete object.
        object: T,
    },
    /// Error occurred.
    Error {
        /// The error.
        error: AIError,
    },
    /// Finish event.
    Finish {
        /// Token usage.
        usage: Usage,
    },
}

/// Result of `stream_object`.
pub struct StreamObjectResult<T> {
    /// The stream of object parts.
    pub stream: Pin<Box<dyn futures::Stream<Item = ObjectStreamPart<T>> + Send>>,
}

impl<T> StreamObjectResult<T> {
    /// Collect the stream into a final object.
    pub async fn into_object(mut self) -> Result<T, AIError>
    where
        T: DeserializeOwned,
    {
        while let Some(part) = self.stream.next().await {
            match part {
                ObjectStreamPart::Object { object } => return Ok(object),
                ObjectStreamPart::Error { error } => return Err(error),
                _ => {}
            }
        }
        Err(AIError::NoOutputGenerated)
    }
}

/// Stream a structured object from a prompt.
///
/// This function streams structured output generation.
///
/// # Arguments
///
/// * `options` - The streaming options including model, prompt, and schema.
///
/// # Returns
///
/// A `StreamObjectResult<T>` containing the stream of object parts.
pub fn stream_object<T: DeserializeOwned + Send + 'static>(
    options: GenerateObjectOptions<T>,
) -> StreamObjectResult<T> {
    let (tx, rx) = tokio::sync::mpsc::channel(100);

    tokio::spawn(async move {
        if let Err(e) = stream_object_inner(options, tx.clone()).await {
            let _ = tx.send(ObjectStreamPart::Error { error: e }).await;
        }
    });

    let stream = ReceiverStream::new(rx);
    StreamObjectResult {
        stream: Box::pin(stream),
    }
}

async fn stream_object_inner<T: DeserializeOwned + Send + 'static>(
    options: GenerateObjectOptions<T>,
    tx: tokio::sync::mpsc::Sender<ObjectStreamPart<T>>,
) -> Result<(), AIError> {
    // Resolve the model
    let model = resolve_language_model(options.model)?;

    // Build the prompt
    let messages = options.prompt.to_model_prompt();

    // Build the response format
    let response_format = ResponseFormat::json_with_schema(options.schema.clone()).with_name(
        options
            .schema_name
            .clone()
            .unwrap_or_else(|| "output".to_string()),
    );

    // Build call options
    let mut call_options = LanguageModelV4CallOptions::new(messages);
    call_options.response_format = Some(response_format);

    // Apply settings
    if let Some(max_tokens) = options.settings.max_tokens {
        call_options.max_output_tokens = Some(max_tokens);
    }
    if let Some(temp) = options.settings.temperature {
        call_options.temperature = Some(temp);
    }
    if let Some(ref signal) = options.abort_signal {
        call_options.abort_signal = Some(signal.clone());
    }

    // Call the model
    let stream_result = model.do_stream(call_options).await?;

    // Process the stream
    let mut full_text = String::new();
    let mut usage = Usage::default();

    use futures::StreamExt;
    let mut stream = stream_result.stream;

    while let Some(part_result) = stream.next().await {
        match part_result {
            Ok(part) => match part {
                vercel_ai_provider::LanguageModelV4StreamPart::TextDelta { delta, .. } => {
                    full_text.push_str(&delta);

                    // Try to parse partial JSON for delta updates
                    if let Ok(partial) = parse_partial_json(&full_text) {
                        let _ = tx
                            .send(ObjectStreamPart::ObjectDelta { delta: partial })
                            .await;
                    }
                }
                vercel_ai_provider::LanguageModelV4StreamPart::Finish { usage: u, .. } => {
                    usage = u;
                }
                _ => {}
            },
            Err(e) => {
                let _ = tx
                    .send(ObjectStreamPart::Error {
                        error: AIError::ProviderError(e),
                    })
                    .await;
                return Err(AIError::ProviderError(AISdkError::new("Stream error")));
            }
        }
    }

    // Parse the final object
    match serde_json::from_str::<T>(&full_text) {
        Ok(object) => {
            let _ = tx.send(ObjectStreamPart::Object { object }).await;
            let _ = tx.send(ObjectStreamPart::Finish { usage }).await;
        }
        Err(e) => {
            let _ = tx
                .send(ObjectStreamPart::Error {
                    error: AIError::SchemaValidation(format!("Failed to parse JSON: {e}")),
                })
                .await;
        }
    }

    Ok(())
}

/// Parse partial JSON (best effort for streaming).
fn parse_partial_json(text: &str) -> Result<serde_json::Value, ()> {
    // Try to parse as-is first
    if let Ok(v) = serde_json::from_str(text) {
        return Ok(v);
    }

    // Try to complete partial JSON
    let mut chars: Vec<char> = text.chars().collect();

    // Count open braces/brackets
    let mut open_braces = 0i32;
    let mut open_brackets = 0i32;
    let mut in_string = false;
    let mut escape_next = false;

    for &c in &chars {
        if escape_next {
            escape_next = false;
            continue;
        }
        match c {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => open_braces += 1,
            '}' if !in_string => open_braces -= 1,
            '[' if !in_string => open_brackets += 1,
            ']' if !in_string => open_brackets -= 1,
            _ => {}
        }
    }

    // If we're in a string, close it
    if in_string {
        chars.push('"');
    }

    // Close open structures
    #[allow(clippy::same_item_push)]
    for _ in 0..open_brackets {
        chars.push(']');
    }
    #[allow(clippy::same_item_push)]
    for _ in 0..open_braces {
        chars.push('}');
    }

    let completed: String = chars.into_iter().collect();
    serde_json::from_str(&completed).map_err(|_| ())
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
