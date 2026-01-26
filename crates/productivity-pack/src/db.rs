// ABOUTME: SQLite database layer for productivity-pack.
// ABOUTME: Manages todos and notes tables with CRUD operations.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{sqlite::SqlitePoolOptions, FromRow, SqlitePool};
use std::path::Path;

/// Escape SQL LIKE pattern metacharacters to prevent pattern injection.
fn escape_like_pattern(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Filter for listing todos by completion status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TodoFilter {
    /// Show all todos regardless of completion status.
    #[default]
    All,
    /// Show only pending (incomplete) todos.
    Pending,
    /// Show only completed todos.
    Done,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Todo {
    pub id: i64,
    pub title: String,
    pub due_date: Option<String>,
    pub completed: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Note {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub tags: String, // JSON array stored as string
    pub created_at: String,
    pub updated_at: String,
}

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let url = format!("sqlite:{}?mode=rwc", path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await?;

        let db = Self { pool };
        db.run_migrations().await?;
        Ok(db)
    }

    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS todos (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                due_date TEXT,
                completed BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                content TEXT NOT NULL,
                tags TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_notes_tags ON notes(tags)
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // Todo operations
    pub async fn add_todo(&self, title: &str, due_date: Option<&str>) -> Result<Todo> {
        let result = sqlx::query_as::<_, Todo>(
            r#"
            INSERT INTO todos (title, due_date)
            VALUES (?, ?)
            RETURNING id, title, due_date, completed, created_at
            "#,
        )
        .bind(title)
        .bind(due_date)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn list_todos(&self, filter: TodoFilter) -> Result<Vec<Todo>> {
        let todos = match filter {
            TodoFilter::All => {
                sqlx::query_as::<_, Todo>("SELECT * FROM todos ORDER BY created_at DESC")
                    .fetch_all(&self.pool)
                    .await?
            }
            TodoFilter::Pending => {
                sqlx::query_as::<_, Todo>(
                    "SELECT * FROM todos WHERE completed = FALSE ORDER BY created_at DESC",
                )
                .fetch_all(&self.pool)
                .await?
            }
            TodoFilter::Done => {
                sqlx::query_as::<_, Todo>(
                    "SELECT * FROM todos WHERE completed = TRUE ORDER BY created_at DESC",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(todos)
    }

    // Reserved for future todo_delete tool implementation
    #[allow(dead_code)]
    pub async fn delete_todo(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM todos WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn complete_todo(&self, id: i64) -> Result<Option<Todo>> {
        let result = sqlx::query_as::<_, Todo>(
            r#"
            UPDATE todos SET completed = TRUE
            WHERE id = ?
            RETURNING id, title, due_date, completed, created_at
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result)
    }

    // Note operations
    pub async fn create_note(&self, title: &str, content: &str, tags: &[String]) -> Result<Note> {
        let tags_json = serde_json::to_string(tags)?;
        let result = sqlx::query_as::<_, Note>(
            r#"
            INSERT INTO notes (title, content, tags)
            VALUES (?, ?, ?)
            RETURNING id, title, content, tags, created_at, updated_at
            "#,
        )
        .bind(title)
        .bind(content)
        .bind(&tags_json)
        .fetch_one(&self.pool)
        .await?;

        Ok(result)
    }

    pub async fn search_notes(&self, query: &str, tags: Option<&[String]>) -> Result<Vec<Note>> {
        // Escape LIKE metacharacters to prevent pattern injection
        let search_pattern = format!("%{}%", escape_like_pattern(query));

        // Use separate query paths to avoid dynamic SQL construction
        let notes = match tags {
            Some(tag_list) if !tag_list.is_empty() => {
                // Fetch notes matching the text query first
                let candidates = sqlx::query_as::<_, Note>(
                    r#"
                    SELECT * FROM notes
                    WHERE (title LIKE ? OR content LIKE ?)
                    ORDER BY updated_at DESC
                    "#,
                )
                .bind(&search_pattern)
                .bind(&search_pattern)
                .fetch_all(&self.pool)
                .await?;

                // Filter by tags in application code for proper JSON array matching
                candidates
                    .into_iter()
                    .filter(|note| {
                        // Parse the JSON tags array and check if all requested tags are present
                        if let Ok(note_tags) = serde_json::from_str::<Vec<String>>(&note.tags) {
                            tag_list.iter().all(|tag| note_tags.contains(tag))
                        } else {
                            false
                        }
                    })
                    .collect()
            }
            _ => {
                // No tags filter - just search by text
                sqlx::query_as::<_, Note>(
                    r#"
                    SELECT * FROM notes
                    WHERE (title LIKE ? OR content LIKE ?)
                    ORDER BY updated_at DESC
                    "#,
                )
                .bind(&search_pattern)
                .bind(&search_pattern)
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(notes)
    }

    pub async fn read_note(&self, id: i64) -> Result<Option<Note>> {
        let note = sqlx::query_as::<_, Note>("SELECT * FROM notes WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(note)
    }

    // Reserved for future note_delete tool implementation
    #[allow(dead_code)]
    pub async fn delete_note(&self, id: i64) -> Result<bool> {
        let result = sqlx::query("DELETE FROM notes WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }
}
