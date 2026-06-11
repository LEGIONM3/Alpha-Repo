//! # alpha-dialog
//!
//! Conversation session and multi-turn history management for Project Alpha.
//!
//! ## Features
//!
//! - **Sessions**: Create, close, and list conversation sessions
//! - **Turns**: Record user and Alpha turns with metadata (model, tokens, duration)
//! - **Persistence**: SQLite-backed with WAL mode for concurrent safety
//! - **Retrieval**: Recent turns (reverse chronological) and full history (chronological)
//!
//! ## Database
//!
//! Stores data in `dialog.db` with two tables:
//! - `sessions` — conversation session lifecycle
//! - `turns` — individual dialog turns within sessions

pub mod session;
pub mod types;

pub use session::SessionManager;
pub use types::{DialogSession, DialogTurn, SessionStatus, TurnRole};

#[cfg(test)]
mod tests;
