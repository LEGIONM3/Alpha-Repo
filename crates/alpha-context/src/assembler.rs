//! Context assembler — builds prompt context from Alpha's knowledge systems.
//!
//! Given a user message, optional query embedding, and conversation history,
//! the assembler retrieves relevant information from memory, relationship,
//! and knowledge stores, then assembles it into a token-budgeted context window.

use alpha_common::error::AlphaError;
use alpha_common::schemas::identity::AlphaIdentity;
use alpha_common::types::AlphaId;
use alpha_knowledge::KnowledgeStore;
use alpha_memory::{MemoryStore, SearchOptions};
use alpha_relationship::RelationshipStore;
use chrono::Utc;
use tracing::debug;

use crate::types::{ContextBlock, ContextConfig, ContextWindow, ConversationTurn, TurnRole};

/// Assembles context from Alpha's knowledge systems.
///
/// NOT a persistent service. Created per-request with references
/// to the stores it needs. The caller provides a query embedding
/// (generated externally via `ModelRouter::embed_single`).
pub struct ContextAssembler<'a> {
    memory_store: &'a MemoryStore,
    relationship_store: &'a RelationshipStore,
    knowledge_store: &'a KnowledgeStore,
    identity: &'a AlphaIdentity,
    config: ContextConfig,
}

impl<'a> ContextAssembler<'a> {
    /// Create a new context assembler.
    pub fn new(
        memory_store: &'a MemoryStore,
        relationship_store: &'a RelationshipStore,
        knowledge_store: &'a KnowledgeStore,
        identity: &'a AlphaIdentity,
        config: ContextConfig,
    ) -> Self {
        Self {
            memory_store,
            relationship_store,
            knowledge_store,
            identity,
            config,
        }
    }

    /// Build the system prompt from identity, personality, and constitution summary.
    pub fn build_system_prompt(&self) -> String {
        let p = &self.identity.personality;
        format!(
            "You are {name}, a personal AI companion.\n\
             Tone: {tone}. Verbosity: {verbosity:.1}.\n\
             \n\
             Constitution:\n\
             - User sovereignty: respect user autonomy above all.\n\
             - Transparency: be honest about capabilities and limitations.\n\
             - Privacy first: protect user data.\n\
             - Do no harm: act in the user's best interest.\n\
             - Proportional autonomy: act only within granted trust.\n\
             - Persistent growth: learn and improve continuously.\n\
             - Graceful degradation: maintain service quality under constraints.",
            name = p.name,
            tone = p.tone,
            verbosity = p.verbosity,
        )
    }

    /// Estimate the token count for a string.
    ///
    /// Uses the simple `text.len() / chars_per_token` formula.
    /// A proper tokenizer can be substituted in a future sprint.
    pub fn estimate_tokens(&self, text: &str) -> usize {
        if self.config.chars_per_token == 0 {
            return 0;
        }
        text.len() / self.config.chars_per_token
    }

    /// Assemble a context window for a user message.
    ///
    /// Steps:
    /// 1. Build system prompt from identity + personality.
    /// 2. Retrieve relevant memories via embedding similarity.
    /// 3. Retrieve relationship records (communication prefs first).
    /// 4. Retrieve relevant knowledge entities.
    /// 5. Include recent conversation history.
    /// 6. Apply token budget with priority-based truncation.
    ///
    /// If `query_embedding` is empty, memory retrieval is skipped gracefully.
    pub fn assemble(
        &self,
        user_message: &str,
        query_embedding: &[f32],
        conversation_history: &[ConversationTurn],
    ) -> Result<ContextWindow, AlphaError> {
        let _ = user_message; // Reserved for future text-based features.

        // 1. Build system prompt (never truncated).
        let system_prompt = self.build_system_prompt();
        let system_tokens = self.estimate_tokens(&system_prompt);

        // Available budget after system prompt.
        let remaining_budget = self.config.max_tokens.saturating_sub(system_tokens);

        if remaining_budget == 0 {
            debug!("Token budget exhausted by system prompt alone");
            return Ok(ContextWindow {
                system_prompt,
                blocks: Vec::new(),
                total_estimated_tokens: system_tokens,
                memory_ids_used: Vec::new(),
                relationship_ids_used: Vec::new(),
            });
        }

        // 2. Retrieve and build all candidate blocks.
        //    Stored in priority order (highest first):
        //    [0] relationship, [1] memories, [2] history, [3] knowledge
        let mut memory_ids: Vec<AlphaId> = Vec::new();
        let mut relationship_ids: Vec<AlphaId> = Vec::new();
        let mut candidates: Vec<ContextBlock> = Vec::new();

        // 2a. Relationship records (highest retrieval priority).
        if let Some(block) = self.build_relationship_block(&mut relationship_ids)? {
            candidates.push(block);
        }

        // 2b. Memories (via embedding similarity).
        if let Some(block) = self.build_memory_block(query_embedding, &mut memory_ids)? {
            candidates.push(block);
        }

        // 2c. Conversation history.
        if let Some(block) = self.build_history_block(conversation_history) {
            candidates.push(block);
        }

        // 2d. Knowledge entities (lowest retrieval priority).
        if let Some(block) = self.build_knowledge_block()? {
            candidates.push(block);
        }

        // 3. Apply token budget with priority-based truncation.
        //    Remove lowest-priority blocks first (from end of candidates).
        let total_needed: usize = candidates.iter().map(|b| b.estimated_tokens).sum();

        if total_needed > remaining_budget {
            debug!(
                total_needed,
                remaining_budget, "Context exceeds budget, truncating"
            );

            // Remove entire blocks from lowest priority until budget fits.
            while candidates.len() > 1 {
                let total: usize = candidates.iter().map(|b| b.estimated_tokens).sum();
                if total <= remaining_budget {
                    break;
                }
                let removed = candidates.pop().unwrap();
                debug!(label = %removed.label, tokens = removed.estimated_tokens, "Block removed by truncation");
            }

            // If the single remaining block still exceeds budget, truncate its content.
            if !candidates.is_empty() {
                let total: usize = candidates.iter().map(|b| b.estimated_tokens).sum();
                if total > remaining_budget {
                    let last = candidates.last_mut().unwrap();
                    let max_chars = remaining_budget * self.config.chars_per_token;
                    last.content = truncate_to_chars(&last.content, max_chars);
                    last.estimated_tokens = self.estimate_tokens(&last.content);
                    debug!(
                        label = %last.label,
                        new_tokens = last.estimated_tokens,
                        "Block truncated to fit budget"
                    );
                }
            }
        }

        // 4. Build final result.
        //    Only include IDs for blocks that survived truncation.
        let has_memories = candidates.iter().any(|b| b.label == "memories");
        let has_relationship = candidates.iter().any(|b| b.label == "relationship");

        let memory_ids_used = if has_memories {
            memory_ids
        } else {
            Vec::new()
        };
        let relationship_ids_used = if has_relationship {
            relationship_ids
        } else {
            Vec::new()
        };

        let block_tokens: usize = candidates.iter().map(|b| b.estimated_tokens).sum();
        let total_estimated_tokens = system_tokens + block_tokens;

        debug!(
            system_tokens,
            block_count = candidates.len(),
            block_tokens,
            total_estimated_tokens,
            "Context assembled"
        );

        Ok(ContextWindow {
            system_prompt,
            blocks: candidates,
            total_estimated_tokens,
            memory_ids_used,
            relationship_ids_used,
        })
    }

    /// Build the relationship block from communication prefs and recent records.
    fn build_relationship_block(
        &self,
        ids_out: &mut Vec<AlphaId>,
    ) -> Result<Option<ContextBlock>, AlphaError> {
        // Communication preferences first (highest value for conversation).
        let mut records = self.relationship_store.get_communication_prefs()?;

        // Fill remaining slots with general relationship records.
        let remaining = self.config.max_relationships.saturating_sub(records.len()) as u32;
        if remaining > 0 {
            let more = self.relationship_store.list(None, remaining, 0)?;
            for r in more {
                // Avoid duplicates (comm prefs are also in general list).
                if !records.iter().any(|existing| existing.id == r.id) {
                    records.push(r);
                }
                if records.len() >= self.config.max_relationships {
                    break;
                }
            }
        }

        if records.is_empty() {
            return Ok(None);
        }

        ids_out.extend(records.iter().map(|r| r.id));

        let mut content = String::from("## About the User\n");
        for record in &records {
            content.push_str(&format!("- {}\n", record.content));
        }

        let estimated_tokens = self.estimate_tokens(&content);
        Ok(Some(ContextBlock {
            label: "relationship".to_string(),
            content,
            estimated_tokens,
        }))
    }

    /// Build the memory block from embedding similarity search.
    ///
    /// If `query_embedding` is empty, memory retrieval is skipped gracefully.
    fn build_memory_block(
        &self,
        query_embedding: &[f32],
        ids_out: &mut Vec<AlphaId>,
    ) -> Result<Option<ContextBlock>, AlphaError> {
        if query_embedding.is_empty() {
            debug!("Empty query embedding, skipping memory retrieval");
            return Ok(None);
        }

        let options = SearchOptions {
            limit: self.config.max_memories as u32,
            min_similarity: self.config.similarity_threshold,
            ..Default::default()
        };

        let now = Utc::now();
        let results = self.memory_store.search_by_embedding(
            query_embedding,
            &options,
            &now,
        )?;

        if results.is_empty() {
            return Ok(None);
        }

        ids_out.extend(results.iter().map(|r| r.record.id));

        let mut content = String::from("## Relevant Memories\n");
        for scored in &results {
            content.push_str(&format!(
                "- {} (importance: {:.1})\n",
                scored.record.content, scored.record.importance,
            ));
        }

        let estimated_tokens = self.estimate_tokens(&content);
        Ok(Some(ContextBlock {
            label: "memories".to_string(),
            content,
            estimated_tokens,
        }))
    }

    /// Build the conversation history block.
    fn build_history_block(&self, history: &[ConversationTurn]) -> Option<ContextBlock> {
        if history.is_empty() {
            return None;
        }

        let mut content = String::from("## Recent Conversation\n");
        for turn in history {
            let role = match turn.role {
                TurnRole::User => "User",
                TurnRole::Alpha => "Alpha",
            };
            content.push_str(&format!("{}: {}\n", role, turn.content));
        }

        let estimated_tokens = self.estimate_tokens(&content);
        Some(ContextBlock {
            label: "history".to_string(),
            content,
            estimated_tokens,
        })
    }

    /// Build the knowledge block from stored entities.
    fn build_knowledge_block(&self) -> Result<Option<ContextBlock>, AlphaError> {
        let entities = self.knowledge_store.list(
            None,
            self.config.max_knowledge as u32,
            0,
        )?;

        if entities.is_empty() {
            return Ok(None);
        }

        let mut content = String::from("## Known Entities\n");
        for entity in &entities {
            if entity.description.is_empty() {
                content.push_str(&format!("- {} ({})\n", entity.name, entity.entity_type));
            } else {
                content.push_str(&format!("- {}: {}\n", entity.name, entity.description));
            }
        }

        let estimated_tokens = self.estimate_tokens(&content);
        Ok(Some(ContextBlock {
            label: "knowledge".to_string(),
            content,
            estimated_tokens,
        }))
    }
}

/// Truncate a string to approximately `max_chars` characters,
/// breaking at the last newline before the limit for cleaner output.
fn truncate_to_chars(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }

    // Find a valid UTF-8 char boundary.
    let mut end = max_chars;
    while !text.is_char_boundary(end) && end > 0 {
        end -= 1;
    }

    let truncated = &text[..end];
    // Try to break at the last newline for cleaner output.
    if let Some(last_newline) = truncated.rfind('\n') {
        if last_newline > 0 {
            return text[..=last_newline].to_string();
        }
    }
    truncated.to_string()
}
