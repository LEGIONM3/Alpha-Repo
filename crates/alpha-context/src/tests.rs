//! Tests for context assembly.

use std::path::Path;

use alpha_common::schemas::identity::{AlphaIdentity, Personality};
use alpha_common::schemas::memory::{MemoryRecord, MemoryType};
use alpha_common::schemas::relationship::{
    RelationshipCategory, RelationshipCoreRecord, RelationshipSource,
};
use alpha_knowledge::{KnowledgeEntity, KnowledgeStore};
use alpha_memory::MemoryStore;
use alpha_relationship::RelationshipStore;
use tempfile::TempDir;

use crate::assembler::ContextAssembler;
use crate::types::{ContextConfig, ConversationTurn, TurnRole};

// ── Helpers ──

/// Create all three stores in a temporary directory.
fn create_stores(dir: &Path) -> (MemoryStore, RelationshipStore, KnowledgeStore) {
    let mem = MemoryStore::open(&dir.join("memory.db"), 3).unwrap();
    let rel = RelationshipStore::open(&dir.join("relationship.db")).unwrap();
    let know = KnowledgeStore::open(&dir.join("knowledge.db")).unwrap();
    (mem, rel, know)
}

/// Create a test identity with customizable personality.
fn test_identity() -> AlphaIdentity {
    let mut identity = AlphaIdentity::new("test_constitution_hash".to_string());
    identity.personality = Personality {
        name: "TestAlpha".to_string(),
        tone: "warm".to_string(),
        verbosity: 0.8,
    };
    identity
}

/// Store a memory record with a 3-dimensional embedding.
fn store_test_memory(store: &MemoryStore, content: &str, embedding: Vec<f32>) {
    let mut record = MemoryRecord::new(
        MemoryType::Episodic,
        content.to_string(),
        "test".to_string(),
        0.8,
    );
    record.embedding = embedding;
    store.store_with_embedding(&record).unwrap();
}

/// Store a relationship record.
fn store_test_relationship(store: &RelationshipStore, content: &str) {
    let record = RelationshipCoreRecord::new(
        RelationshipCategory::CommunicationPref,
        content.to_string(),
        RelationshipSource::UserExplicit,
        0.9,
    );
    store.store_with_embedding(&record).unwrap();
}

/// Store a knowledge entity.
fn store_test_knowledge(store: &KnowledgeStore, name: &str, description: &str) {
    let entity = KnowledgeEntity::new("person", name, "test")
        .with_description(description);
    store.store(&entity).unwrap();
}

/// Build sample conversation history.
fn sample_history() -> Vec<ConversationTurn> {
    vec![
        ConversationTurn {
            role: TurnRole::User,
            content: "Hello Alpha, how are you?".to_string(),
        },
        ConversationTurn {
            role: TurnRole::Alpha,
            content: "I'm doing well, thank you for asking!".to_string(),
        },
    ]
}

// ── Tests ──

#[test]
fn test_system_prompt_contains_identity() {
    let dir = TempDir::new().unwrap();
    let (mem, rel, know) = create_stores(dir.path());
    let identity = test_identity();
    let config = ContextConfig::default();
    let assembler = ContextAssembler::new(&mem, &rel, &know, &identity, config);

    let prompt = assembler.build_system_prompt();

    assert!(
        prompt.contains("TestAlpha"),
        "System prompt must contain Alpha's name"
    );
    assert!(
        prompt.contains("warm"),
        "System prompt must contain tone"
    );
    assert!(
        prompt.contains("0.8"),
        "System prompt must contain verbosity"
    );
    assert!(
        prompt.contains("Constitution"),
        "System prompt must reference the constitution"
    );
    assert!(
        prompt.contains("personal AI companion"),
        "System prompt must describe Alpha's role"
    );
}

#[test]
fn test_empty_stores_produces_context() {
    let dir = TempDir::new().unwrap();
    let (mem, rel, know) = create_stores(dir.path());
    let identity = test_identity();
    let config = ContextConfig::default();
    let assembler = ContextAssembler::new(&mem, &rel, &know, &identity, config);

    let result = assembler.assemble("hello", &[], &[]).unwrap();

    // System prompt should always be present.
    assert!(!result.system_prompt.is_empty());
    assert!(result.system_prompt.contains("TestAlpha"));

    // No blocks from empty stores and no history.
    assert!(
        result.blocks.is_empty(),
        "Empty stores + no history should produce no context blocks"
    );

    // IDs should be empty.
    assert!(result.memory_ids_used.is_empty());
    assert!(result.relationship_ids_used.is_empty());

    // Total tokens should be just the system prompt.
    assert!(result.total_estimated_tokens > 0);
}

#[test]
fn test_memory_block_included() {
    let dir = TempDir::new().unwrap();
    let (mem, rel, know) = create_stores(dir.path());
    let identity = test_identity();

    // Store a memory with a known embedding.
    store_test_memory(&mem, "The user enjoys hiking on weekends", vec![1.0, 0.0, 0.0]);
    store_test_memory(&mem, "The user works as a software engineer", vec![0.0, 1.0, 0.0]);

    let config = ContextConfig {
        similarity_threshold: 0.0, // Accept all similarities.
        ..Default::default()
    };
    let assembler = ContextAssembler::new(&mem, &rel, &know, &identity, config);

    // Query with an embedding identical to the first memory.
    let result = assembler
        .assemble("hobbies", &[1.0, 0.0, 0.0], &[])
        .unwrap();

    // Should have a memories block.
    let memories_block = result
        .blocks
        .iter()
        .find(|b| b.label == "memories")
        .expect("Memories block should be present");

    assert!(memories_block.content.contains("hiking"));
    assert!(memories_block.estimated_tokens > 0);
    assert!(!result.memory_ids_used.is_empty());
}

#[test]
fn test_relationship_block_included() {
    let dir = TempDir::new().unwrap();
    let (mem, rel, know) = create_stores(dir.path());
    let identity = test_identity();

    store_test_relationship(&rel, "User prefers concise responses");
    store_test_relationship(&rel, "User likes technical depth");

    let config = ContextConfig::default();
    let assembler = ContextAssembler::new(&mem, &rel, &know, &identity, config);

    let result = assembler.assemble("hello", &[], &[]).unwrap();

    let rel_block = result
        .blocks
        .iter()
        .find(|b| b.label == "relationship")
        .expect("Relationship block should be present");

    assert!(rel_block.content.contains("concise responses"));
    assert!(rel_block.content.contains("## About the User"));
    assert!(!result.relationship_ids_used.is_empty());
    assert_eq!(result.relationship_ids_used.len(), 2);
}

#[test]
fn test_knowledge_block_included() {
    let dir = TempDir::new().unwrap();
    let (mem, rel, know) = create_stores(dir.path());
    let identity = test_identity();

    store_test_knowledge(&know, "Alice", "Best friend of the user");
    store_test_knowledge(&know, "Rust", "A systems programming language");

    let config = ContextConfig::default();
    let assembler = ContextAssembler::new(&mem, &rel, &know, &identity, config);

    let result = assembler.assemble("hello", &[], &[]).unwrap();

    let know_block = result
        .blocks
        .iter()
        .find(|b| b.label == "knowledge")
        .expect("Knowledge block should be present");

    assert!(know_block.content.contains("Alice"));
    assert!(know_block.content.contains("## Known Entities"));
    assert!(know_block.estimated_tokens > 0);
}

#[test]
fn test_history_block_included() {
    let dir = TempDir::new().unwrap();
    let (mem, rel, know) = create_stores(dir.path());
    let identity = test_identity();

    let history = sample_history();
    let config = ContextConfig::default();
    let assembler = ContextAssembler::new(&mem, &rel, &know, &identity, config);

    let result = assembler.assemble("hello", &[], &history).unwrap();

    let hist_block = result
        .blocks
        .iter()
        .find(|b| b.label == "history")
        .expect("History block should be present");

    assert!(hist_block.content.contains("User: Hello Alpha"));
    assert!(hist_block.content.contains("Alpha: I'm doing well"));
    assert!(hist_block.content.contains("## Recent Conversation"));
    assert!(hist_block.estimated_tokens > 0);
}

#[test]
fn test_token_budget_respected() {
    let dir = TempDir::new().unwrap();
    let (mem, rel, know) = create_stores(dir.path());
    let identity = test_identity();

    // Add substantial content to all stores.
    let long_content = "a]".repeat(200); // 400 chars
    store_test_relationship(&rel, &long_content);
    store_test_knowledge(&know, "BigEntity", &long_content);
    store_test_memory(&mem, &long_content, vec![1.0, 0.0, 0.0]);

    let long_history = vec![
        ConversationTurn {
            role: TurnRole::User,
            content: "x".repeat(400),
        },
        ConversationTurn {
            role: TurnRole::Alpha,
            content: "y".repeat(400),
        },
    ];

    // Set a tight budget.
    let config = ContextConfig {
        max_tokens: 200,
        chars_per_token: 4,
        similarity_threshold: 0.0,
        ..Default::default()
    };
    let assembler = ContextAssembler::new(&mem, &rel, &know, &identity, config);

    let result = assembler
        .assemble("test", &[1.0, 0.0, 0.0], &long_history)
        .unwrap();

    // Total estimated tokens must not exceed the budget.
    assert!(
        result.total_estimated_tokens <= 200,
        "Total tokens {} must not exceed budget 200",
        result.total_estimated_tokens
    );
}

#[test]
fn test_truncation_priority_order() {
    let dir = TempDir::new().unwrap();
    let (mem, rel, know) = create_stores(dir.path());
    let identity = test_identity();

    // Add content to all sources with known sizes.
    // Each block will be at least 100+ chars.
    let medium_content = "x".repeat(100);
    store_test_relationship(&rel, &medium_content);
    store_test_memory(&mem, &medium_content, vec![1.0, 0.0, 0.0]);
    store_test_knowledge(&know, "Entity", &medium_content);

    let history = vec![ConversationTurn {
        role: TurnRole::User,
        content: medium_content.clone(),
    }];

    // First, measure sizes with a generous budget.
    let generous_config = ContextConfig {
        max_tokens: 100_000,
        chars_per_token: 4,
        similarity_threshold: 0.0,
        ..Default::default()
    };
    let generous = ContextAssembler::new(&mem, &rel, &know, &identity, generous_config);
    let full_result = generous
        .assemble("test", &[1.0, 0.0, 0.0], &history)
        .unwrap();

    // Verify all 4 blocks are present in the full result.
    let full_labels: Vec<&str> = full_result
        .blocks
        .iter()
        .map(|b| b.label.as_str())
        .collect();
    assert!(full_labels.contains(&"relationship"));
    assert!(full_labels.contains(&"memories"));
    assert!(full_labels.contains(&"history"));
    assert!(full_labels.contains(&"knowledge"));

    // Calculate a budget that fits system + relationship + memories only.
    let sys_tokens = generous.estimate_tokens(&full_result.system_prompt);
    let rel_tokens: usize = full_result
        .blocks
        .iter()
        .filter(|b| b.label == "relationship")
        .map(|b| b.estimated_tokens)
        .sum();
    let mem_tokens: usize = full_result
        .blocks
        .iter()
        .filter(|b| b.label == "memories")
        .map(|b| b.estimated_tokens)
        .sum();

    // Budget: enough for system + relationship + memories, but NOT history + knowledge.
    let tight_budget = sys_tokens + rel_tokens + mem_tokens;
    let tight_config = ContextConfig {
        max_tokens: tight_budget,
        chars_per_token: 4,
        similarity_threshold: 0.0,
        ..Default::default()
    };
    let tight = ContextAssembler::new(&mem, &rel, &know, &identity, tight_config);
    let result = tight
        .assemble("test", &[1.0, 0.0, 0.0], &history)
        .unwrap();

    let labels: Vec<&str> = result.blocks.iter().map(|b| b.label.as_str()).collect();

    // Knowledge (lowest priority) should be removed first.
    assert!(
        !labels.contains(&"knowledge"),
        "Knowledge should be removed first (lowest priority)"
    );

    // History (second lowest) should also be removed.
    assert!(
        !labels.contains(&"history"),
        "History should be removed before memories"
    );

    // Relationship (highest priority) should survive.
    assert!(
        labels.contains(&"relationship"),
        "Relationship should survive (highest retrieval priority)"
    );

    // Memories should survive.
    assert!(
        labels.contains(&"memories"),
        "Memories should survive (higher priority than history/knowledge)"
    );

    // System prompt is always present.
    assert!(!result.system_prompt.is_empty());

    // Budget must be respected.
    assert!(result.total_estimated_tokens <= tight_budget);
}

#[test]
fn test_token_estimation() {
    let dir = TempDir::new().unwrap();
    let (mem, rel, know) = create_stores(dir.path());
    let identity = test_identity();

    // Default chars_per_token = 4.
    let config = ContextConfig::default();
    let assembler = ContextAssembler::new(&mem, &rel, &know, &identity, config);

    // 12 chars / 4 = 3 tokens.
    assert_eq!(assembler.estimate_tokens("hello world!"), 3);

    // 8 chars / 4 = 2 tokens.
    assert_eq!(assembler.estimate_tokens("12345678"), 2);

    // 3 chars / 4 = 0 tokens (integer division).
    assert_eq!(assembler.estimate_tokens("abc"), 0);

    // Empty string = 0 tokens.
    assert_eq!(assembler.estimate_tokens(""), 0);

    // Custom chars_per_token.
    let config2 = ContextConfig {
        chars_per_token: 2,
        ..Default::default()
    };
    let assembler2 = ContextAssembler::new(&mem, &rel, &know, &identity, config2);

    // 8 chars / 2 = 4 tokens.
    assert_eq!(assembler2.estimate_tokens("12345678"), 4);
}
