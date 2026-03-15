//! Google Generative AI video model settings.

use serde::Deserialize;
use serde::Serialize;

/// Settings for the Google video generation model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleGenerativeAIVideoSettings {
    // Video model settings can be extended as needed.
}
