// ABOUTME: Message types for single mode chat history
// ABOUTME: Tracks user messages, agent responses, and tool activity

#![allow(dead_code)] // Types used by future tasks in this implementation series

use chrono::{DateTime, Local};

/// Who sent a message
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Role {
    User,
    Agent,
    System,
}

/// A tool execution record
#[derive(Debug, Clone)]
pub struct ToolExecution {
    pub id: String,
    pub name: String,
    pub input_preview: String,
    pub status: ToolStatus,
    pub output_preview: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToolStatus {
    Pending,   // Waiting for approval
    Executing, // Running
    Completed, // Finished successfully
    Failed,    // Finished with error
    Denied,    // User denied approval
}

/// A chat message with optional tool activity
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    pub timestamp: DateTime<Local>,
    pub tools: Vec<ToolExecution>,
    pub is_streaming: bool,
}

impl ChatMessage {
    pub fn user(content: String) -> Self {
        Self {
            role: Role::User,
            content,
            timestamp: Local::now(),
            tools: vec![],
            is_streaming: false,
        }
    }

    pub fn agent() -> Self {
        Self {
            role: Role::Agent,
            content: String::new(),
            timestamp: Local::now(),
            tools: vec![],
            is_streaming: true,
        }
    }

    pub fn system(content: String) -> Self {
        Self {
            role: Role::System,
            content,
            timestamp: Local::now(),
            tools: vec![],
            is_streaming: false,
        }
    }
}
