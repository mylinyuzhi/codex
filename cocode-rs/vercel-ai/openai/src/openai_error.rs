use async_trait::async_trait;
use reqwest::Response;
use serde::Deserialize;
use serde_json::Value;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider_utils::ResponseHandler;

/// OpenAI error response shape.
#[derive(Debug, Deserialize)]
pub struct OpenAIErrorData {
    pub error: OpenAIErrorDetail,
}

/// Inner error detail from OpenAI API.
#[derive(Debug, Deserialize)]
pub struct OpenAIErrorDetail {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
    pub param: Option<Value>,
    pub code: Option<Value>,
}

/// Error response handler that parses OpenAI error JSON and wraps it in `AISdkError`.
pub struct OpenAIFailedResponseHandler;

#[async_trait]
impl ResponseHandler<AISdkError> for OpenAIFailedResponseHandler {
    async fn handle(
        &self,
        response: Response,
        _url: &str,
        _request_body_values: &Value,
    ) -> Result<AISdkError, AISdkError> {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("<failed to read body>"));

        let message = match serde_json::from_str::<OpenAIErrorData>(&body) {
            Ok(data) => data.error.message,
            Err(_) => {
                // Fall back to generic error message extraction
                vercel_ai_provider_utils::get_error_message(
                    &serde_json::from_str::<Value>(&body).unwrap_or(Value::String(body.clone())),
                )
            }
        };

        let _is_retryable = status.as_u16() == 429 || status.as_u16() >= 500;

        Ok(AISdkError::new(format!(
            "OpenAI API error ({status}): {message}"
        )))
    }
}

#[cfg(test)]
#[path = "openai_error.test.rs"]
mod tests;
