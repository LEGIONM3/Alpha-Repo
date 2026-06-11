//! Types for dialog management.

use alpha_common::types::{AlphaId, JsonValue, Timestamp};
use serde::{Deserialize, Serialize};

/// Role in a dialog turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnRole {
    /// The human user.
    User,
    /// Alpha's response.
    Alpha,
}

impl TurnRole {
    /// Convert to the SQL string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            TurnRole::User => "user",
            TurnRole::Alpha => "alpha",
        }
    }

    /// Parse from a SQL string representation.
    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "user" => Some(TurnRole::User),
            "alpha" => Some(TurnRole::Alpha),
            _ => None,
        }
    }
}

/// Session status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Session is currently active and accepting turns.
    Active,
    /// Session has been closed.
    Closed,
}

impl SessionStatus {
    /// Convert to the SQL string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionStatus::Active => "active",
            SessionStatus::Closed => "closed",
        }
    }

    /// Parse from a SQL string representation.
    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(SessionStatus::Active),
            "closed" => Some(SessionStatus::Closed),
            _ => None,
        }
    }
}

/// A single turn in a conversation.
#[derive(Debug, Clone)]
pub struct DialogTurn {
    /// Unique turn identifier.
    pub id: AlphaId,
    /// The session this turn belongs to.
    pub session_id: AlphaId,
    /// Who spoke (user or alpha).
    pub role: TurnRole,
    /// The text content of the turn.
    pub content: String,
    /// Zero-based turn index within the session.
    pub turn_index: u32,
    /// Number of tokens used (for Alpha turns).
    pub tokens_used: u32,
    /// Model used for generation (for Alpha turns).
    pub model_used: String,
    /// Response generation time in milliseconds (for Alpha turns).
    pub duration_ms: u64,
    /// When this turn was created.
    pub created_at: Timestamp,
    /// Extensible metadata.
    pub metadata: JsonValue,
}

/// A conversation session.
#[derive(Debug, Clone)]
pub struct DialogSession {
    /// Unique session identifier.
    pub id: AlphaId,
    /// Session title (may be empty until set or auto-generated).
    pub title: String,
    /// Current session status.
    pub status: SessionStatus,
    /// When this session was created.
    pub created_at: Timestamp,
    /// When this session was last updated (turn added or status changed).
    pub updated_at: Timestamp,
    /// Number of turns in this session.
    pub turn_count: u32,
    /// Extensible metadata.
    pub metadata: JsonValue,
}
