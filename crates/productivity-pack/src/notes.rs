// ABOUTME: Notes tool handlers for productivity-pack.
// ABOUTME: Implements create, search, and read operations.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct NoteCreateInput {
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct NoteSearchInput {
    pub query: String,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct NoteReadInput {
    pub id: i64,
}

#[derive(Debug, Serialize)]
pub struct NoteCreateOutput {
    pub note: crate::db::Note,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct NoteSearchOutput {
    pub notes: Vec<crate::db::Note>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct NoteReadOutput {
    pub note: Option<crate::db::Note>,
}
