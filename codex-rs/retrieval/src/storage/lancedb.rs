//! LanceDB storage layer.
//!
//! Provides vector storage and full-text search using LanceDB.
//! Extended schema includes file metadata for incremental indexing.

use std::path::Path;
use std::sync::Arc;

use arrow::array::Array;
use arrow::array::BooleanArray;
use arrow::array::FixedSizeListArray;
use arrow::array::Float32Array;
use arrow::array::Int32Array;
use arrow::array::Int64Array;
use arrow::array::RecordBatch;
use arrow::array::StringArray;
use arrow::datatypes::DataType;
use arrow::datatypes::Field;
use arrow::datatypes::Schema;
use lance_index::scalar::FullTextSearchQuery;
use lancedb::Table;
use lancedb::connection::Connection;
use lancedb::query::ExecutableQuery;
use lancedb::query::QueryBase;

use crate::config::default_embedding_dimension;
use crate::error::Result;
use crate::error::RetrievalErr;
use crate::types::CodeChunk;

/// LanceDB store for code chunks and vectors.
pub struct LanceDbStore {
    db: Arc<Connection>,
    table_name: String,
    dimension: i32,
}

impl LanceDbStore {
    /// Open or create a LanceDB database.
    pub async fn open(path: &Path) -> Result<Self> {
        Self::open_with_dimension(path, default_embedding_dimension()).await
    }

    /// Open or create a LanceDB database with custom dimension.
    pub async fn open_with_dimension(path: &Path, dimension: i32) -> Result<Self> {
        let uri = path.to_string_lossy().to_string();
        let db = lancedb::connect(&uri).execute().await.map_err(|e| {
            RetrievalErr::LanceDbConnectionFailed {
                uri: uri.clone(),
                cause: e.to_string(),
            }
        })?;

        Ok(Self {
            db: Arc::new(db),
            table_name: "code_chunks".to_string(),
            dimension,
        })
    }

    /// Get the Arrow schema for the chunks table.
    ///
    /// Extended schema includes metadata for incremental indexing:
    /// - workspace: workspace identifier
    /// - content_hash: SHA256 hash of file content
    /// - mtime: file modification timestamp
    /// - indexed_at: when the chunk was indexed
    /// - parent_symbol: parent class/struct/impl context
    fn get_schema(&self) -> Schema {
        Schema::new(vec![
            // Core chunk fields
            Field::new("id", DataType::Utf8, false),
            Field::new("source_id", DataType::Utf8, false),
            Field::new("filepath", DataType::Utf8, false),
            Field::new("language", DataType::Utf8, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("start_line", DataType::Int32, false),
            Field::new("end_line", DataType::Int32, false),
            Field::new(
                "embedding",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, false)),
                    self.dimension,
                ),
                true, // nullable for chunks without embeddings
            ),
            // Extended metadata fields for incremental indexing
            Field::new("workspace", DataType::Utf8, false),
            Field::new("content_hash", DataType::Utf8, false),
            Field::new("mtime", DataType::Int64, false),
            Field::new("indexed_at", DataType::Int64, false),
            // Parent symbol context (nullable)
            Field::new("parent_symbol", DataType::Utf8, true),
            // Is overview chunk (nullable, defaults to false for backward compatibility)
            Field::new("is_overview", DataType::Boolean, true),
        ])
    }

    /// Check if the chunks table exists.
    pub async fn table_exists(&self) -> Result<bool> {
        let tables = self.db.table_names().execute().await.map_err(|e| {
            RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            }
        })?;

        Ok(tables.contains(&self.table_name))
    }

    /// Get or create the chunks table.
    async fn get_or_create_table(&self) -> Result<Table> {
        if self.table_exists().await? {
            self.db
                .open_table(&self.table_name)
                .execute()
                .await
                .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                    table: self.table_name.clone(),
                    cause: e.to_string(),
                })
        } else {
            // Create empty table with schema
            let schema = Arc::new(self.get_schema());
            let empty_batch = RecordBatch::new_empty(schema.clone());
            let reader =
                arrow::record_batch::RecordBatchIterator::new(vec![Ok(empty_batch)], schema);

            self.db
                .create_table(&self.table_name, reader)
                .execute()
                .await
                .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                    table: self.table_name.clone(),
                    cause: e.to_string(),
                })
        }
    }

    /// Convert chunks to Arrow RecordBatch.
    fn chunks_to_batch(&self, chunks: &[CodeChunk]) -> Result<RecordBatch> {
        // Core chunk fields
        let ids: Vec<&str> = chunks.iter().map(|c| c.id.as_str()).collect();
        let source_ids: Vec<&str> = chunks.iter().map(|c| c.source_id.as_str()).collect();
        let filepaths: Vec<&str> = chunks.iter().map(|c| c.filepath.as_str()).collect();
        let languages: Vec<&str> = chunks.iter().map(|c| c.language.as_str()).collect();
        let contents: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
        let start_lines: Vec<i32> = chunks.iter().map(|c| c.start_line).collect();
        let end_lines: Vec<i32> = chunks.iter().map(|c| c.end_line).collect();

        // Build embeddings array
        let embedding_values: Vec<Option<Vec<f32>>> = chunks
            .iter()
            .map(|c| {
                c.embedding.as_ref().map(|e| {
                    // Pad or truncate to dimension
                    let mut vec = e.clone();
                    vec.resize(self.dimension as usize, 0.0);
                    vec
                })
            })
            .collect();

        let embedding_array = self.build_embedding_array(&embedding_values)?;

        // Extended metadata fields
        let workspaces: Vec<&str> = chunks
            .iter()
            .map(|c| {
                if c.workspace.is_empty() {
                    c.source_id.as_str()
                } else {
                    c.workspace.as_str()
                }
            })
            .collect();
        let content_hashes: Vec<&str> = chunks.iter().map(|c| c.content_hash.as_str()).collect();
        let mtimes: Vec<i64> = chunks
            .iter()
            .map(|c| c.modified_time.unwrap_or(0))
            .collect();
        let indexed_ats: Vec<i64> = chunks
            .iter()
            .map(|c| {
                if c.indexed_at == 0 {
                    chrono::Utc::now().timestamp()
                } else {
                    c.indexed_at
                }
            })
            .collect();

        // Parent symbol context (nullable)
        let parent_symbols: Vec<Option<&str>> =
            chunks.iter().map(|c| c.parent_symbol.as_deref()).collect();

        // Is overview chunk (nullable)
        let is_overviews: Vec<Option<bool>> = chunks.iter().map(|c| Some(c.is_overview)).collect();

        let schema = Arc::new(self.get_schema());
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(ids)),
                Arc::new(StringArray::from(source_ids)),
                Arc::new(StringArray::from(filepaths)),
                Arc::new(StringArray::from(languages)),
                Arc::new(StringArray::from(contents)),
                Arc::new(Int32Array::from(start_lines)),
                Arc::new(Int32Array::from(end_lines)),
                Arc::new(embedding_array),
                Arc::new(StringArray::from(workspaces)),
                Arc::new(StringArray::from(content_hashes)),
                Arc::new(Int64Array::from(mtimes)),
                Arc::new(Int64Array::from(indexed_ats)),
                Arc::new(StringArray::from(parent_symbols)),
                Arc::new(BooleanArray::from(is_overviews)),
            ],
        )
        .map_err(|e| RetrievalErr::LanceDbQueryFailed {
            table: self.table_name.clone(),
            cause: e.to_string(),
        })
    }

    /// Build a FixedSizeList array from embedding vectors.
    fn build_embedding_array(&self, embeddings: &[Option<Vec<f32>>]) -> Result<FixedSizeListArray> {
        let dim = self.dimension as usize;
        let mut values: Vec<f32> = Vec::with_capacity(embeddings.len() * dim);
        let mut validity: Vec<bool> = Vec::with_capacity(embeddings.len());

        for embedding in embeddings {
            match embedding {
                Some(vec) => {
                    values.extend(vec.iter().take(dim));
                    // Pad if needed
                    if vec.len() < dim {
                        values.extend(std::iter::repeat(0.0).take(dim - vec.len()));
                    }
                    validity.push(true);
                }
                None => {
                    values.extend(std::iter::repeat(0.0).take(dim));
                    validity.push(false);
                }
            }
        }

        let values_array = Float32Array::from(values);
        let field = Arc::new(Field::new("item", DataType::Float32, false));

        FixedSizeListArray::try_new(
            field,
            self.dimension,
            Arc::new(values_array),
            Some(validity.into()),
        )
        .map_err(|e| RetrievalErr::LanceDbQueryFailed {
            table: self.table_name.clone(),
            cause: e.to_string(),
        })
    }

    /// Store a batch of code chunks.
    pub async fn store_chunks(&self, chunks: &[CodeChunk]) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }

        let table = self.get_or_create_table().await?;
        let batch = self.chunks_to_batch(chunks)?;

        // Create a RecordBatchIterator for LanceDB
        let schema = batch.schema();
        let reader = arrow::record_batch::RecordBatchIterator::new(vec![Ok(batch)], schema);

        table
            .add(reader)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        Ok(())
    }

    /// Parse a RecordBatch into CodeChunks.
    fn batch_to_chunks(batch: &RecordBatch) -> Result<Vec<CodeChunk>> {
        // Core fields
        let ids = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| RetrievalErr::LanceDbQueryFailed {
                table: "code_chunks".to_string(),
                cause: "Invalid id column".to_string(),
            })?;

        let source_ids = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| RetrievalErr::LanceDbQueryFailed {
                table: "code_chunks".to_string(),
                cause: "Invalid source_id column".to_string(),
            })?;

        let filepaths = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| RetrievalErr::LanceDbQueryFailed {
                table: "code_chunks".to_string(),
                cause: "Invalid filepath column".to_string(),
            })?;

        let languages = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| RetrievalErr::LanceDbQueryFailed {
                table: "code_chunks".to_string(),
                cause: "Invalid language column".to_string(),
            })?;

        let contents = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| RetrievalErr::LanceDbQueryFailed {
                table: "code_chunks".to_string(),
                cause: "Invalid content column".to_string(),
            })?;

        let start_lines = batch
            .column(5)
            .as_any()
            .downcast_ref::<Int32Array>()
            .ok_or_else(|| RetrievalErr::LanceDbQueryFailed {
                table: "code_chunks".to_string(),
                cause: "Invalid start_line column".to_string(),
            })?;

        let end_lines = batch
            .column(6)
            .as_any()
            .downcast_ref::<Int32Array>()
            .ok_or_else(|| RetrievalErr::LanceDbQueryFailed {
                table: "code_chunks".to_string(),
                cause: "Invalid end_line column".to_string(),
            })?;

        let embeddings = batch
            .column(7)
            .as_any()
            .downcast_ref::<FixedSizeListArray>();

        // Extended metadata fields (optional for backward compatibility)
        let workspaces = batch
            .column_by_name("workspace")
            .and_then(|c| c.as_any().downcast_ref::<StringArray>());
        let content_hashes = batch
            .column_by_name("content_hash")
            .and_then(|c| c.as_any().downcast_ref::<StringArray>());
        let mtimes = batch
            .column_by_name("mtime")
            .and_then(|c| c.as_any().downcast_ref::<Int64Array>());
        let indexed_ats = batch
            .column_by_name("indexed_at")
            .and_then(|c| c.as_any().downcast_ref::<Int64Array>());
        let parent_symbols = batch
            .column_by_name("parent_symbol")
            .and_then(|c| c.as_any().downcast_ref::<StringArray>());
        let is_overviews = batch
            .column_by_name("is_overview")
            .and_then(|c| c.as_any().downcast_ref::<BooleanArray>());

        let mut chunks = Vec::with_capacity(batch.num_rows());
        for i in 0..batch.num_rows() {
            let embedding = embeddings.and_then(|emb| {
                if emb.is_null(i) {
                    None
                } else {
                    let values = emb.value(i);
                    let arr = values.as_any().downcast_ref::<Float32Array>()?;
                    Some(arr.values().to_vec())
                }
            });

            // Read extended metadata with fallback defaults
            let workspace = workspaces
                .map(|w| w.value(i).to_string())
                .unwrap_or_else(|| source_ids.value(i).to_string());
            let content_hash = content_hashes
                .map(|h| h.value(i).to_string())
                .unwrap_or_default();
            let mtime = mtimes.map(|m| m.value(i)).unwrap_or(0);
            let indexed_at = indexed_ats.map(|a| a.value(i)).unwrap_or(0);
            let parent_symbol = parent_symbols.and_then(|ps| {
                let val = ps.value(i);
                if val.is_empty() {
                    None
                } else {
                    Some(val.to_string())
                }
            });

            // Read is_overview with fallback to false
            let is_overview = is_overviews.map(|o| o.value(i)).unwrap_or(false);

            chunks.push(CodeChunk {
                id: ids.value(i).to_string(),
                source_id: source_ids.value(i).to_string(),
                filepath: filepaths.value(i).to_string(),
                language: languages.value(i).to_string(),
                content: contents.value(i).to_string(),
                start_line: start_lines.value(i),
                end_line: end_lines.value(i),
                embedding,
                modified_time: if mtime > 0 { Some(mtime) } else { None },
                workspace,
                content_hash,
                indexed_at,
                parent_symbol,
                is_overview,
            });
        }

        Ok(chunks)
    }

    /// Search using full-text search (BM25).
    pub async fn search_fts(&self, query: &str, limit: i32) -> Result<Vec<CodeChunk>> {
        if !self.table_exists().await? {
            return Ok(Vec::new());
        }

        let table = self
            .db
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        // Use LanceDB full-text search
        let results = table
            .query()
            .full_text_search(FullTextSearchQuery::new(query.to_string()))
            .limit(limit as usize)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        let mut chunks = Vec::new();
        let mut stream = results;
        while let Some(batch) = futures::StreamExt::next(&mut stream).await {
            let batch = batch.map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;
            chunks.extend(Self::batch_to_chunks(&batch)?);
        }

        Ok(chunks)
    }

    /// Search using vector similarity.
    ///
    /// Returns an error if the embedding dimension doesn't match the configured dimension.
    /// This prevents silent quality degradation from dimension mismatches.
    pub async fn search_vector(&self, embedding: &[f32], limit: i32) -> Result<Vec<CodeChunk>> {
        // Validate embedding dimension matches configured dimension
        if embedding.len() != self.dimension as usize {
            return Err(RetrievalErr::EmbeddingDimensionMismatch {
                expected: self.dimension,
                actual: embedding.len() as i32,
            });
        }

        if !self.table_exists().await? {
            return Ok(Vec::new());
        }

        let table = self
            .db
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        // Use embedding directly - already validated
        let query_vec = embedding.to_vec();

        let results = table
            .vector_search(query_vec)
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?
            .limit(limit as usize)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        let mut chunks = Vec::new();
        let mut stream = results;
        while let Some(batch) = futures::StreamExt::next(&mut stream).await {
            let batch = batch.map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;
            chunks.extend(Self::batch_to_chunks(&batch)?);
        }

        Ok(chunks)
    }

    /// Validate a filepath for safe use in SQL queries.
    ///
    /// Only allows alphanumeric characters, path separators, underscores, hyphens, and dots.
    /// This prevents SQL injection by restricting the character set rather than
    /// trying to escape dangerous patterns.
    fn validate_filepath(filepath: &str) -> Result<()> {
        if filepath.is_empty() {
            return Err(RetrievalErr::FileNotIndexable {
                path: filepath.into(),
                reason: "Empty filepath".to_string(),
            });
        }

        // Whitelist approach: only allow safe characters
        let is_safe = filepath.chars().all(|c| {
            c.is_alphanumeric()
                || c == '/'
                || c == '\\'
                || c == '.'
                || c == '_'
                || c == '-'
                || c == ' '
                || c == '@'
                || c == '+'
                || c == '('
                || c == ')'
        });

        if !is_safe {
            return Err(RetrievalErr::FileNotIndexable {
                path: filepath.into(),
                reason: "Filepath contains potentially unsafe characters".to_string(),
            });
        }

        // Also reject common SQL injection patterns as defense in depth
        let dangerous_patterns = [
            '\0', ';', // SQL statement terminators
        ];
        if filepath.chars().any(|c| dangerous_patterns.contains(&c)) {
            return Err(RetrievalErr::FileNotIndexable {
                path: filepath.into(),
                reason: "Filepath contains dangerous SQL characters".to_string(),
            });
        }

        if filepath.contains("--") || filepath.contains("/*") || filepath.contains("*/") {
            return Err(RetrievalErr::FileNotIndexable {
                path: filepath.into(),
                reason: "Filepath contains SQL comment markers".to_string(),
            });
        }

        Ok(())
    }

    /// Delete chunks by file path.
    ///
    /// Validates the filepath to prevent SQL injection attacks.
    pub async fn delete_by_path(&self, filepath: &str) -> Result<i32> {
        // Validate filepath using whitelist approach
        Self::validate_filepath(filepath)?;

        if !self.table_exists().await? {
            return Ok(0);
        }

        let table = self
            .db
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        // Count before delete
        let count_before = table.count_rows(None).await.unwrap_or(0);

        // Escape single quotes for SQL safety
        let escaped_filepath = filepath.replace('\'', "''");
        table
            .delete(&format!("filepath = '{}'", escaped_filepath))
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        let count_after = table.count_rows(None).await.unwrap_or(0);

        Ok((count_before - count_after) as i32)
    }

    /// Count total chunks.
    pub async fn count(&self) -> Result<i64> {
        if !self.table_exists().await? {
            return Ok(0);
        }

        let table = self
            .db
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        table
            .count_rows(None)
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })
            .map(|c| c as i64)
    }

    /// Create a vector index for faster similarity search.
    pub async fn create_vector_index(&self) -> Result<()> {
        if !self.table_exists().await? {
            return Ok(());
        }

        let table = self
            .db
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        // Create IVF-PQ index for large datasets
        table
            .create_index(&["embedding"], lancedb::index::Index::Auto)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        Ok(())
    }

    /// Create a full-text search index.
    pub async fn create_fts_index(&self) -> Result<()> {
        if !self.table_exists().await? {
            return Ok(());
        }

        let table = self
            .db
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        // Create FTS index on content column
        table
            .create_index(&["content"], lancedb::index::Index::FTS(Default::default()))
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        Ok(())
    }

    // ========== Catalog-like operations for incremental indexing ==========

    /// Get file metadata for a specific file in a workspace.
    ///
    /// Returns the first chunk's metadata for the file, which contains
    /// content_hash, mtime, and indexed_at for change detection.
    pub async fn get_file_metadata(
        &self,
        workspace: &str,
        filepath: &str,
    ) -> Result<Option<FileMetadata>> {
        if !self.table_exists().await? {
            return Ok(None);
        }

        let table = self
            .db
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        // Query for the first chunk of this file
        let escaped_workspace = workspace.replace('\'', "''");
        let escaped_filepath = filepath.replace('\'', "''");
        let filter = format!(
            "workspace = '{}' AND filepath = '{}'",
            escaped_workspace, escaped_filepath
        );

        let results = table
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        let mut stream = results;
        while let Some(batch) = futures::StreamExt::next(&mut stream).await {
            let batch = batch.map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

            if batch.num_rows() > 0 {
                let chunks = Self::batch_to_chunks(&batch)?;
                if let Some(chunk) = chunks.into_iter().next() {
                    return Ok(Some(FileMetadata {
                        filepath: chunk.filepath,
                        workspace: chunk.workspace,
                        content_hash: chunk.content_hash,
                        mtime: chunk.modified_time.unwrap_or(0),
                        indexed_at: chunk.indexed_at,
                    }));
                }
            }
        }

        Ok(None)
    }

    /// Get all file metadata in a workspace.
    ///
    /// Returns unique file entries with their metadata.
    pub async fn get_workspace_files(&self, workspace: &str) -> Result<Vec<FileMetadata>> {
        if !self.table_exists().await? {
            return Ok(Vec::new());
        }

        let table = self
            .db
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        // Build filter
        let escaped_workspace = workspace.replace('\'', "''");
        let filter = format!("workspace = '{}'", escaped_workspace);

        let results = table.query().only_if(filter).execute().await.map_err(|e| {
            RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            }
        })?;

        // Collect unique files
        let mut files: std::collections::HashMap<String, FileMetadata> =
            std::collections::HashMap::new();

        let mut stream = results;
        while let Some(batch) = futures::StreamExt::next(&mut stream).await {
            let batch = batch.map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

            let chunks = Self::batch_to_chunks(&batch)?;
            for chunk in chunks {
                files.entry(chunk.filepath.clone()).or_insert(FileMetadata {
                    filepath: chunk.filepath,
                    workspace: chunk.workspace,
                    content_hash: chunk.content_hash,
                    mtime: chunk.modified_time.unwrap_or(0),
                    indexed_at: chunk.indexed_at,
                });
            }
        }

        Ok(files.into_values().collect())
    }

    /// Delete all chunks for a workspace.
    pub async fn delete_workspace(&self, workspace: &str) -> Result<i32> {
        if !self.table_exists().await? {
            return Ok(0);
        }

        let table = self
            .db
            .open_table(&self.table_name)
            .execute()
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        let count_before = table.count_rows(None).await.unwrap_or(0);

        let escaped_workspace = workspace.replace('\'', "''");
        table
            .delete(&format!("workspace = '{}'", escaped_workspace))
            .await
            .map_err(|e| RetrievalErr::LanceDbQueryFailed {
                table: self.table_name.clone(),
                cause: e.to_string(),
            })?;

        let count_after = table.count_rows(None).await.unwrap_or(0);

        Ok((count_before - count_after) as i32)
    }
}

/// File metadata for incremental indexing.
///
/// Contains the metadata needed for change detection without
/// loading the full chunk content.
#[derive(Debug, Clone)]
pub struct FileMetadata {
    /// File path (relative to workspace)
    pub filepath: String,
    /// Workspace identifier
    pub workspace: String,
    /// Content hash for change detection
    pub content_hash: String,
    /// File modification time
    pub mtime: i64,
    /// Index timestamp
    pub indexed_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper to create a test chunk with metadata.
    fn test_chunk(
        id: &str,
        source_id: &str,
        filepath: &str,
        content: &str,
        content_hash: &str,
    ) -> CodeChunk {
        CodeChunk {
            id: id.to_string(),
            source_id: source_id.to_string(),
            filepath: filepath.to_string(),
            language: "rust".to_string(),
            content: content.to_string(),
            start_line: 1,
            end_line: 1,
            embedding: None,
            modified_time: Some(1700000000),
            workspace: source_id.to_string(),
            content_hash: content_hash.to_string(),
            indexed_at: 1700000100,
            parent_symbol: None,
            is_overview: false,
        }
    }

    #[tokio::test]
    async fn test_open_database() {
        let dir = TempDir::new().unwrap();
        let store = LanceDbStore::open(dir.path()).await.unwrap();
        assert!(!store.table_exists().await.unwrap());
    }

    #[tokio::test]
    async fn test_store_and_count() {
        let dir = TempDir::new().unwrap();
        let store = LanceDbStore::open(dir.path()).await.unwrap();

        let chunks = vec![
            test_chunk("ws:test.rs:0", "ws", "test.rs", "fn main() {}", "abc123"),
            test_chunk("ws:test.rs:1", "ws", "test.rs", "fn foo() {}", "abc123"),
        ];

        store.store_chunks(&chunks).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn test_delete_by_path() {
        let dir = TempDir::new().unwrap();
        let store = LanceDbStore::open(dir.path()).await.unwrap();

        let chunks = vec![
            test_chunk("ws:a.rs:0", "ws", "a.rs", "fn a() {}", "hash_a"),
            test_chunk("ws:b.rs:0", "ws", "b.rs", "fn b() {}", "hash_b"),
        ];

        store.store_chunks(&chunks).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 2);

        let deleted = store.delete_by_path("a.rs").await.unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(store.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_get_file_metadata() {
        let dir = TempDir::new().unwrap();
        let store = LanceDbStore::open(dir.path()).await.unwrap();

        let chunks = vec![
            test_chunk("ws:test.rs:0", "ws", "test.rs", "fn main() {}", "abc123"),
            test_chunk("ws:test.rs:1", "ws", "test.rs", "fn foo() {}", "abc123"),
        ];

        store.store_chunks(&chunks).await.unwrap();

        // Get metadata for existing file
        let metadata = store.get_file_metadata("ws", "test.rs").await.unwrap();
        assert!(metadata.is_some());
        let meta = metadata.unwrap();
        assert_eq!(meta.filepath, "test.rs");
        assert_eq!(meta.workspace, "ws");
        assert_eq!(meta.content_hash, "abc123");
        assert_eq!(meta.mtime, 1700000000);

        // Get metadata for non-existent file
        let metadata = store
            .get_file_metadata("ws", "nonexistent.rs")
            .await
            .unwrap();
        assert!(metadata.is_none());
    }

    #[tokio::test]
    async fn test_get_workspace_files() {
        let dir = TempDir::new().unwrap();
        let store = LanceDbStore::open(dir.path()).await.unwrap();

        let chunks = vec![
            test_chunk("ws:a.rs:0", "ws", "a.rs", "fn a() {}", "hash_a"),
            test_chunk("ws:a.rs:1", "ws", "a.rs", "fn a2() {}", "hash_a"),
            test_chunk("ws:b.rs:0", "ws", "b.rs", "fn b() {}", "hash_b"),
        ];

        store.store_chunks(&chunks).await.unwrap();

        let files = store.get_workspace_files("ws").await.unwrap();
        assert_eq!(files.len(), 2); // a.rs and b.rs

        // Check that both files are present
        let filepaths: Vec<_> = files.iter().map(|f| f.filepath.as_str()).collect();
        assert!(filepaths.contains(&"a.rs"));
        assert!(filepaths.contains(&"b.rs"));
    }

    #[tokio::test]
    async fn test_delete_workspace() {
        let dir = TempDir::new().unwrap();
        let store = LanceDbStore::open(dir.path()).await.unwrap();

        let chunks = vec![
            test_chunk("ws1:a.rs:0", "ws1", "a.rs", "fn a() {}", "hash_a"),
            test_chunk("ws2:b.rs:0", "ws2", "b.rs", "fn b() {}", "hash_b"),
        ];

        store.store_chunks(&chunks).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 2);

        let deleted = store.delete_workspace("ws1").await.unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(store.count().await.unwrap(), 1);

        // Verify ws2 is still there
        let files = store.get_workspace_files("ws2").await.unwrap();
        assert_eq!(files.len(), 1);
    }
}
