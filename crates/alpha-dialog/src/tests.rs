//! Tests for dialog session management.

use tempfile::TempDir;

use crate::session::SessionManager;
use crate::types::{SessionStatus, TurnRole};

// ── Helpers ──

fn test_manager() -> (TempDir, SessionManager) {
    let dir = TempDir::new().unwrap();
    let mgr = SessionManager::open(&dir.path().join("dialog.db")).unwrap();
    (dir, mgr)
}

// ── Tests ──

#[test]
fn test_create_session() {
    let (_dir, mgr) = test_manager();

    let id = mgr.create_session().unwrap();
    let session = mgr.get_session(&id).unwrap().expect("Session should exist");

    assert_eq!(session.id, id);
    assert_eq!(session.status, SessionStatus::Active);
    assert_eq!(session.title, "");
    assert_eq!(session.turn_count, 0);
}

#[test]
fn test_add_user_turn() {
    let (_dir, mgr) = test_manager();
    let session_id = mgr.create_session().unwrap();

    let turn = mgr.add_user_turn(&session_id, "Hello Alpha").unwrap();

    assert_eq!(turn.session_id, session_id);
    assert_eq!(turn.role, TurnRole::User);
    assert_eq!(turn.content, "Hello Alpha");
    assert_eq!(turn.turn_index, 0);
    assert_eq!(turn.tokens_used, 0);
    assert_eq!(turn.model_used, "");
    assert_eq!(turn.duration_ms, 0);

    // Session should be updated.
    let session = mgr.get_session(&session_id).unwrap().unwrap();
    assert_eq!(session.turn_count, 1);

    // Second user turn should have index 1.
    let turn2 = mgr.add_user_turn(&session_id, "Another message").unwrap();
    assert_eq!(turn2.turn_index, 1);

    let session = mgr.get_session(&session_id).unwrap().unwrap();
    assert_eq!(session.turn_count, 2);
}

#[test]
fn test_add_alpha_turn() {
    let (_dir, mgr) = test_manager();
    let session_id = mgr.create_session().unwrap();

    // User turn first.
    mgr.add_user_turn(&session_id, "What is Rust?").unwrap();

    // Alpha response.
    let turn = mgr
        .add_alpha_turn(
            &session_id,
            "Rust is a systems programming language.",
            "llama3.1:8b",
            42,
            1500,
        )
        .unwrap();

    assert_eq!(turn.role, TurnRole::Alpha);
    assert_eq!(turn.content, "Rust is a systems programming language.");
    assert_eq!(turn.turn_index, 1);
    assert_eq!(turn.tokens_used, 42);
    assert_eq!(turn.model_used, "llama3.1:8b");
    assert_eq!(turn.duration_ms, 1500);

    // Session should reflect 2 turns.
    let session = mgr.get_session(&session_id).unwrap().unwrap();
    assert_eq!(session.turn_count, 2);
}

#[test]
fn test_get_recent_turns() {
    let (_dir, mgr) = test_manager();
    let session_id = mgr.create_session().unwrap();

    // Add 5 turns.
    for i in 0..5 {
        mgr.add_user_turn(&session_id, &format!("Message {i}"))
            .unwrap();
    }

    // Get most recent 3.
    let recent = mgr.get_recent_turns(&session_id, 3).unwrap();
    assert_eq!(recent.len(), 3);

    // Most recent first: turn_index 4, 3, 2.
    assert_eq!(recent[0].turn_index, 4);
    assert_eq!(recent[0].content, "Message 4");
    assert_eq!(recent[1].turn_index, 3);
    assert_eq!(recent[1].content, "Message 3");
    assert_eq!(recent[2].turn_index, 2);
    assert_eq!(recent[2].content, "Message 2");

    // Get more than available.
    let all_recent = mgr.get_recent_turns(&session_id, 100).unwrap();
    assert_eq!(all_recent.len(), 5);
    assert_eq!(all_recent[0].turn_index, 4);
    assert_eq!(all_recent[4].turn_index, 0);
}

#[test]
fn test_get_turns_chronological() {
    let (_dir, mgr) = test_manager();
    let session_id = mgr.create_session().unwrap();

    mgr.add_user_turn(&session_id, "First").unwrap();
    mgr.add_alpha_turn(&session_id, "Second", "model", 10, 100)
        .unwrap();
    mgr.add_user_turn(&session_id, "Third").unwrap();

    let turns = mgr.get_turns(&session_id).unwrap();
    assert_eq!(turns.len(), 3);

    // Chronological order: index 0, 1, 2.
    assert_eq!(turns[0].turn_index, 0);
    assert_eq!(turns[0].content, "First");
    assert_eq!(turns[0].role, TurnRole::User);

    assert_eq!(turns[1].turn_index, 1);
    assert_eq!(turns[1].content, "Second");
    assert_eq!(turns[1].role, TurnRole::Alpha);

    assert_eq!(turns[2].turn_index, 2);
    assert_eq!(turns[2].content, "Third");
    assert_eq!(turns[2].role, TurnRole::User);
}

#[test]
fn test_close_session() {
    let (_dir, mgr) = test_manager();
    let session_id = mgr.create_session().unwrap();

    // Session starts active.
    let session = mgr.get_session(&session_id).unwrap().unwrap();
    assert_eq!(session.status, SessionStatus::Active);

    // Close it.
    mgr.close_session(&session_id).unwrap();

    let session = mgr.get_session(&session_id).unwrap().unwrap();
    assert_eq!(session.status, SessionStatus::Closed);

    // updated_at should have changed.
    assert!(session.updated_at >= session.created_at);
}

#[test]
fn test_get_or_create_active() {
    let (_dir, mgr) = test_manager();

    // No sessions exist — should create one.
    let session1 = mgr.get_or_create_active().unwrap();
    assert_eq!(session1.status, SessionStatus::Active);

    // Should return the same session (it's still active).
    let session2 = mgr.get_or_create_active().unwrap();
    assert_eq!(session1.id, session2.id);

    // Close the session.
    mgr.close_session(&session1.id).unwrap();

    // Should create a new one since no active session exists.
    let session3 = mgr.get_or_create_active().unwrap();
    assert_ne!(session3.id, session1.id);
    assert_eq!(session3.status, SessionStatus::Active);
}

#[test]
fn test_list_sessions() {
    let (_dir, mgr) = test_manager();

    let id1 = mgr.create_session().unwrap();
    let _id2 = mgr.create_session().unwrap();
    let id3 = mgr.create_session().unwrap();

    // Close one so we have mixed statuses.
    mgr.close_session(&id1).unwrap();

    // List all.
    let all = mgr.list_sessions(None, 100, 0).unwrap();
    assert_eq!(all.len(), 3);

    // Most recently updated first (id1 was just closed, so it's most recent).
    assert_eq!(all[0].id, id1);

    // List active only.
    let active = mgr
        .list_sessions(Some(&SessionStatus::Active), 100, 0)
        .unwrap();
    assert_eq!(active.len(), 2);
    assert!(active.iter().all(|s| s.status == SessionStatus::Active));

    // List closed only.
    let closed = mgr
        .list_sessions(Some(&SessionStatus::Closed), 100, 0)
        .unwrap();
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].id, id1);

    // Pagination.
    let page = mgr.list_sessions(None, 2, 0).unwrap();
    assert_eq!(page.len(), 2);

    let page2 = mgr.list_sessions(None, 2, 2).unwrap();
    assert_eq!(page2.len(), 1);

    // Offset beyond total.
    let empty = mgr.list_sessions(None, 10, 100).unwrap();
    assert!(empty.is_empty());

    // Verify updated_at ordering: add a turn to id3, making it the most recent.
    mgr.add_user_turn(&id3, "Update id3").unwrap();
    let ordered = mgr.list_sessions(None, 100, 0).unwrap();
    assert_eq!(ordered[0].id, id3, "Session with most recent turn should be first");
}

#[test]
fn test_set_title() {
    let (_dir, mgr) = test_manager();
    let session_id = mgr.create_session().unwrap();

    // Title starts empty.
    let session = mgr.get_session(&session_id).unwrap().unwrap();
    assert_eq!(session.title, "");

    // Set title.
    mgr.set_title(&session_id, "Chat about Rust").unwrap();

    let session = mgr.get_session(&session_id).unwrap().unwrap();
    assert_eq!(session.title, "Chat about Rust");
    assert!(session.updated_at >= session.created_at);

    // Update title.
    mgr.set_title(&session_id, "Rust Deep Dive").unwrap();
    let session = mgr.get_session(&session_id).unwrap().unwrap();
    assert_eq!(session.title, "Rust Deep Dive");
}

#[test]
fn test_count() {
    let (_dir, mgr) = test_manager();

    assert_eq!(mgr.count(None).unwrap(), 0);

    mgr.create_session().unwrap();
    mgr.create_session().unwrap();
    let id3 = mgr.create_session().unwrap();

    assert_eq!(mgr.count(None).unwrap(), 3);
    assert_eq!(mgr.count(Some(&SessionStatus::Active)).unwrap(), 3);
    assert_eq!(mgr.count(Some(&SessionStatus::Closed)).unwrap(), 0);

    mgr.close_session(&id3).unwrap();
    assert_eq!(mgr.count(Some(&SessionStatus::Active)).unwrap(), 2);
    assert_eq!(mgr.count(Some(&SessionStatus::Closed)).unwrap(), 1);
}

#[test]
fn test_persistence_across_restart() {
    let dir = TempDir::new().unwrap();
    let db_path = dir.path().join("dialog.db");

    let session_id;
    let turn_id;

    // Session 1: create session and add turns.
    {
        let mgr = SessionManager::open(&db_path).unwrap();
        session_id = mgr.create_session().unwrap();
        mgr.set_title(&session_id, "Persistent Chat").unwrap();
        let turn = mgr.add_user_turn(&session_id, "Remember me").unwrap();
        turn_id = turn.id;
        mgr.add_alpha_turn(&session_id, "I will!", "llama3.1:8b", 5, 200)
            .unwrap();
    }

    // Session 2: reopen and verify everything persisted.
    {
        let mgr = SessionManager::open(&db_path).unwrap();

        let session = mgr
            .get_session(&session_id)
            .unwrap()
            .expect("Session should persist across restart");

        assert_eq!(session.title, "Persistent Chat");
        assert_eq!(session.status, SessionStatus::Active);
        assert_eq!(session.turn_count, 2);

        let turns = mgr.get_turns(&session_id).unwrap();
        assert_eq!(turns.len(), 2);

        assert_eq!(turns[0].id, turn_id);
        assert_eq!(turns[0].content, "Remember me");
        assert_eq!(turns[0].role, TurnRole::User);

        assert_eq!(turns[1].content, "I will!");
        assert_eq!(turns[1].role, TurnRole::Alpha);
        assert_eq!(turns[1].model_used, "llama3.1:8b");
        assert_eq!(turns[1].tokens_used, 5);
        assert_eq!(turns[1].duration_ms, 200);

        assert_eq!(mgr.count(None).unwrap(), 1);
    }
}

#[test]
fn test_wal_mode_enabled() {
    let (_dir, mgr) = test_manager();
    assert!(mgr.is_wal_mode().unwrap(), "WAL mode should be enabled");
}
