//! LanceDB index policy and status extensions.
//!
//! Provides automatic index creation based on chunk count thresholds.
//! Also provides BM25 metadata storage for custom BM25 implementation.

use std::collections::HashMap;
use std::sync::Arc;

use arrow::array::Float32Array;
use arrow::array::Int64Array;
use arrow::array::RecordBatch;
use arrow::array::StringArray;
use arrow::datatypes::DataType;
use arrow::datatypes::Field;
use arrow::datatypes::Schema;
use lancedb::query::ExecutableQuery;
use lancedb::query::QueryBase;

use super::lancedb::LanceDbStore;
use crate::error::Result;
use crate::error::RetrievalErr;
use crate::search::Bm25Metadata;
use crate::search::SparseEmbedding;

/// Index creation policy.
///
/// Defines when and how to create vector/FTS indexes.
#[derive(Debug, Clone)]
pub struct IndexPolicy {
    /// Create vector index after N chunks (0 = never auto-create).
    pub chunk_threshold: i64,
    /// Create FTS index after N chunks (0 = never auto-create).
    pub fts_chunk_threshold: i64,
    /// Force index rebuild even if index exists.
    pub force_rebuild: bool,
}

impl Default for IndexPolicy {
    fn default() -> Self {
        Self {
            chunk_threshold: 10_000,    // 10k chunks for vector index
            fts_chunk_threshold: 1_000, // 1k chunks for FTS index
            force_rebuild: false,
        }
    }
}

impl IndexPolicy {
    /// Create a policy that never auto-creates indexes.
    pub fn never() -> Self {
        Self {
            chunk_threshold: 0,
            fts_chunk_threshold: 0,
            force_rebuild: false,
        }
    }

    /// Create a policy for immediate index creation.
    pub fn immediate() -> Self {
        Self {
            chunk_threshold: 1,
            fts_chunk_threshold: 1,
            force_rebuild: false,
        }
    }

    /// Set vector index threshold.
    pub fn with_vector_threshold(mut self, threshold: i64) -> Self {
        self.chunk_threshold = threshold;
        self
    }

    /// Set FTS index threshold.
    pub fn with_fts_threshold(mut self, threshold: i64) -> Self {
        self.fts_chunk_threshold = threshold;
        self
    }

    /// Enable force rebuild.
    pub fn with_force_rebuild(mut self) -> Self {
        self.force_rebuild = true;
        self
    }
}

/// Index status information.
#[derive(Debug, Clone, Default)]
pub struct IndexStatus {
    /// Whether table exists.
    pub table_exists: bool,
    /// Current chunk count.
    pub chunk_count: i64,
    /// Whether vector index creation is recommended.
    pub vector_index_recommended: bool,
    /// Whether FTS index creation is recommended.
    pub fts_index_recommended: bool,
}

impl IndexStatus {
    /// Check if any index creation is recommended.
    pub fn needs_indexing(&self) -> bool {
        self.vector_index_recommended || self.fts_index_recommended
    }
}

impl LanceDbStore {
    /// Get current index status.
    ///
    /// Returns information about table existence, chunk count,
    /// and whether index creation is recommended.
    pub async fn get_index_status(&self, policy: &IndexPolicy) -> Result<IndexStatus> {
        let table_exists = self.table_exists().await?;

        if !table_exists {
            return Ok(IndexStatus::default());
        }

        let chunk_count = self.count().await?;

        let vector_index_recommended =
            policy.chunk_threshold > 0 && chunk_count >= policy.chunk_threshold;

        let fts_index_recommended =
            policy.fts_chunk_threshold > 0 && chunk_count >= policy.fts_chunk_threshold;

        Ok(IndexStatus {
            table_exists,
            chunk_count,
            vector_index_recommended,
            fts_index_recommended,
        })
    }

    /// Apply index policy - create indexes if thresholds are met.
    ///
    /// Returns true if any index was created.
    ///
    /// # Arguments
    /// * `policy` - Index creation policy
    /// * `quantization_config` - Optional quantization config for vector index
    pub async fn apply_index_policy(
        &self,
        policy: &IndexPolicy,
        quantization_config: Option<&crate::config::QuantizationConfig>,
    ) -> Result<bool> {
        let status = self.get_index_status(policy).await?;

        if !status.table_exists {
            return Ok(false);
        }

        let mut created = false;

        // Create vector index if recommended
        if status.vector_index_recommended || policy.force_rebuild {
            self.create_vector_index_with_config(quantization_config)
                .await?;
            created = true;
        }

        // Create FTS index if recommended
        if status.fts_index_recommended || policy.force_rebuild {
            self.create_fts_index().await?;
            created = true;
        }

        Ok(created)
    }

    /// Check if index creation is needed based on policy.
    pub async fn needs_index(&self, policy: &IndexPolicy) -> Result<bool> {
        let status = self.get_index_status(policy).await?;
        Ok(status.needs_indexing())
    }
}

/// Configuration for index policy from TOML.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct IndexPolicyConfig {
    /// Create vector index after N chunks (0 = never).
    #[serde(default = "default_chunk_threshold")]
    pub chunk_threshold: i64,

    /// Create FTS index after N chunks (0 = never).
    #[serde(default = "default_fts_chunk_threshold")]
    pub fts_chunk_threshold: i64,
}

fn default_chunk_threshold() -> i64 {
    10_000
}

fn default_fts_chunk_threshold() -> i64 {
    1_000
}

impl From<&IndexPolicyConfig> for IndexPolicy {
    fn from(config: &IndexPolicyConfig) -> Self {
        Self {
            chunk_threshold: config.chunk_threshold,
            fts_chunk_threshold: config.fts_chunk_threshold,
            force_rebuild: false,
        }
    }
}

// ============================================================================
// BM25 Metadata Storage
// ============================================================================

const BM25_METADATA_TABLE: &str = "bm25_metadata";

impl LanceDbStore {
    /// Get the schema for BM25 metadata table.
    fn bm25_metadata_schema() -> Schema {
        Schema::new(vec![
            Field::new("avgdl", DataType::Float32, false),
            Field::new("total_docs", DataType::Int64, false),
            Field::new("updated_at", DataType::Int64, false),
        ])
    }

    /// Check if BM25 metadata table exists.
    pub async fn bm25_metadata_exists(&self) -> Result<bool> {
        let tables = self.db().table_names().execute().await.map_err(|e| {
            RetrievalErr::LanceDbQueryFailed {
                table: BM25_METADATA_TABLE.to_string(),
                cause: e.to_string(),
            }
        })?;
        Ok(tables.contains(&BM25_METADATA_TABLE.to_string()))
    }

    /// Save BM25 metadata.
    ///
    /// Creates or replaces the metadata record.
    pub async fn save_bm25_metadata(&self, metadata: &Bm25Metadata) -> Result<()> {
        let schema = Arc::new(Self::bm25_metadata_schema());

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Float32Array::from(vec![metadata.avgdl])),
                Arc::new(Int64Array::from(vec![metadata.total_docs])),
                Arc::new(Int64Array::from(vec![metadata.updated_at])),
            ],
        )
        .map_err(|e| RetrievalErr::LanceDbQueryFailed {
            table: BM25_METADATA_TABLE.to_string(),
            cause: e.to_string(),
        })?;

        let reader = arrow::record_batch::RecordBatchIterator::new(vec![Ok(batch)], schema.clone());

        // Drop existing table and recreate (simple upsert strategy)
        if self.bm25_metadata_exists().await? {
            self.db()
                .drop_table(BM25_METADATA_TABLE, &[])
                .await
                .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                    table: BM25_METADATA_TABLE.to_string(),
                    cause: e.to_string(),
                })?;
        }

        self.db()
            .create_table(BM25_METADATA_TABLE, reader)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: BM25_METADATA_TABLE.to_string(),
                cause: e.to_string(),
            })?;

        Ok(())
    }

    /// Load BM25 metadata.
    pub async fn load_bm25_metadata(&self) -> Result<Option<Bm25Metadata>> {
        if !self.bm25_metadata_exists().await? {
            return Ok(None);
        }

        let table = self
            .db()
            .open_table(BM25_METADATA_TABLE)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: BM25_METADATA_TABLE.to_string(),
                cause: e.to_string(),
            })?;

        let mut stream = table.query().limit(1).execute().await.map_err(|e| {
            RetrievalErr::LanceDbQueryFailed {
                table: BM25_METADATA_TABLE.to_string(),
                cause: e.to_string(),
            }
        })?;

        use futures::StreamExt;
        if let Some(batch_result) = stream.next().await {
            let batch = batch_result.map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: BM25_METADATA_TABLE.to_string(),
                cause: e.to_string(),
            })?;

            if batch.num_rows() == 0 {
                return Ok(None);
            }

            let avgdl = batch
                .column(0)
                .as_any()
                .downcast_ref::<Float32Array>()
                .map(|a| a.value(0))
                .unwrap_or(100.0);

            let total_docs = batch
                .column(1)
                .as_any()
                .downcast_ref::<Int64Array>()
                .map(|a| a.value(0))
                .unwrap_or(0);

            let updated_at = batch
                .column(2)
                .as_any()
                .downcast_ref::<Int64Array>()
                .map(|a| a.value(0))
                .unwrap_or(0);

            return Ok(Some(Bm25Metadata {
                avgdl,
                total_docs,
                updated_at,
            }));
        }

        Ok(None)
    }

    /// Load all BM25 embeddings from chunks.
    ///
    /// Returns a map of chunk_id -> SparseEmbedding.
    /// Embeddings are stored as JSON strings in the bm25_embedding column.
    pub async fn load_all_bm25_embeddings(&self) -> Result<HashMap<String, SparseEmbedding>> {
        let mut result = HashMap::new();

        if !self.table_exists().await? {
            return Ok(result);
        }

        let table = self
            .db()
            .open_table(self.table_name())
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name().to_string(),
                cause: e.to_string(),
            })?;

        // Select only id and bm25_embedding columns
        let mut stream = table
            .query()
            .select(lancedb::query::Select::Columns(vec![
                "id".to_string(),
                "bm25_embedding".to_string(),
            ]))
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name().to_string(),
                cause: e.to_string(),
            })?;

        use futures::StreamExt;
        while let Some(batch_result) = stream.next().await {
            let batch = batch_result.map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name().to_string(),
                cause: e.to_string(),
            })?;

            let ids = batch
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| RetrievalErr::LanceDbQueryFailed {
                    table: self.table_name().to_string(),
                    cause: "Invalid id column".to_string(),
                })?;

            let embeddings = batch
                .column_by_name("bm25_embedding")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>());

            if let Some(embeddings_col) = embeddings {
                for i in 0..batch.num_rows() {
                    let id = ids.value(i).to_string();
                    let json = embeddings_col.value(i);
                    if !json.is_empty() {
                        if let Some(embedding) = SparseEmbedding::from_json(json) {
                            result.insert(id, embedding);
                        }
                    }
                }
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy() {
        let policy = IndexPolicy::default();
        assert_eq!(policy.chunk_threshold, 10_000);
        assert_eq!(policy.fts_chunk_threshold, 1_000);
        assert!(!policy.force_rebuild);
    }

    #[test]
    fn test_never_policy() {
        let policy = IndexPolicy::never();
        assert_eq!(policy.chunk_threshold, 0);
        assert_eq!(policy.fts_chunk_threshold, 0);
    }

    #[test]
    fn test_immediate_policy() {
        let policy = IndexPolicy::immediate();
        assert_eq!(policy.chunk_threshold, 1);
        assert_eq!(policy.fts_chunk_threshold, 1);
    }

    #[test]
    fn test_policy_builder() {
        let policy = IndexPolicy::default()
            .with_vector_threshold(5_000)
            .with_fts_threshold(500)
            .with_force_rebuild();

        assert_eq!(policy.chunk_threshold, 5_000);
        assert_eq!(policy.fts_chunk_threshold, 500);
        assert!(policy.force_rebuild);
    }

    #[test]
    fn test_index_status_needs_indexing() {
        let status = IndexStatus {
            table_exists: true,
            chunk_count: 15_000,
            vector_index_recommended: true,
            fts_index_recommended: false,
        };
        assert!(status.needs_indexing());

        let status = IndexStatus::default();
        assert!(!status.needs_indexing());
    }

    #[test]
    fn test_config_to_policy() {
        let config = IndexPolicyConfig {
            chunk_threshold: 20_000,
            fts_chunk_threshold: 2_000,
        };

        let policy = IndexPolicy::from(&config);
        assert_eq!(policy.chunk_threshold, 20_000);
        assert_eq!(policy.fts_chunk_threshold, 2_000);
        assert!(!policy.force_rebuild);
    }
}
