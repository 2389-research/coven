// ABOUTME: Productivity pack providing todo and notes tools.
// ABOUTME: Uses SQLite for persistent storage.

mod db;
mod notes;
mod todo;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use coven_pack::{ManifestBuilder, PackClient, SchemaBuilder, ToolError, ToolHandler};
use coven_ssh::load_or_generate_key;
use db::{Database, TodoFilter};
use notes::{
    NoteCreateInput, NoteCreateOutput, NoteReadInput, NoteReadOutput, NoteSearchInput,
    NoteSearchOutput,
};
use std::path::PathBuf;
use std::sync::Arc;
use todo::{
    TodoAddInput, TodoAddOutput, TodoCompleteInput, TodoCompleteOutput, TodoListInput,
    TodoListOutput,
};
use tracing::info;

const PACK_NAME: &str = "productivity-pack";

/// Get XDG-style data directory (~/.local/share/coven).
fn xdg_data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("share")))
        .map(|p| p.join("coven"))
}

struct ProductivityHandler {
    db: Arc<Database>,
}

#[async_trait]
impl ToolHandler for ProductivityHandler {
    async fn execute(&self, tool_name: &str, input_json: &str) -> Result<String, ToolError> {
        info!("Executing tool: {} with input: {}", tool_name, input_json);

        match tool_name {
            "todo_add" => {
                let input: TodoAddInput = serde_json::from_str(input_json)
                    .map_err(|e| ToolError::InvalidInput(e.to_string()))?;

                let todo = self
                    .db
                    .add_todo(&input.title, input.due_date.as_deref())
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                let output = TodoAddOutput {
                    message: format!("Created todo #{}", todo.id),
                    todo,
                };

                serde_json::to_string(&output)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
            }

            "todo_list" => {
                let input: TodoListInput = serde_json::from_str(input_json)
                    .map_err(|e| ToolError::InvalidInput(e.to_string()))?;

                let filter = match input.filter.as_str() {
                    "pending" => TodoFilter::Pending,
                    "done" => TodoFilter::Done,
                    _ => TodoFilter::All,
                };

                let todos = self
                    .db
                    .list_todos(filter)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                let count = todos.len();
                let output = TodoListOutput { todos, count };

                serde_json::to_string(&output)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
            }

            "todo_complete" => {
                let input: TodoCompleteInput = serde_json::from_str(input_json)
                    .map_err(|e| ToolError::InvalidInput(e.to_string()))?;

                let todo = self
                    .db
                    .complete_todo(input.id)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                let message = match &todo {
                    Some(t) => format!("Completed todo #{}: {}", t.id, t.title),
                    None => format!("Todo #{} not found", input.id),
                };

                let output = TodoCompleteOutput { todo, message };

                serde_json::to_string(&output)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
            }

            "note_create" => {
                let input: NoteCreateInput = serde_json::from_str(input_json)
                    .map_err(|e| ToolError::InvalidInput(e.to_string()))?;

                let note = self
                    .db
                    .create_note(&input.title, &input.content, &input.tags)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                let output = NoteCreateOutput {
                    message: format!("Created note #{}", note.id),
                    note,
                };

                serde_json::to_string(&output)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
            }

            "note_search" => {
                let input: NoteSearchInput = serde_json::from_str(input_json)
                    .map_err(|e| ToolError::InvalidInput(e.to_string()))?;

                let notes = self
                    .db
                    .search_notes(&input.query, input.tags.as_deref())
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                let count = notes.len();
                let output = NoteSearchOutput { notes, count };

                serde_json::to_string(&output)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
            }

            "note_read" => {
                let input: NoteReadInput = serde_json::from_str(input_json)
                    .map_err(|e| ToolError::InvalidInput(e.to_string()))?;

                let note = self
                    .db
                    .read_note(input.id)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                let output = NoteReadOutput { note };

                serde_json::to_string(&output)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
            }

            _ => Err(ToolError::UnknownTool(tool_name.to_string())),
        }
    }
}

fn build_manifest() -> coven_proto::PackManifest {
    // Todo schemas
    let todo_add_schema = SchemaBuilder::object()
        .property(
            "title",
            SchemaBuilder::string().description("Title of the todo item"),
        )
        .property(
            "due_date",
            SchemaBuilder::string().description("Optional due date (ISO 8601 format)"),
        )
        .required(&["title"])
        .build();

    let todo_list_schema = SchemaBuilder::object()
        .property(
            "filter",
            SchemaBuilder::string()
                .description("Filter todos: 'all', 'pending', or 'done'")
                .default_value("all"),
        )
        .build();

    let todo_complete_schema = SchemaBuilder::object()
        .property(
            "id",
            SchemaBuilder::integer().description("ID of the todo to complete"),
        )
        .required(&["id"])
        .build();

    // Note schemas
    let note_create_schema = SchemaBuilder::object()
        .property(
            "title",
            SchemaBuilder::string().description("Title of the note"),
        )
        .property(
            "content",
            SchemaBuilder::string().description("Content of the note"),
        )
        .property(
            "tags",
            SchemaBuilder::array(SchemaBuilder::string()).description("Optional tags for the note"),
        )
        .required(&["title", "content"])
        .build();

    let note_search_schema = SchemaBuilder::object()
        .property(
            "query",
            SchemaBuilder::string().description("Search query for title and content"),
        )
        .property(
            "tags",
            SchemaBuilder::array(SchemaBuilder::string()).description("Optional tags to filter by"),
        )
        .required(&["query"])
        .build();

    let note_read_schema = SchemaBuilder::object()
        .property(
            "id",
            SchemaBuilder::integer().description("ID of the note to read"),
        )
        .required(&["id"])
        .build();

    ManifestBuilder::new(PACK_NAME, "0.1.0")
        .tool(
            "todo_add",
            "Add a todo item with optional due date",
            &todo_add_schema,
            &[],
        )
        .tool(
            "todo_list",
            "List todos with optional filter (all, pending, done)",
            &todo_list_schema,
            &[],
        )
        .tool(
            "todo_complete",
            "Mark a todo item as complete",
            &todo_complete_schema,
            &[],
        )
        .tool(
            "note_create",
            "Create a note with title, content, and optional tags",
            &note_create_schema,
            &[],
        )
        .tool(
            "note_search",
            "Search notes by query and optional tags",
            &note_search_schema,
            &[],
        )
        .tool("note_read", "Read a note by ID", &note_read_schema, &[])
        .build()
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("productivity_pack=info".parse()?),
        )
        .init();

    let config = coven_pack::PackConfig::load(PACK_NAME).map_err(|e| anyhow!("{}", e))?;

    // Use PRODUCTIVITY_DB_PATH env var if set, otherwise use XDG data path
    let db_path = std::env::var("PRODUCTIVITY_DB_PATH")
        .map(PathBuf::from)
        .or_else(|_| {
            xdg_data_dir()
                .map(|d| d.join("packs").join(PACK_NAME).join("productivity.db"))
                .ok_or_else(|| anyhow!("Could not determine data directory"))
        })?;

    info!("Starting {}", PACK_NAME);
    info!("Gateway: {}", config.gateway_url);
    info!("SSH key: {}", config.ssh_key_path.display());
    info!("Database: {}", db_path.display());

    // Load existing key or generate one
    let _private_key = load_or_generate_key(&config.ssh_key_path)?;

    let db = Database::new(&db_path).await?;
    let handler = ProductivityHandler { db: Arc::new(db) };
    let manifest = build_manifest();

    info!("Registering {} tools", manifest.tools.len());

    let client = PackClient::connect(&config.gateway_url, &config.ssh_key_path).await?;
    client.run(manifest, handler).await?;

    Ok(())
}
