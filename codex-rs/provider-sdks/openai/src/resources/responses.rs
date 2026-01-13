//! Responses resource for the OpenAI API.

use crate::client::Client;
use crate::error::Result;
use crate::types::Response;
use crate::types::ResponseCreateParams;

/// Responses resource for creating API responses.
pub struct Responses<'a> {
    client: &'a Client,
}

impl<'a> Responses<'a> {
    /// Create a new Responses resource.
    pub(crate) fn new(client: &'a Client) -> Self {
        Self { client }
    }

    /// Create a response (non-streaming).
    ///
    /// # Arguments
    ///
    /// * `params` - The parameters for creating the response
    ///
    /// # Returns
    ///
    /// The API response containing generated content.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use openai_sdk::{Client, ResponseCreateParams, InputMessage};
    ///
    /// let client = Client::from_env()?;
    /// let params = ResponseCreateParams::new("gpt-4o", vec![
    ///     InputMessage::user_text("Hello!")
    /// ]);
    ///
    /// let response = client.responses().create(params).await?;
    /// println!("Response: {}", response.text());
    /// ```
    pub async fn create(&self, params: ResponseCreateParams) -> Result<Response> {
        let body = serde_json::to_value(&params)?;
        self.client.post_response("/responses", body).await
    }

    /// Retrieve a response by ID.
    ///
    /// Use this to check the status of a background response or fetch
    /// a previously created response.
    ///
    /// # Arguments
    ///
    /// * `response_id` - The ID of the response to retrieve
    ///
    /// # Returns
    ///
    /// The response with current status and output.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use openai_sdk::Client;
    ///
    /// let client = Client::from_env()?;
    /// let response = client.responses().retrieve("resp-abc123").await?;
    /// println!("Status: {:?}", response.status);
    /// ```
    pub async fn retrieve(&self, response_id: impl AsRef<str>) -> Result<Response> {
        let path = format!("/responses/{}", response_id.as_ref());
        self.client.get_response(&path).await
    }

    /// Cancel a background response.
    ///
    /// Only works for responses created with `background: true` that are
    /// still in progress.
    ///
    /// # Arguments
    ///
    /// * `response_id` - The ID of the response to cancel
    ///
    /// # Returns
    ///
    /// The cancelled response.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use openai_sdk::Client;
    ///
    /// let client = Client::from_env()?;
    /// let response = client.responses().cancel("resp-abc123").await?;
    /// assert_eq!(response.status, ResponseStatus::Cancelled);
    /// ```
    pub async fn cancel(&self, response_id: impl AsRef<str>) -> Result<Response> {
        let path = format!("/responses/{}/cancel", response_id.as_ref());
        self.client
            .post_response(&path, serde_json::json!({}))
            .await
    }
}
