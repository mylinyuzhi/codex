//! Google Generative AI embedding model options.

use serde::Deserialize;
use serde::Serialize;

/// Embedding task type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EmbeddingTaskType {
    RetrievalQuery,
    RetrievalDocument,
    SemanticSimilarity,
    Classification,
    Clustering,
    QuestionAnswering,
    FactVerification,
    CodeRetrievalQuery,
}

/// Google embedding model options (provider-specific).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleEmbeddingModelOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_dimensionality: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_type: Option<EmbeddingTaskType>,
}
