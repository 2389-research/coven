// ABOUTME: Todo tool handlers for productivity-pack.
// ABOUTME: Implements add, list, and complete operations.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct TodoAddInput {
    pub title: String,
    pub due_date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TodoListInput {
    #[serde(default = "default_filter")]
    pub filter: String, // Will be converted to TodoFilter enum in handler
}

fn default_filter() -> String {
    "all".to_string()
}

#[derive(Debug, Deserialize)]
pub struct TodoCompleteInput {
    pub id: i64,
}

#[derive(Debug, Serialize)]
pub struct TodoAddOutput {
    pub todo: crate::db::Todo,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct TodoListOutput {
    pub todos: Vec<crate::db::Todo>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct TodoCompleteOutput {
    pub todo: Option<crate::db::Todo>,
    pub message: String,
}
