// ABOUTME: SQLite persistence for local gateway - simplified schema for super-trusted mode
// ABOUTME: Stores agents, conversations, and messages without auth/principal complexity

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use sqlx::{sqlite::SqlitePoolOptions, Pool, Row, Sqlite};
use std::path::Path;

/// Local gateway store backed by SQLite
#[derive(Clone)]
pub struct Store {
    pool: Pool<Sqlite>,
}

/// Connected agent info
#[derive(Debug, Clone)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub working_dir: String,
    pub connected: bool,
    pub connected_at: Option<DateTime<Utc>>,
    pub last_seen: Option<DateTime<Utc>>,
}

/// Conversation thread
#[derive(Debug, Clone)]
pub struct Conversation {
    pub id: String,
    pub agent_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Message in a conversation
#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub direction: String, // "inbound" or "outbound"
    pub author: String,
    pub content: String,
    pub message_type: String, // "message", "tool_use", "tool_result", "thinking"
    pub created_at: DateTime<Utc>,
}

/// Pack registration
#[derive(Debug, Clone)]
pub struct Pack {
    pub id: String,
    pub version: String,
    pub connected: bool,
    pub connected_at: Option<DateTime<Utc>>,
}

impl Store {
    /// Open or create the store at the given path
    pub async fn open(path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating directory: {}", parent.display()))?;
        }

        let url = format!("sqlite:{}?mode=rwc", path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .with_context(|| format!("opening database: {}", path.display()))?;

        let store = Self { pool };
        store.init_schema().await?;
        Ok(store)
    }

    /// Initialize database schema
    async fn init_schema(&self) -> Result<()> {
        // Enable WAL mode for better concurrency
        sqlx::query("PRAGMA journal_mode = WAL")
            .execute(&self.pool)
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                backend TEXT NOT NULL DEFAULT '',
                working_dir TEXT NOT NULL DEFAULT '',
                connected INTEGER NOT NULL DEFAULT 0,
                connected_at TEXT,
                last_seen TEXT
            );

            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_conversations_agent ON conversations(agent_id);

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                direction TEXT NOT NULL,
                author TEXT NOT NULL,
                content TEXT NOT NULL,
                message_type TEXT NOT NULL DEFAULT 'message',
                created_at TEXT NOT NULL,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id)
            );
            CREATE INDEX IF NOT EXISTS idx_messages_conversation ON messages(conversation_id, created_at);

            CREATE TABLE IF NOT EXISTS packs (
                id TEXT PRIMARY KEY,
                version TEXT NOT NULL,
                connected INTEGER NOT NULL DEFAULT 0,
                connected_at TEXT
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("initializing schema")?;

        Ok(())
    }

    // --- Agent operations ---

    /// Register or update an agent
    pub async fn upsert_agent(&self, agent: &Agent) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO agents (id, name, backend, working_dir, connected, connected_at, last_seen)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                backend = excluded.backend,
                working_dir = excluded.working_dir,
                connected = excluded.connected,
                connected_at = COALESCE(excluded.connected_at, agents.connected_at),
                last_seen = excluded.last_seen
            "#,
        )
        .bind(&agent.id)
        .bind(&agent.name)
        .bind(&agent.backend)
        .bind(&agent.working_dir)
        .bind(agent.connected)
        .bind(agent.connected_at.map(|t| t.to_rfc3339()))
        .bind(agent.last_seen.map(|t| t.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Mark agent as connected
    pub async fn set_agent_connected(&self, agent_id: &str, connected: bool) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        if connected {
            sqlx::query(
                "UPDATE agents SET connected = 1, connected_at = ?, last_seen = ? WHERE id = ?",
            )
            .bind(&now)
            .bind(&now)
            .bind(agent_id)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query("UPDATE agents SET connected = 0, last_seen = ? WHERE id = ?")
                .bind(&now)
                .bind(agent_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// List all agents
    pub async fn list_agents(&self) -> Result<Vec<Agent>> {
        let rows = sqlx::query(
            "SELECT id, name, backend, working_dir, connected, connected_at, last_seen FROM agents ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut agents = Vec::with_capacity(rows.len());
        for row in rows {
            agents.push(Agent {
                id: row.get("id"),
                name: row.get("name"),
                backend: row.get("backend"),
                working_dir: row.get("working_dir"),
                connected: row.get::<i32, _>("connected") != 0,
                connected_at: row
                    .get::<Option<String>, _>("connected_at")
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
                last_seen: row
                    .get::<Option<String>, _>("last_seen")
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
            });
        }
        Ok(agents)
    }

    /// Get agent by ID
    pub async fn get_agent(&self, agent_id: &str) -> Result<Option<Agent>> {
        let row = sqlx::query(
            "SELECT id, name, backend, working_dir, connected, connected_at, last_seen FROM agents WHERE id = ?",
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| Agent {
            id: row.get("id"),
            name: row.get("name"),
            backend: row.get("backend"),
            working_dir: row.get("working_dir"),
            connected: row.get::<i32, _>("connected") != 0,
            connected_at: row
                .get::<Option<String>, _>("connected_at")
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            last_seen: row
                .get::<Option<String>, _>("last_seen")
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
        }))
    }

    // --- Conversation operations ---

    /// Create a new conversation
    /// Note: In local gateway mode, we use agent_id as the conversation id for simplicity.
    /// This means there's one conversation per agent.
    pub async fn create_conversation(&self, agent_id: &str) -> Result<Conversation> {
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        // Use agent_id as conversation id for simplicity in local mode
        sqlx::query(
            "INSERT INTO conversations (id, agent_id, created_at, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(agent_id)
        .bind(agent_id)
        .bind(&now_str)
        .bind(&now_str)
        .execute(&self.pool)
        .await?;

        Ok(Conversation {
            id: agent_id.to_string(),
            agent_id: agent_id.to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Get or create conversation for an agent
    pub async fn get_or_create_conversation(&self, agent_id: &str) -> Result<Conversation> {
        // Try to get existing
        let row = sqlx::query(
            "SELECT id, agent_id, created_at, updated_at FROM conversations WHERE agent_id = ? ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(agent_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let created_str: String = row.get("created_at");
            let updated_str: String = row.get("updated_at");
            return Ok(Conversation {
                id: row.get("id"),
                agent_id: row.get("agent_id"),
                created_at: DateTime::parse_from_rfc3339(&created_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: DateTime::parse_from_rfc3339(&updated_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            });
        }

        // Create new
        self.create_conversation(agent_id).await
    }

    /// Touch conversation (update updated_at)
    pub async fn touch_conversation(&self, conversation_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query("UPDATE conversations SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(conversation_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // --- Message operations ---

    /// Save a message
    pub async fn save_message(&self, msg: &Message) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO messages (id, conversation_id, direction, author, content, message_type, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&msg.id)
        .bind(&msg.conversation_id)
        .bind(&msg.direction)
        .bind(&msg.author)
        .bind(&msg.content)
        .bind(&msg.message_type)
        .bind(msg.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        // Touch conversation
        self.touch_conversation(&msg.conversation_id).await?;

        Ok(())
    }

    /// Get messages for a conversation
    pub async fn get_messages(&self, conversation_id: &str, limit: i64) -> Result<Vec<Message>> {
        let rows = sqlx::query(
            r#"
            SELECT id, conversation_id, direction, author, content, message_type, created_at
            FROM messages
            WHERE conversation_id = ?
            ORDER BY created_at ASC
            LIMIT ?
            "#,
        )
        .bind(conversation_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut messages = Vec::with_capacity(rows.len());
        for row in rows {
            let created_str: String = row.get("created_at");
            messages.push(Message {
                id: row.get("id"),
                conversation_id: row.get("conversation_id"),
                direction: row.get("direction"),
                author: row.get("author"),
                content: row.get("content"),
                message_type: row.get("message_type"),
                created_at: DateTime::parse_from_rfc3339(&created_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            });
        }
        Ok(messages)
    }

    // --- Pack operations ---

    /// Register or update a pack
    pub async fn upsert_pack(&self, pack: &Pack) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO packs (id, version, connected, connected_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                version = excluded.version,
                connected = excluded.connected,
                connected_at = COALESCE(excluded.connected_at, packs.connected_at)
            "#,
        )
        .bind(&pack.id)
        .bind(&pack.version)
        .bind(pack.connected)
        .bind(pack.connected_at.map(|t| t.to_rfc3339()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Mark pack as connected/disconnected
    pub async fn set_pack_connected(&self, pack_id: &str, connected: bool) -> Result<()> {
        if connected {
            let now = Utc::now().to_rfc3339();
            sqlx::query("UPDATE packs SET connected = 1, connected_at = ? WHERE id = ?")
                .bind(&now)
                .bind(pack_id)
                .execute(&self.pool)
                .await?;
        } else {
            sqlx::query("UPDATE packs SET connected = 0 WHERE id = ?")
                .bind(pack_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// List all packs
    pub async fn list_packs(&self) -> Result<Vec<Pack>> {
        let rows =
            sqlx::query("SELECT id, version, connected, connected_at FROM packs ORDER BY id")
                .fetch_all(&self.pool)
                .await?;

        let mut packs = Vec::with_capacity(rows.len());
        for row in rows {
            packs.push(Pack {
                id: row.get("id"),
                version: row.get("version"),
                connected: row.get::<i32, _>("connected") != 0,
                connected_at: row
                    .get::<Option<String>, _>("connected_at")
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
            });
        }
        Ok(packs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use uuid::Uuid;

    async fn test_store() -> (Store, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.db");
        let store = Store::open(&path).await.unwrap();
        (store, dir)
    }

    #[tokio::test]
    async fn test_agent_crud() {
        let (store, _dir): (Store, TempDir) = test_store().await;

        // Create agent
        let agent = Agent {
            id: "test-agent".to_string(),
            name: "Test Agent".to_string(),
            backend: "mux".to_string(),
            working_dir: "/tmp".to_string(),
            connected: true,
            connected_at: Some(Utc::now()),
            last_seen: Some(Utc::now()),
        };
        store.upsert_agent(&agent).await.unwrap();

        // List agents
        let agents = store.list_agents().await.unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, "test-agent");
        assert!(agents[0].connected);

        // Get agent
        let fetched = store.get_agent("test-agent").await.unwrap().unwrap();
        assert_eq!(fetched.name, "Test Agent");

        // Disconnect
        store
            .set_agent_connected("test-agent", false)
            .await
            .unwrap();
        let fetched = store.get_agent("test-agent").await.unwrap().unwrap();
        assert!(!fetched.connected);
    }

    #[tokio::test]
    async fn test_conversation_and_messages() {
        let (store, _dir): (Store, TempDir) = test_store().await;

        // Create agent first
        let agent = Agent {
            id: "agent-1".to_string(),
            name: "Agent 1".to_string(),
            backend: "mux".to_string(),
            working_dir: "/tmp".to_string(),
            connected: true,
            connected_at: Some(Utc::now()),
            last_seen: Some(Utc::now()),
        };
        store.upsert_agent(&agent).await.unwrap();

        // Get or create conversation
        let conv = store.get_or_create_conversation("agent-1").await.unwrap();
        assert_eq!(conv.agent_id, "agent-1");

        // Save messages
        let msg1 = Message {
            id: Uuid::new_v4().to_string(),
            conversation_id: conv.id.clone(),
            direction: "inbound".to_string(),
            author: "user".to_string(),
            content: "Hello".to_string(),
            message_type: "message".to_string(),
            created_at: Utc::now(),
        };
        store.save_message(&msg1).await.unwrap();

        let msg2 = Message {
            id: Uuid::new_v4().to_string(),
            conversation_id: conv.id.clone(),
            direction: "outbound".to_string(),
            author: "agent".to_string(),
            content: "Hi there!".to_string(),
            message_type: "message".to_string(),
            created_at: Utc::now(),
        };
        store.save_message(&msg2).await.unwrap();

        // Get messages
        let messages = store.get_messages(&conv.id, 100).await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[1].content, "Hi there!");
    }
}
