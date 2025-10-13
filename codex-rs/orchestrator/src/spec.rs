//! Task specification and checklist models for orchestrator.

use serde::Deserialize;
use serde::Serialize;

/// Specification for a task to be executed by a child agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    /// Unique identifier for this task.
    pub id: String,
    /// Human-readable purpose of this task.
    pub purpose: String,
    /// Detailed prompt/instructions for the child agent.
    pub prompt: String,
    /// Checklist of items that must be completed.
    pub checklist: Vec<ChecklistItem>,
    /// Optional profile to use for this task.
    pub profile: Option<String>,
}

/// An item in a task checklist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistItem {
    /// Description of what needs to be done.
    pub description: String,
    /// Whether this item has been completed.
    #[serde(default)]
    pub completed: bool,
}

impl ChecklistItem {
    pub fn new(description: String) -> Self {
        Self {
            description,
            completed: false,
        }
    }
}

impl TaskSpec {
    pub fn new(purpose: String, prompt: String, checklist: Vec<ChecklistItem>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            purpose,
            prompt,
            checklist,
            profile: None,
        }
    }

    pub fn with_profile(mut self, profile: String) -> Self {
        self.profile = Some(profile);
        self
    }
}
