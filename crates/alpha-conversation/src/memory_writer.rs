//! Memory writer — persists conversation exchanges as episodic memories.
//!
//! After each successful conversation exchange, the memory writer stores
//! the user/alpha exchange as a single episodic memory with an embedding.
//! This runs as a fire-and-forget background task so the user response
//! is never delayed by memory persistence.

use alpha_common::error::AlphaError;
use alpha_common::schemas::memory::{MemoryRecord, MemoryType};
use alpha_memory::MemoryStore;
use tracing::{debug, warn};

use crate::types::ModelProvider;

/// Writes conversation exchanges to memory storage.
pub struct MemoryWriter;

impl MemoryWriter {
    /// Write a user-alpha exchange to the memory store.
    ///
    /// Creates an episodic memory record containing both the user message
    /// and alpha response, generates an embedding via the model provider,
    /// and stores the result.
    ///
    /// If embedding generation fails, the memory is stored without an
    /// embedding (it won't be found via semantic search but is still
    /// persisted for history).
    pub async fn write_exchange<M: ModelProvider>(
        model_provider: &M,
        memory_store: &MemoryStore,
        user_message: &str,
        alpha_response: &str,
    ) -> Result<(), AlphaError> {
        let exchange_text = format!("User: {user_message}\nAlpha: {alpha_response}");

        // Create the episodic memory record.
        let mut record = MemoryRecord::new(
            MemoryType::Episodic,
            exchange_text.clone(),
            "conversation".to_string(),
            0.5,
        );

        // Generate embedding. If this fails, store without embedding.
        match model_provider.embed_single(&exchange_text).await {
            Ok(embedding) => {
                record.embedding = embedding;
                debug!("Exchange embedding generated");
            }
            Err(e) => {
                warn!(error = %e, "Failed to generate exchange embedding, storing without");
            }
        }

        memory_store.store_with_embedding(&record)?;
        debug!(memory_id = %record.id, "Exchange stored as episodic memory");

        Ok(())
    }
}
