//! Conversation service — orchestrates the full message pipeline.
//!
//! Pipeline steps (non-streaming):
//! 1. Get or create active session
//! 2. Store user turn
//! 3. Generate query embedding (graceful on failure)
//! 4. Load recent turns for context
//! 5. Build context window
//! 6. Construct prompt messages
//! 7. Call model for inference
//! 8. Store alpha turn
//! 9. Spawn memory writer task
//! 10. Publish completion event
//! 11. Return response
//!
//! Streaming pipeline (`send_message_stream`):
//! Steps 1-6 are identical. Step 7 uses `chat_stream()`.
//! Token chunks are forwarded via an unbounded channel.
//! On final chunk: steps 8-10 execute, then `StreamEvent::Done` is sent.

use std::sync::Arc;
use std::time::Instant;

use alpha_common::error::AlphaError;
use alpha_common::schemas::identity::AlphaIdentity;
use alpha_common::Event;
use alpha_context::{ContextAssembler, ContextConfig, ConversationTurn, TurnRole};
use alpha_dialog::SessionManager;
use alpha_event_bus::EventBus;
use alpha_knowledge::KnowledgeStore;
use alpha_memory::MemoryStore;
use alpha_model_router::types::{ChatMessage, ChatOptions};
use alpha_relationship::RelationshipStore;
use futures::StreamExt;
use serde_json::json;
use tracing::{debug, info, warn};

use crate::memory_writer::MemoryWriter;
use crate::types::{
    ConversationRequest, ConversationResponse, ModelProvider, SessionHistory, SessionSummary,
    StreamEvent, TurnSummary,
};

/// The number of recent turns to load for context assembly.
const RECENT_TURNS_LIMIT: u32 = 20;

/// Orchestrates the full conversation pipeline.
pub struct ConversationService<M: ModelProvider> {
    model_provider: Arc<M>,
    pub(crate) memory_store: Arc<MemoryStore>,
    pub(crate) relationship_store: Arc<RelationshipStore>,
    pub(crate) knowledge_store: Arc<KnowledgeStore>,
    session_manager: Arc<SessionManager>,
    event_bus: Arc<EventBus>,
    pub(crate) identity: Arc<AlphaIdentity>,
    context_config: ContextConfig,
}

impl<M: ModelProvider + 'static> ConversationService<M> {
    /// Create a new conversation service.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        model_provider: Arc<M>,
        memory_store: Arc<MemoryStore>,
        relationship_store: Arc<RelationshipStore>,
        knowledge_store: Arc<KnowledgeStore>,
        session_manager: Arc<SessionManager>,
        event_bus: Arc<EventBus>,
        identity: Arc<AlphaIdentity>,
        context_config: ContextConfig,
    ) -> Self {
        Self {
            model_provider,
            memory_store,
            relationship_store,
            knowledge_store,
            session_manager,
            event_bus,
            identity,
            context_config,
        }
    }

    /// Process a user message and return Alpha's response.
    ///
    /// This is the primary entry point for the conversation pipeline.
    /// See module-level docs for the full pipeline description.
    pub async fn send_message(
        &self,
        request: &ConversationRequest,
    ) -> Result<ConversationResponse, AlphaError> {
        let start = Instant::now();

        // 1. Get or create active session.
        let session = match request.session_id {
            Some(ref id) => self
                .session_manager
                .get_session(id)?
                .ok_or_else(|| AlphaError::NotFound {
                    entity: "DialogSession".to_string(),
                    id: id.to_string(),
                })?,
            None => self.session_manager.get_or_create_active()?,
        };
        let session_id = session.id;
        debug!(session_id = %session_id, "Using session");

        // 2. Store user turn.
        self.session_manager
            .add_user_turn(&session_id, &request.message)?;
        debug!("User turn stored");

        // 3. Generate query embedding (graceful on failure).
        let query_embedding = match self
            .model_provider
            .embed_single(&request.message)
            .await
        {
            Ok(embedding) => {
                debug!(dim = embedding.len(), "Query embedding generated");
                embedding
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "Embedding generation failed, continuing without memory context"
                );
                Vec::new()
            }
        };

        // 4. Load recent turns for context.
        let recent_turns = self
            .session_manager
            .get_recent_turns(&session_id, RECENT_TURNS_LIMIT)?;

        // Convert to context assembler's turn type.
        // recent_turns is most-recent-first; reverse to chronological.
        // Skip the user turn we just added (it will be the current message).
        let history: Vec<ConversationTurn> = recent_turns
            .iter()
            .rev()
            .skip(1) // Skip the turn we just stored (current user message).
            .map(|t| ConversationTurn {
                role: match t.role {
                    alpha_dialog::TurnRole::User => TurnRole::User,
                    alpha_dialog::TurnRole::Alpha => TurnRole::Alpha,
                },
                content: t.content.clone(),
            })
            .collect();

        // 5. Build context window.
        let assembler = ContextAssembler::new(
            &self.memory_store,
            &self.relationship_store,
            &self.knowledge_store,
            &self.identity,
            self.context_config.clone(),
        );

        let context = assembler.assemble(
            &request.message,
            &query_embedding,
            &history,
        )?;
        debug!(
            blocks = context.blocks.len(),
            tokens = context.total_estimated_tokens,
            "Context assembled"
        );

        // 6. Construct prompt messages.
        let messages = Self::build_prompt_messages(&context, &request.message);

        // 7. Call model for inference.
        let options = ChatOptions::default();
        let chat_response = self.model_provider.chat(&messages, &options).await?;
        let response_text = chat_response.message.content.clone();
        let model = chat_response.model.clone();
        let tokens_used = chat_response.eval_count.unwrap_or(0);
        let duration_ms = start.elapsed().as_millis() as u64;

        info!(
            model = %model,
            tokens = tokens_used,
            duration_ms,
            "Inference complete"
        );

        // 8. Store alpha turn.
        self.session_manager.add_alpha_turn(
            &session_id,
            &response_text,
            &model,
            tokens_used,
            duration_ms,
        )?;
        debug!("Alpha turn stored");

        // 9. Spawn memory writer task (fire-and-forget).
        {
            let mem_store = Arc::clone(&self.memory_store);
            let provider = Arc::clone(&self.model_provider);
            let user_msg = request.message.clone();
            let alpha_resp = response_text.clone();

            tokio::spawn(async move {
                if let Err(e) = MemoryWriter::write_exchange(
                    provider.as_ref(),
                    &mem_store,
                    &user_msg,
                    &alpha_resp,
                )
                .await
                {
                    warn!(error = %e, "Background memory write failed");
                }
            });
        }

        // 10. Publish completion event.
        let memory_ids_used = context.memory_ids_used.clone();
        let relationship_ids_used = context.relationship_ids_used.clone();

        let _ = self
            .event_bus
            .publish(Event::new(
                "alpha.conversation.exchange.completed",
                "alpha-conversation",
                json!({
                    "session_id": session_id.to_string(),
                    "model": model,
                    "tokens_used": tokens_used,
                    "duration_ms": duration_ms,
                    "memory_ids_used": memory_ids_used.len(),
                    "relationship_ids_used": relationship_ids_used.len(),
                }),
            ))
            .await;

        // 11. Return response.
        Ok(ConversationResponse {
            session_id,
            response: response_text,
            model,
            tokens_used,
            duration_ms,
            memory_ids_used,
            relationship_ids_used,
        })
    }

    /// Process a user message with streaming response.
    ///
    /// Returns an unbounded channel receiver that yields `StreamEvent`s:
    /// 1. `StreamEvent::Started` — emitted once before tokens
    /// 2. `StreamEvent::Token(text)` — one per token chunk
    /// 3. `StreamEvent::Done(response)` — on final chunk (turn stored, memory written)
    ///
    /// If any pipeline step fails, `StreamEvent::Error` is emitted and the channel closes.
    ///
    /// Steps 1-6 (session, user turn, embedding, history, context, prompt) run synchronously
    /// before returning the receiver. Step 7 (streaming) runs in a background task.
    pub async fn send_message_stream(
        &self,
        request: &ConversationRequest,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<StreamEvent>, AlphaError> {
        let start = Instant::now();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

        // ── Steps 1-6: Synchronous pipeline preparation ──

        // 1. Get or create active session.
        let session = match request.session_id {
            Some(ref id) => self
                .session_manager
                .get_session(id)?
                .ok_or_else(|| AlphaError::NotFound {
                    entity: "DialogSession".to_string(),
                    id: id.to_string(),
                })?,
            None => self.session_manager.get_or_create_active()?,
        };
        let session_id = session.id;
        debug!(session_id = %session_id, "Streaming: using session");

        // 2. Store user turn.
        self.session_manager
            .add_user_turn(&session_id, &request.message)?;
        debug!("Streaming: user turn stored");

        // 3. Generate query embedding (graceful on failure).
        let query_embedding = match self
            .model_provider
            .embed_single(&request.message)
            .await
        {
            Ok(embedding) => {
                debug!(dim = embedding.len(), "Streaming: query embedding generated");
                embedding
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "Streaming: embedding failed, continuing without memory context"
                );
                Vec::new()
            }
        };

        // 4. Load recent turns for context.
        let recent_turns = self
            .session_manager
            .get_recent_turns(&session_id, RECENT_TURNS_LIMIT)?;

        let history: Vec<ConversationTurn> = recent_turns
            .iter()
            .rev()
            .skip(1)
            .map(|t| ConversationTurn {
                role: match t.role {
                    alpha_dialog::TurnRole::User => TurnRole::User,
                    alpha_dialog::TurnRole::Alpha => TurnRole::Alpha,
                },
                content: t.content.clone(),
            })
            .collect();

        // 5. Build context window.
        let assembler = ContextAssembler::new(
            &self.memory_store,
            &self.relationship_store,
            &self.knowledge_store,
            &self.identity,
            self.context_config.clone(),
        );

        let context = assembler.assemble(
            &request.message,
            &query_embedding,
            &history,
        )?;

        let memory_ids_used = context.memory_ids_used.clone();
        let relationship_ids_used = context.relationship_ids_used.clone();

        debug!(
            blocks = context.blocks.len(),
            tokens = context.total_estimated_tokens,
            "Streaming: context assembled"
        );

        // 6. Construct prompt messages.
        let messages = Self::build_prompt_messages(&context, &request.message);

        // ── Step 7+: Streaming in background task ──

        // Emit Started event.
        let _ = tx.send(StreamEvent::Started { session_id });

        // Acquire the stream. If this fails, emit error and return.
        let options = ChatOptions::default();
        let chat_stream = match self.model_provider.chat_stream(&messages, &options).await {
            Ok(stream) => stream,
            Err(e) => {
                let _ = tx.send(StreamEvent::Error(format!("Failed to start stream: {e}")));
                return Ok(rx);
            }
        };

        // Capture shared state for the background task.
        let session_manager = Arc::clone(&self.session_manager);
        let model_provider = Arc::clone(&self.model_provider);
        let mem_store = Arc::clone(&self.memory_store);
        let event_bus = Arc::clone(&self.event_bus);
        let user_message = request.message.clone();

        tokio::spawn(async move {
            let mut accumulated_response = String::new();
            let mut model_name = String::new();
            let mut total_tokens: u32 = 0;

            futures::pin_mut!(chat_stream);

            while let Some(result) = chat_stream.next().await {
                match result {
                    Ok(chunk) => {
                        // Accumulate the response text.
                        let token_text = chunk.message.content.clone();
                        accumulated_response.push_str(&token_text);

                        if model_name.is_empty() {
                            model_name.clone_from(&chunk.model);
                        }

                        // Emit token event for non-empty content.
                        if !token_text.is_empty()
                            && tx.send(StreamEvent::Token(token_text)).is_err()
                        {
                            // Receiver dropped — abort streaming.
                            debug!("Streaming: receiver dropped, aborting");
                            return;
                        }

                        // Final chunk — perform completion steps.
                        if chunk.done {
                            total_tokens = chunk.eval_count.unwrap_or(0);
                            let duration_ms = start.elapsed().as_millis() as u64;

                            info!(
                                model = %model_name,
                                tokens = total_tokens,
                                duration_ms,
                                "Streaming inference complete"
                            );

                            // 8. Store alpha turn.
                            if let Err(e) = session_manager.add_alpha_turn(
                                &session_id,
                                &accumulated_response,
                                &model_name,
                                total_tokens,
                                duration_ms,
                            ) {
                                warn!(error = %e, "Streaming: failed to store alpha turn");
                            }

                            // 9. Spawn memory writer (fire-and-forget).
                            {
                                let mem = Arc::clone(&mem_store);
                                let prov = Arc::clone(&model_provider);
                                let user_msg = user_message.clone();
                                let alpha_resp = accumulated_response.clone();

                                tokio::spawn(async move {
                                    if let Err(e) = MemoryWriter::write_exchange(
                                        prov.as_ref(),
                                        &mem,
                                        &user_msg,
                                        &alpha_resp,
                                    )
                                    .await
                                    {
                                        warn!(
                                            error = %e,
                                            "Streaming: background memory write failed"
                                        );
                                    }
                                });
                            }

                            // 10. Publish completion event.
                            let _ = event_bus
                                .publish(Event::new(
                                    "alpha.conversation.exchange.completed",
                                    "alpha-conversation",
                                    json!({
                                        "session_id": session_id.to_string(),
                                        "model": model_name,
                                        "tokens_used": total_tokens,
                                        "duration_ms": duration_ms,
                                        "memory_ids_used": memory_ids_used.len(),
                                        "relationship_ids_used": relationship_ids_used.len(),
                                        "streaming": true,
                                    }),
                                ))
                                .await;

                            // Emit Done event.
                            let _ = tx.send(StreamEvent::Done(ConversationResponse {
                                session_id,
                                response: accumulated_response,
                                model: model_name,
                                tokens_used: total_tokens,
                                duration_ms,
                                memory_ids_used,
                                relationship_ids_used,
                            }));

                            return;
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Streaming: chunk error");
                        let _ = tx.send(StreamEvent::Error(format!("Stream error: {e}")));
                        return;
                    }
                }
            }

            // Stream ended without a done=true chunk — unexpected but not fatal.
            if !accumulated_response.is_empty() {
                let duration_ms = start.elapsed().as_millis() as u64;
                warn!("Streaming: stream ended without done=true, finalizing");

                if let Err(e) = session_manager.add_alpha_turn(
                    &session_id,
                    &accumulated_response,
                    &model_name,
                    total_tokens,
                    duration_ms,
                ) {
                    warn!(error = %e, "Streaming: failed to store alpha turn on early close");
                }

                let _ = tx.send(StreamEvent::Done(ConversationResponse {
                    session_id,
                    response: accumulated_response,
                    model: model_name,
                    tokens_used: total_tokens,
                    duration_ms,
                    memory_ids_used,
                    relationship_ids_used,
                }));
            }
        });

        Ok(rx)
    }

    /// Build the prompt messages from the assembled context window.
    ///
    /// Order:
    /// 1. System prompt (from context window) with context blocks appended
    /// 2. Current user message
    pub fn build_prompt_messages(
        context: &alpha_context::ContextWindow,
        user_message: &str,
    ) -> Vec<ChatMessage> {
        let mut messages = Vec::new();

        // System prompt with context blocks appended.
        let mut system_content = context.system_prompt.clone();
        for block in &context.blocks {
            system_content.push_str("\n\n");
            system_content.push_str(&block.content);
        }
        messages.push(ChatMessage::system(system_content));

        // Current user message.
        messages.push(ChatMessage::user(user_message));

        messages
    }

    // ── Session Management ──

    /// List conversation sessions, most recently updated first.
    ///
    /// Returns lightweight `SessionSummary` projections suitable for
    /// a session sidebar or list view.
    pub fn list_sessions(
        &self,
        limit: u32,
    ) -> Result<Vec<SessionSummary>, AlphaError> {
        let sessions = self.session_manager.list_sessions(None, limit, 0)?;

        let summaries = sessions
            .into_iter()
            .map(|s| SessionSummary {
                id: s.id,
                title: s.title,
                status: s.status.as_str().to_string(),
                turn_count: s.turn_count,
                updated_at: s.updated_at.to_rfc3339(),
                created_at: s.created_at.to_rfc3339(),
            })
            .collect();

        Ok(summaries)
    }

    /// Load a session's full history (metadata + all turns in chronological order).
    ///
    /// Returns `AlphaError::NotFound` if the session does not exist.
    pub fn load_session(
        &self,
        session_id: &alpha_common::types::AlphaId,
    ) -> Result<SessionHistory, AlphaError> {
        let session = self
            .session_manager
            .get_session(session_id)?
            .ok_or_else(|| AlphaError::NotFound {
                entity: "DialogSession".to_string(),
                id: session_id.to_string(),
            })?;

        let dialog_turns = self.session_manager.get_turns(session_id)?;

        let turns = dialog_turns
            .into_iter()
            .map(|t| TurnSummary {
                role: t.role.as_str().to_string(),
                content: t.content,
                model_used: t.model_used,
                tokens_used: t.tokens_used,
                created_at: t.created_at.to_rfc3339(),
            })
            .collect();

        Ok(SessionHistory {
            session_id: session.id,
            title: session.title,
            status: session.status.as_str().to_string(),
            created_at: session.created_at.to_rfc3339(),
            updated_at: session.updated_at.to_rfc3339(),
            turns,
        })
    }

    /// Create a new active conversation session.
    ///
    /// Closes the current active session (if any) before creating a new one.
    /// Returns the new session's summary.
    pub fn new_session(&self) -> Result<SessionSummary, AlphaError> {
        // Close any currently active sessions.
        let active_sessions = self
            .session_manager
            .list_sessions(Some(&alpha_dialog::SessionStatus::Active), 10, 0)?;

        for session in &active_sessions {
            if let Err(e) = self.session_manager.close_session(&session.id) {
                warn!(session_id = %session.id, error = %e, "Failed to close active session");
            }
        }

        // Create a new session.
        let session_id = self.session_manager.create_session()?;

        let session = self
            .session_manager
            .get_session(&session_id)?
            .ok_or_else(|| {
                AlphaError::Other("Failed to read newly created session".to_string())
            })?;

        debug!(session_id = %session_id, "New session created");

        Ok(SessionSummary {
            id: session.id,
            title: session.title,
            status: session.status.as_str().to_string(),
            turn_count: session.turn_count,
            updated_at: session.updated_at.to_rfc3339(),
            created_at: session.created_at.to_rfc3339(),
        })
    }

    /// Close the current active session.
    ///
    /// Returns `AlphaError::NotFound` if no active session exists.
    pub fn close_session(
        &self,
        session_id: &alpha_common::types::AlphaId,
    ) -> Result<(), AlphaError> {
        self.session_manager.close_session(session_id)?;
        debug!(session_id = %session_id, "Session closed via ConversationService");
        Ok(())
    }
}
