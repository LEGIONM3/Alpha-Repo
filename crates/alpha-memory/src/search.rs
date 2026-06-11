//! Semantic search over stored memories.
//!
//! Sprint 2 Phase 2: embedding-based retrieval with composite scoring.
//! Text-based search (which generates embeddings via ModelRouter) is
//! deferred — use `search_by_embedding()` directly for now.

use alpha_common::error::AlphaError;
use alpha_common::schemas::memory::{GovernanceState, MemoryRecord};
use alpha_common::types::Timestamp;
use tracing::debug;

use crate::scoring::{composite_score, cosine_similarity, recency_factor};
use crate::store::{validate_embedding_dimension, MemoryStore};

/// A search result with its composite score.
#[derive(Debug, Clone)]
pub struct ScoredMemory {
    /// The matched memory record.
    pub record: MemoryRecord,
    /// Cosine similarity to the query embedding.
    pub similarity: f32,
    /// Composite retrieval score (similarity × importance × recency).
    pub score: f32,
}

/// Search options for embedding-based retrieval.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    /// Maximum number of results to return.
    pub limit: u32,
    /// Minimum cosine similarity threshold. Results below this are discarded.
    pub min_similarity: f32,
    /// If set, only return memories with this governance state.
    pub governance_filter: Option<GovernanceState>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 20,
            min_similarity: 0.0,
            governance_filter: None,
        }
    }
}

/// SQL candidate limit per Sprint 2 Amendment.
const CANDIDATE_LIMIT: u32 = 1000;

impl MemoryStore {
    /// Search memories by embedding similarity.
    ///
    /// Algorithm:
    /// 1. Validate query embedding dimension
    /// 2. Fetch candidate memories (up to 1000, Sprint 2 Amendment)
    /// 3. Compute cosine similarity for each candidate
    /// 4. Filter by minimum similarity threshold
    /// 5. Compute composite score (similarity × importance × recency)
    /// 6. Sort descending by composite score
    /// 7. Return top `limit` results
    ///
    /// Candidates are memories that have a non-empty embedding.
    pub fn search_by_embedding(
        &self,
        query_embedding: &[f32],
        options: &SearchOptions,
        now: &Timestamp,
    ) -> Result<Vec<ScoredMemory>, AlphaError> {
        // Validate dimension if configured.
        if self.expected_dimension > 0 {
            validate_embedding_dimension(query_embedding, self.expected_dimension)?;
        }

        if query_embedding.is_empty() {
            return Err(AlphaError::Invariant(
                "Query embedding must not be empty".to_string(),
            ));
        }

        let conn = self.conn.lock().map_err(|e| {
            AlphaError::Database(format!("Failed to acquire lock: {e}"))
        })?;

        // Build SQL with optional governance filter.
        let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
            if let Some(ref gs) = options.governance_filter {
                let gs_str = serde_json::to_string(gs)
                    .unwrap_or_default()
                    .trim_matches('"')
                    .to_string();
                (
                    format!(
                        "SELECT id, memory_type, content, embedding, importance, access_count,
                                last_accessed, created_at, source, associations, tags,
                                confidence, decay_rate, governance_state, metadata
                         FROM memories
                         WHERE length(embedding) > 0 AND governance_state = ?1
                         ORDER BY importance DESC
                         LIMIT {CANDIDATE_LIMIT}"
                    ),
                    vec![Box::new(gs_str) as Box<dyn rusqlite::types::ToSql>],
                )
            } else {
                (
                    format!(
                        "SELECT id, memory_type, content, embedding, importance, access_count,
                                last_accessed, created_at, source, associations, tags,
                                confidence, decay_rate, governance_state, metadata
                         FROM memories
                         WHERE length(embedding) > 0
                         ORDER BY importance DESC
                         LIMIT {CANDIDATE_LIMIT}"
                    ),
                    vec![],
                )
            };

        let mut stmt = conn.prepare(&sql).map_err(|e| {
            AlphaError::Database(format!("Failed to prepare search query: {e}"))
        })?;

        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(crate::store::row_to_memory(row))
            })
            .map_err(|e| AlphaError::Database(format!("Failed to query candidates: {e}")))?;

        // Score and filter candidates.
        let mut scored: Vec<ScoredMemory> = Vec::new();

        for row_result in rows {
            let record = row_result
                .map_err(|e| AlphaError::Database(format!("Row error: {e}")))?;
            let record = record?;

            // Skip empty embeddings (shouldn't happen due to WHERE, but defensive).
            if record.embedding.is_empty() {
                continue;
            }

            let similarity = cosine_similarity(query_embedding, &record.embedding);

            // Apply minimum similarity filter.
            if similarity < options.min_similarity {
                continue;
            }

            let recency = recency_factor(record.decay_rate, &record.last_accessed, now);
            let score = composite_score(similarity, record.importance, recency);

            scored.push(ScoredMemory {
                record,
                similarity,
                score,
            });
        }

        // Sort by composite score descending.
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Truncate to requested limit.
        scored.truncate(options.limit as usize);

        debug!(
            candidates_scored = scored.len(),
            limit = options.limit,
            min_similarity = options.min_similarity,
            "Search complete"
        );

        Ok(scored)
    }

    /// Search memories by text query.
    ///
    /// **Deferred**: Requires ModelRouter integration to generate the
    /// query embedding from text. Use [`Self::search_by_embedding`] directly.
    pub fn search(
        &self,
        _query_text: &str,
        _options: &SearchOptions,
    ) -> Result<Vec<ScoredMemory>, AlphaError> {
        Err(AlphaError::Other(
            "Not implemented in Sprint 2".into(),
        ))
    }
}
