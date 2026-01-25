// ABOUTME: SQLite-backed storage for threads, messages, and backend events
// ABOUTME: Handles persistence for conversation history and debugging

use crate::types::Thread;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::path::Path;
use std::str::FromStr;

/// Persistent storage for threads
pub struct ThreadStore {
    pool: SqlitePool,
}

impl ThreadStore {
    /// Open or create a thread store at the given path
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let url = format!("sqlite:{}?mode=rwc", path.as_ref().display());

        // Configure SQLite with foreign keys enabled for ALL connections in the pool
        let options = SqliteConnectOptions::from_str(&url)?
            .foreign_keys(true)
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        // Create tables if needed
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS threads (
                id TEXT PRIMARY KEY,
                claude_session_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                last_active TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Messages table - stores user and assistant messages
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Backend events table - stores all events from the backend
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS backend_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                event_data TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&pool)
        .await?;

        // Create indexes for efficient queries
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages(thread_id)")
            .execute(&pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_events_thread ON backend_events(thread_id)")
            .execute(&pool)
            .await?;

        Ok(Self { pool })
    }

    /// Get a thread by ID, or None if it doesn't exist
    pub async fn get(&self, thread_id: &str) -> Result<Option<Thread>> {
        let row = sqlx::query_as::<_, ThreadRow>(
            "SELECT id, claude_session_id, created_at, last_active FROM threads WHERE id = ?",
        )
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| r.into()))
    }

    /// Get or create a thread. If created, claude_session_id will be empty
    /// and caller should initialize it.
    /// Uses INSERT OR IGNORE to handle concurrent creation race conditions.
    pub async fn get_or_create(&self, thread_id: &str) -> Result<(Thread, bool)> {
        let now = Utc::now();

        // Try to insert first with INSERT OR IGNORE to handle race conditions
        let result = sqlx::query(
            "INSERT OR IGNORE INTO threads (id, claude_session_id, created_at, last_active) VALUES (?, ?, ?, ?)",
        )
        .bind(thread_id)
        .bind("")
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        // If rows_affected is 1, we created it; if 0, it already existed
        let is_new = result.rows_affected() == 1;

        // Now fetch the thread (guaranteed to exist)
        let thread = self
            .get(thread_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Thread should exist after INSERT OR IGNORE"))?;

        Ok((thread, is_new))
    }

    /// Update the Claude session ID for a thread
    pub async fn set_session_id(&self, thread_id: &str, session_id: &str) -> Result<()> {
        sqlx::query("UPDATE threads SET claude_session_id = ? WHERE id = ?")
            .bind(session_id)
            .bind(thread_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Update last_active timestamp
    pub async fn touch(&self, thread_id: &str) -> Result<()> {
        let now = Utc::now();
        sqlx::query("UPDATE threads SET last_active = ? WHERE id = ?")
            .bind(now.to_rfc3339())
            .bind(thread_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// List all threads, most recently active first
    pub async fn list(&self) -> Result<Vec<Thread>> {
        let rows = sqlx::query_as::<_, ThreadRow>(
            "SELECT id, claude_session_id, created_at, last_active FROM threads ORDER BY last_active DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// Delete a thread
    pub async fn delete(&self, thread_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM threads WHERE id = ?")
            .bind(thread_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Store a message in the conversation
    pub async fn add_message(&self, thread_id: &str, role: &str, content: &str) -> Result<i64> {
        let now = Utc::now();
        let result = sqlx::query(
            "INSERT INTO messages (thread_id, role, content, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(thread_id)
        .bind(role)
        .bind(content)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get all messages for a thread
    pub async fn get_messages(&self, thread_id: &str) -> Result<Vec<Message>> {
        let rows = sqlx::query_as::<_, MessageRow>(
            "SELECT id, thread_id, role, content, created_at FROM messages WHERE thread_id = ? ORDER BY id ASC",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// Store a backend event
    pub async fn add_event(
        &self,
        thread_id: &str,
        event_type: &str,
        event_data: &serde_json::Value,
    ) -> Result<i64> {
        let now = Utc::now();
        let result = sqlx::query(
            "INSERT INTO backend_events (thread_id, event_type, event_data, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(thread_id)
        .bind(event_type)
        .bind(event_data.to_string())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get all backend events for a thread
    pub async fn get_events(&self, thread_id: &str) -> Result<Vec<BackendEventLog>> {
        let rows = sqlx::query_as::<_, BackendEventRow>(
            "SELECT id, thread_id, event_type, event_data, created_at FROM backend_events WHERE thread_id = ? ORDER BY id ASC",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    /// Get recent backend events across all threads (for debugging)
    pub async fn get_recent_events(&self, limit: i64) -> Result<Vec<BackendEventLog>> {
        let rows = sqlx::query_as::<_, BackendEventRow>(
            "SELECT id, thread_id, event_type, event_data, created_at FROM backend_events ORDER BY id DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }
}

/// A message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: i64,
    pub thread_id: String,
    pub role: String, // "user" or "assistant"
    pub content: String,
    pub created_at: DateTime<Utc>,
}

/// A logged backend event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendEventLog {
    pub id: i64,
    pub thread_id: String,
    pub event_type: String,
    pub event_data: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct ThreadRow {
    id: String,
    claude_session_id: String,
    created_at: String,
    last_active: String,
}

impl From<ThreadRow> for Thread {
    fn from(row: ThreadRow) -> Self {
        Thread {
            id: row.id,
            claude_session_id: row.claude_session_id,
            created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            last_active: chrono::DateTime::parse_from_rfc3339(&row.last_active)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        }
    }
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: i64,
    thread_id: String,
    role: String,
    content: String,
    created_at: String,
}

impl From<MessageRow> for Message {
    fn from(row: MessageRow) -> Self {
        Message {
            id: row.id,
            thread_id: row.thread_id,
            role: row.role,
            content: row.content,
            created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        }
    }
}

#[derive(sqlx::FromRow)]
struct BackendEventRow {
    id: i64,
    thread_id: String,
    event_type: String,
    event_data: String,
    created_at: String,
}

impl From<BackendEventRow> for BackendEventLog {
    fn from(row: BackendEventRow) -> Self {
        BackendEventLog {
            id: row.id,
            thread_id: row.thread_id,
            event_type: row.event_type,
            event_data: serde_json::from_str(&row.event_data).unwrap_or(serde_json::Value::Null),
            created_at: chrono::DateTime::parse_from_rfc3339(&row.created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
        }
    }
}
