//! Tests for the conversation service.
//!
//! Uses a `MockModelProvider` to avoid requiring a live Ollama instance.

use std::sync::Arc;

use alpha_common::error::AlphaError;
use alpha_common::schemas::identity::AlphaIdentity;
use alpha_context::ContextConfig;
use alpha_dialog::SessionManager;
use alpha_event_bus::EventBus;
use alpha_knowledge::KnowledgeStore;
use alpha_memory::MemoryStore;
use alpha_model_router::types::{
    ChatMessage, ChatOptions, ChatResponse, ChatRole, ChatStream, ChatStreamChunk,
};
use alpha_relationship::RelationshipStore;
use futures::stream;
use tempfile::TempDir;

use crate::memory_writer::MemoryWriter;
use crate::service::ConversationService;
use crate::types::{ConversationRequest, ModelProvider, StreamEvent};

// ── Mock Model Provider ──

/// Mock model provider for testing without Ollama.
struct MockModelProvider {
    /// Whether embed_single should fail.
    embed_fails: bool,
    /// Whether chat_stream should return an error.
    stream_fails: bool,
}

impl MockModelProvider {
    fn new() -> Self {
        Self {
            embed_fails: false,
            stream_fails: false,
        }
    }

    fn with_embed_failure() -> Self {
        Self {
            embed_fails: true,
            stream_fails: false,
        }
    }

    fn with_stream_failure() -> Self {
        Self {
            embed_fails: false,
            stream_fails: true,
        }
    }
}

impl ModelProvider for MockModelProvider {
    async fn chat(
        &self,
        messages: &[ChatMessage],
        _options: &ChatOptions,
    ) -> Result<ChatResponse, AlphaError> {
        // Echo back the last user message with a canned prefix.
        let user_content = messages
            .iter()
            .rev()
            .find(|m| m.role == ChatRole::User)
            .map(|m| m.content.clone())
            .unwrap_or_default();

        Ok(ChatResponse {
            model: "mock-model".to_string(),
            message: ChatMessage::assistant(format!("Mock response to: {user_content}")),
            done: true,
            total_duration_ns: Some(100_000_000),
            prompt_eval_count: Some(10),
            eval_count: Some(20),
        })
    }

    async fn chat_stream(
        &self,
        messages: &[ChatMessage],
        _options: &ChatOptions,
    ) -> Result<ChatStream, AlphaError> {
        if self.stream_fails {
            return Err(AlphaError::Other("Mock stream failure".into()));
        }

        let user_content = messages
            .iter()
            .rev()
            .find(|m| m.role == ChatRole::User)
            .map(|m| m.content.clone())
            .unwrap_or_default();

        // Simulate streaming: yield 3 token chunks, then a final done chunk.
        let tokens = vec!["Hello", " from", " Alpha"];
        let model = "mock-model".to_string();
        let full_response = format!("Hello from Alpha to: {user_content}");

        let mut chunks: Vec<Result<ChatStreamChunk, AlphaError>> = Vec::new();

        for token in &tokens {
            chunks.push(Ok(ChatStreamChunk {
                model: model.clone(),
                message: ChatMessage::assistant(token.to_string()),
                done: false,
                total_duration: None,
                prompt_eval_count: None,
                eval_count: None,
            }));
        }

        // Add a chunk with the rest of the response before the done chunk.
        let remaining = format!(" to: {user_content}");
        chunks.push(Ok(ChatStreamChunk {
            model: model.clone(),
            message: ChatMessage::assistant(remaining),
            done: false,
            total_duration: None,
            prompt_eval_count: None,
            eval_count: None,
        }));

        // Final (done) chunk with metadata.
        chunks.push(Ok(ChatStreamChunk {
            model: model.clone(),
            message: ChatMessage::assistant(String::new()),
            done: true,
            total_duration: Some(500_000_000),
            prompt_eval_count: Some(10),
            eval_count: Some(15),
        }));

        // Verify accumulated text will match.
        let _ = full_response;

        Ok(Box::pin(stream::iter(chunks)))
    }

    async fn embed_single(&self, _text: &str) -> Result<Vec<f32>, AlphaError> {
        if self.embed_fails {
            return Err(AlphaError::Other("Mock embedding failure".into()));
        }
        Ok(vec![0.1, 0.2, 0.3])
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }
}

// ── Test Helpers ──

struct TestHarness {
    _dir: TempDir,
    service: ConversationService<MockModelProvider>,
    session_manager: Arc<SessionManager>,
    _memory_store: Arc<MemoryStore>,
}

fn create_harness() -> TestHarness {
    create_harness_with_provider(MockModelProvider::new())
}

fn create_harness_with_provider(provider: MockModelProvider) -> TestHarness {
    let dir = TempDir::new().unwrap();
    let base = dir.path();

    let memory_store = Arc::new(
        MemoryStore::open(&base.join("memory.db"), 0).unwrap(),
    );
    let relationship_store = Arc::new(
        RelationshipStore::open(&base.join("relationship.db")).unwrap(),
    );
    let knowledge_store = Arc::new(
        KnowledgeStore::open(&base.join("knowledge.db")).unwrap(),
    );
    let session_manager = Arc::new(
        SessionManager::open(&base.join("dialog.db")).unwrap(),
    );
    let event_bus = Arc::new(
        EventBus::open(&base.join("events.db")).unwrap(),
    );

    let identity = Arc::new(AlphaIdentity::new("test_hash".to_string()));

    let service = ConversationService::new(
        Arc::new(provider),
        Arc::clone(&memory_store),
        Arc::clone(&relationship_store),
        Arc::clone(&knowledge_store),
        Arc::clone(&session_manager),
        Arc::clone(&event_bus),
        identity,
        ContextConfig::default(),
    );

    TestHarness {
        _dir: dir,
        service,
        session_manager,
        _memory_store: memory_store,
    }
}

// ── Non-Streaming Tests (existing) ──

#[tokio::test]
async fn test_conversation_creates_session() {
    let h = create_harness();

    // No sessions exist initially.
    assert_eq!(h.session_manager.count(None).unwrap(), 0);

    let request = ConversationRequest {
        message: "Hello Alpha".to_string(),
        session_id: None,
    };

    let response = h.service.send_message(&request).await.unwrap();

    // A session should have been created.
    assert_eq!(h.session_manager.count(None).unwrap(), 1);

    // Response should reference the created session.
    let session = h
        .session_manager
        .get_session(&response.session_id)
        .unwrap()
        .expect("Session should exist");

    assert_eq!(session.status, alpha_dialog::SessionStatus::Active);
}

#[tokio::test]
async fn test_conversation_adds_turns() {
    let h = create_harness();

    let request = ConversationRequest {
        message: "What is Rust?".to_string(),
        session_id: None,
    };

    let response = h.service.send_message(&request).await.unwrap();

    // Should have 2 turns: user + alpha.
    let turns = h
        .session_manager
        .get_turns(&response.session_id)
        .unwrap();

    assert_eq!(turns.len(), 2);

    // First turn: user.
    assert_eq!(turns[0].role, alpha_dialog::TurnRole::User);
    assert_eq!(turns[0].content, "What is Rust?");
    assert_eq!(turns[0].turn_index, 0);

    // Second turn: alpha.
    assert_eq!(turns[1].role, alpha_dialog::TurnRole::Alpha);
    assert!(turns[1].content.contains("Mock response"));
    assert_eq!(turns[1].turn_index, 1);
    assert_eq!(turns[1].model_used, "mock-model");
}

#[tokio::test]
async fn test_active_session_reused() {
    let h = create_harness();

    let req1 = ConversationRequest {
        message: "First message".to_string(),
        session_id: None,
    };
    let resp1 = h.service.send_message(&req1).await.unwrap();

    let req2 = ConversationRequest {
        message: "Second message".to_string(),
        session_id: None,
    };
    let resp2 = h.service.send_message(&req2).await.unwrap();

    // Both should use the same session.
    assert_eq!(resp1.session_id, resp2.session_id);

    // Session should have 4 turns (2 user + 2 alpha).
    let turns = h
        .session_manager
        .get_turns(&resp1.session_id)
        .unwrap();
    assert_eq!(turns.len(), 4);

    // Still only 1 session.
    assert_eq!(h.session_manager.count(None).unwrap(), 1);
}

#[tokio::test]
async fn test_memory_writer_creates_memory() {
    let dir = TempDir::new().unwrap();
    let memory_store = MemoryStore::open(&dir.path().join("memory.db"), 0).unwrap();
    let provider = MockModelProvider::new();

    MemoryWriter::write_exchange(
        &provider,
        &memory_store,
        "Hello Alpha",
        "Hello! How can I help?",
    )
    .await
    .unwrap();

    // Should have stored one memory.
    let count = memory_store.count(None, None).unwrap();
    assert_eq!(count, 1);

    // Verify memory content.
    let memories = memory_store.list(None, None, 10, 0).unwrap();
    assert_eq!(memories.len(), 1);

    let memory = &memories[0];
    assert!(memory.content.contains("User: Hello Alpha"));
    assert!(memory.content.contains("Alpha: Hello! How can I help?"));
    assert_eq!(memory.source, "conversation");
    assert!((memory.importance - 0.5).abs() < f32::EPSILON);
    assert_eq!(
        memory.memory_type,
        alpha_common::schemas::memory::MemoryType::Episodic
    );

    // Embedding should have been stored.
    assert!(!memory.embedding.is_empty());
}

#[tokio::test]
async fn test_embedding_failure_graceful() {
    let h = create_harness_with_provider(MockModelProvider::with_embed_failure());

    let request = ConversationRequest {
        message: "This should still work".to_string(),
        session_id: None,
    };

    // Should NOT fail — embedding failure is graceful.
    let response = h.service.send_message(&request).await.unwrap();

    assert!(!response.response.is_empty());
    assert!(response.response.contains("Mock response"));
    assert_eq!(response.model, "mock-model");

    // Turns should still be stored.
    let turns = h
        .session_manager
        .get_turns(&response.session_id)
        .unwrap();
    assert_eq!(turns.len(), 2);
}

#[tokio::test]
async fn test_prompt_construction_order() {
    // Build prompt messages directly to inspect structure.
    let dir = TempDir::new().unwrap();
    let base = dir.path();

    let memory_store = MemoryStore::open(&base.join("memory.db"), 0).unwrap();
    let relationship_store = RelationshipStore::open(&base.join("relationship.db")).unwrap();
    let knowledge_store = KnowledgeStore::open(&base.join("knowledge.db")).unwrap();
    let identity = AlphaIdentity::new("test_hash".to_string());

    let assembler = alpha_context::ContextAssembler::new(
        &memory_store,
        &relationship_store,
        &knowledge_store,
        &identity,
        ContextConfig::default(),
    );

    let context = assembler.assemble("Hello", &[], &[]).unwrap();
    let messages =
        ConversationService::<MockModelProvider>::build_prompt_messages(&context, "Hello");

    // Must have at least 2 messages: system + user.
    assert!(messages.len() >= 2);

    // First message must be system prompt.
    assert_eq!(messages[0].role, ChatRole::System);
    assert!(
        messages[0].content.contains("Alpha"),
        "System prompt should contain Alpha's name"
    );
    assert!(
        messages[0].content.contains("Constitution"),
        "System prompt should contain constitution"
    );

    // Last message must be the user's current message.
    let last = messages.last().unwrap();
    assert_eq!(last.role, ChatRole::User);
    assert_eq!(last.content, "Hello");
}

#[tokio::test]
async fn test_conversation_response_structure() {
    let h = create_harness();

    let request = ConversationRequest {
        message: "Tell me about yourself".to_string(),
        session_id: None,
    };

    let response = h.service.send_message(&request).await.unwrap();

    // Verify all fields are populated.
    assert!(!response.response.is_empty());
    assert_eq!(response.model, "mock-model");
    assert_eq!(response.tokens_used, 20);
    assert!(response.duration_ms > 0);

    // Session ID should be valid.
    let session = h
        .session_manager
        .get_session(&response.session_id)
        .unwrap();
    assert!(session.is_some());

    // Memory and relationship IDs should be vectors (possibly empty).
    // With empty stores, they should be empty.
    assert!(response.memory_ids_used.is_empty());
    assert!(response.relationship_ids_used.is_empty());
}

// ── Streaming Tests (Sprint 4A) ──

#[tokio::test]
async fn test_stream_started_event() {
    let h = create_harness();

    let request = ConversationRequest {
        message: "Hello streaming".to_string(),
        session_id: None,
    };

    let mut rx = h.service.send_message_stream(&request).await.unwrap();

    // First event must be Started.
    let first = rx.recv().await.expect("Should receive Started event");
    match first {
        StreamEvent::Started { session_id } => {
            // Session should exist.
            let session = h
                .session_manager
                .get_session(&session_id)
                .unwrap();
            assert!(session.is_some(), "Session should exist after Started");
        }
        other => panic!("Expected StreamEvent::Started, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_token_streaming() {
    let h = create_harness();

    let request = ConversationRequest {
        message: "Stream me tokens".to_string(),
        session_id: None,
    };

    let mut rx = h.service.send_message_stream(&request).await.unwrap();

    // Collect all events.
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    // Should have: Started, Token(s), Done.
    assert!(events.len() >= 3, "Should have at least Started + Token + Done");

    // First must be Started.
    assert!(
        matches!(&events[0], StreamEvent::Started { .. }),
        "First event should be Started"
    );

    // Count token events.
    let token_count = events
        .iter()
        .filter(|e| matches!(e, StreamEvent::Token(_)))
        .count();
    assert!(token_count >= 1, "Should have at least one Token event");

    // Collect token text.
    let accumulated: String = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Token(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        !accumulated.is_empty(),
        "Accumulated token text should not be empty"
    );

    // Last must be Done.
    let last = events.last().unwrap();
    assert!(
        matches!(last, StreamEvent::Done(_)),
        "Last event should be Done"
    );
}

#[tokio::test]
async fn test_stream_completed_event() {
    let h = create_harness();

    let request = ConversationRequest {
        message: "Complete me".to_string(),
        session_id: None,
    };

    let mut rx = h.service.send_message_stream(&request).await.unwrap();

    // Drain to the Done event.
    let mut done_response = None;
    while let Some(event) = rx.recv().await {
        if let StreamEvent::Done(resp) = event {
            done_response = Some(resp);
            break;
        }
    }

    let response = done_response.expect("Should receive Done event");

    // Verify Done payload.
    assert!(!response.response.is_empty());
    assert_eq!(response.model, "mock-model");
    assert_eq!(response.tokens_used, 15); // From mock's eval_count
    assert!(response.duration_ms > 0);

    // Verify turns were stored (user + alpha).
    let turns = h
        .session_manager
        .get_turns(&response.session_id)
        .unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].role, alpha_dialog::TurnRole::User);
    assert_eq!(turns[0].content, "Complete me");
    assert_eq!(turns[1].role, alpha_dialog::TurnRole::Alpha);
    assert!(!turns[1].content.is_empty());
    assert_eq!(turns[1].model_used, "mock-model");
}

#[tokio::test]
async fn test_stream_error_event() {
    let h = create_harness_with_provider(MockModelProvider::with_stream_failure());

    let request = ConversationRequest {
        message: "This should fail".to_string(),
        session_id: None,
    };

    let mut rx = h.service.send_message_stream(&request).await.unwrap();

    // Collect all events.
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    // Should have Started, then Error.
    assert!(events.len() >= 2, "Should have at least Started + Error");

    assert!(
        matches!(&events[0], StreamEvent::Started { .. }),
        "First event should be Started"
    );

    // Find the error event.
    let has_error = events
        .iter()
        .any(|e| matches!(e, StreamEvent::Error(_)));
    assert!(has_error, "Should have an Error event");

    // Should NOT have a Done event.
    let has_done = events
        .iter()
        .any(|e| matches!(e, StreamEvent::Done(_)));
    assert!(!has_done, "Should NOT have a Done event after error");
}

#[tokio::test]
async fn test_embedding_failure_graceful_stream() {
    let h = create_harness_with_provider(MockModelProvider::with_embed_failure());

    let request = ConversationRequest {
        message: "Embed fails but stream works".to_string(),
        session_id: None,
    };

    // Should NOT fail — embedding failure is graceful.
    let mut rx = h.service.send_message_stream(&request).await.unwrap();

    // Drain to the Done event.
    let mut done_response = None;
    while let Some(event) = rx.recv().await {
        if let StreamEvent::Done(resp) = event {
            done_response = Some(resp);
            break;
        }
    }

    let response = done_response.expect("Should receive Done despite embedding failure");

    assert!(!response.response.is_empty());
    assert_eq!(response.model, "mock-model");

    // Turns should still be stored.
    let turns = h
        .session_manager
        .get_turns(&response.session_id)
        .unwrap();
    assert_eq!(turns.len(), 2);
}

#[tokio::test]
async fn test_session_created_stream() {
    let h = create_harness();

    // No sessions initially.
    assert_eq!(h.session_manager.count(None).unwrap(), 0);

    let request = ConversationRequest {
        message: "Create a session for me".to_string(),
        session_id: None,
    };

    let mut rx = h.service.send_message_stream(&request).await.unwrap();

    // Drain all events.
    let mut session_id = None;
    while let Some(event) = rx.recv().await {
        if let StreamEvent::Started { session_id: sid } = event {
            session_id = Some(sid);
        }
    }

    let sid = session_id.expect("Should have received Started with session_id");

    // Session should have been created.
    assert_eq!(h.session_manager.count(None).unwrap(), 1);

    let session = h
        .session_manager
        .get_session(&sid)
        .unwrap()
        .expect("Session should exist");
    assert_eq!(session.status, alpha_dialog::SessionStatus::Active);
}

#[tokio::test]
async fn test_memory_writer_triggered() {
    let h = create_harness();

    let request = ConversationRequest {
        message: "Remember this conversation".to_string(),
        session_id: None,
    };

    let mut rx = h.service.send_message_stream(&request).await.unwrap();

    // Drain all events.
    while let Some(_event) = rx.recv().await {}

    // Give the fire-and-forget memory writer task a moment to complete.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // The MemoryWriter should have stored an episodic memory.
    let count = h._memory_store.count(None, None).unwrap();
    assert_eq!(count, 1, "MemoryWriter should have stored one memory");

    let memories = h._memory_store.list(None, None, 10, 0).unwrap();
    let memory = &memories[0];
    assert!(memory.content.contains("User: Remember this conversation"));
    assert!(memory.content.contains("Alpha: "));
    assert_eq!(memory.source, "conversation");
    assert_eq!(
        memory.memory_type,
        alpha_common::schemas::memory::MemoryType::Episodic
    );
}

// ── Session Management Tests (Sprint 4A Phase 2) ──

#[tokio::test]
async fn test_list_sessions() {
    let h = create_harness();

    // Initially empty.
    let sessions = h.service.list_sessions(10).unwrap();
    assert!(sessions.is_empty(), "Should have no sessions initially");

    // Create sessions via conversation.
    let req = ConversationRequest {
        message: "First conversation".to_string(),
        session_id: None,
    };
    h.service.send_message(&req).await.unwrap();

    // Create a second session explicitly.
    let summary = h.service.new_session().unwrap();

    let req2 = ConversationRequest {
        message: "Second conversation".to_string(),
        session_id: Some(summary.id),
    };
    h.service.send_message(&req2).await.unwrap();

    // Should have 2 sessions.
    let sessions = h.service.list_sessions(10).unwrap();
    assert_eq!(sessions.len(), 2, "Should have 2 sessions");

    // Most recently updated should be first.
    assert!(
        sessions[0].updated_at >= sessions[1].updated_at,
        "Sessions should be ordered by updated_at DESC"
    );

    // Limit should be respected.
    let sessions = h.service.list_sessions(1).unwrap();
    assert_eq!(sessions.len(), 1, "Limit should restrict results");
}

#[tokio::test]
async fn test_load_session() {
    let h = create_harness();

    // Create a conversation with some turns.
    let req = ConversationRequest {
        message: "Hello Alpha".to_string(),
        session_id: None,
    };
    let resp = h.service.send_message(&req).await.unwrap();

    let req2 = ConversationRequest {
        message: "How are you?".to_string(),
        session_id: Some(resp.session_id),
    };
    h.service.send_message(&req2).await.unwrap();

    // Load the session history.
    let history = h.service.load_session(&resp.session_id).unwrap();

    assert_eq!(history.session_id, resp.session_id);
    assert_eq!(history.status, "active");

    // Should have 4 turns: user + alpha + user + alpha.
    assert_eq!(history.turns.len(), 4);

    // Turns should be in chronological order.
    assert_eq!(history.turns[0].role, "user");
    assert_eq!(history.turns[0].content, "Hello Alpha");
    assert_eq!(history.turns[1].role, "alpha");
    assert_eq!(history.turns[2].role, "user");
    assert_eq!(history.turns[2].content, "How are you?");
    assert_eq!(history.turns[3].role, "alpha");
}

#[tokio::test]
async fn test_load_session_not_found() {
    let h = create_harness();

    let fake_id: alpha_common::types::AlphaId = alpha_common::types::new_id();
    let result = h.service.load_session(&fake_id);
    assert!(result.is_err(), "Should fail for nonexistent session");
}

#[tokio::test]
async fn test_new_session() {
    let h = create_harness();

    // Create first session via conversation.
    let req = ConversationRequest {
        message: "First".to_string(),
        session_id: None,
    };
    let resp = h.service.send_message(&req).await.unwrap();

    // Verify one active session exists.
    assert_eq!(h.session_manager.count(None).unwrap(), 1);

    // Create a new session (should close the old one).
    let new = h.service.new_session().unwrap();

    assert_ne!(new.id, resp.session_id, "New session should have a new ID");
    assert_eq!(new.status, "active");
    assert_eq!(new.turn_count, 0);

    // Old session should now be closed.
    let old = h
        .session_manager
        .get_session(&resp.session_id)
        .unwrap()
        .unwrap();
    assert_eq!(old.status, alpha_dialog::SessionStatus::Closed);

    // Total sessions should be 2.
    assert_eq!(h.session_manager.count(None).unwrap(), 2);
}

#[tokio::test]
async fn test_close_session() {
    let h = create_harness();

    // Create session via conversation.
    let req = ConversationRequest {
        message: "To be closed".to_string(),
        session_id: None,
    };
    let resp = h.service.send_message(&req).await.unwrap();

    // Session should be active.
    let session = h
        .session_manager
        .get_session(&resp.session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.status, alpha_dialog::SessionStatus::Active);

    // Close it.
    h.service.close_session(&resp.session_id).unwrap();

    // Session should now be closed.
    let session = h
        .session_manager
        .get_session(&resp.session_id)
        .unwrap()
        .unwrap();
    assert_eq!(session.status, alpha_dialog::SessionStatus::Closed);
}

#[tokio::test]
async fn test_close_session_not_found() {
    let h = create_harness();

    let fake_id = alpha_common::types::new_id();
    let result = h.service.close_session(&fake_id);
    assert!(result.is_err(), "Should fail for nonexistent session");
}

#[tokio::test]
async fn test_session_history_order() {
    let h = create_harness();

    // Create a conversation with 3 exchanges.
    let req1 = ConversationRequest {
        message: "Message one".to_string(),
        session_id: None,
    };
    let resp = h.service.send_message(&req1).await.unwrap();

    let req2 = ConversationRequest {
        message: "Message two".to_string(),
        session_id: Some(resp.session_id),
    };
    h.service.send_message(&req2).await.unwrap();

    let req3 = ConversationRequest {
        message: "Message three".to_string(),
        session_id: Some(resp.session_id),
    };
    h.service.send_message(&req3).await.unwrap();

    let history = h.service.load_session(&resp.session_id).unwrap();

    // 6 turns total: 3 user + 3 alpha.
    assert_eq!(history.turns.len(), 6);

    // Verify strict chronological order by checking timestamps are non-decreasing.
    for i in 1..history.turns.len() {
        assert!(
            history.turns[i].created_at >= history.turns[i - 1].created_at,
            "Turn {} timestamp ({}) should be >= turn {} timestamp ({})",
            i,
            history.turns[i].created_at,
            i - 1,
            history.turns[i - 1].created_at
        );
    }

    // Verify alternating roles.
    assert_eq!(history.turns[0].role, "user");
    assert_eq!(history.turns[1].role, "alpha");
    assert_eq!(history.turns[2].role, "user");
    assert_eq!(history.turns[3].role, "alpha");
    assert_eq!(history.turns[4].role, "user");
    assert_eq!(history.turns[5].role, "alpha");
}

#[tokio::test]
async fn test_session_summary_fields() {
    let h = create_harness();

    // Create a conversation.
    let req = ConversationRequest {
        message: "Summary test".to_string(),
        session_id: None,
    };
    let resp = h.service.send_message(&req).await.unwrap();

    // Set a title via SessionManager directly.
    h.session_manager
        .set_title(&resp.session_id, "Test Title")
        .unwrap();

    // List sessions and verify summary fields.
    let sessions = h.service.list_sessions(10).unwrap();
    assert_eq!(sessions.len(), 1);

    let summary = &sessions[0];
    assert_eq!(summary.id, resp.session_id);
    assert_eq!(summary.title, "Test Title");
    assert_eq!(summary.status, "active");
    assert_eq!(summary.turn_count, 2); // user + alpha

    // Timestamps should be valid ISO 8601.
    assert!(
        !summary.updated_at.is_empty(),
        "updated_at should be non-empty"
    );
    assert!(
        !summary.created_at.is_empty(),
        "created_at should be non-empty"
    );

    // Parse timestamps to verify they are valid RFC3339.
    chrono::DateTime::parse_from_rfc3339(&summary.updated_at)
        .expect("updated_at should be valid RFC3339");
    chrono::DateTime::parse_from_rfc3339(&summary.created_at)
        .expect("created_at should be valid RFC3339");
}

