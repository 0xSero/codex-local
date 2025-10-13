//! Agent view structure for multi-agent orchestration UI.

use crate::chatwidget::ChatWidget;
use codex_protocol::ConversationId;
use codex_protocol::protocol::TokenUsage;

/// Status of an agent in the orchestrator.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Foundation for future multi-agent UI implementation
pub enum AgentStatus {
    /// Agent is actively working
    Active,
    /// Agent is waiting for work
    Idle,
    /// Agent has completed successfully
    Completed,
    /// Agent failed or was terminated
    Failed,
}

/// Represents a single agent in the multi-agent orchestrator view.
#[allow(dead_code)] // Foundation for future multi-agent UI implementation
pub struct AgentView {
    /// Unique identifier for this agent
    pub id: String,
    /// Display name for the agent
    pub name: String,
    /// The chat widget managing this agent's conversation
    pub chat_widget: ChatWidget,
    /// Current status of the agent
    pub status: AgentStatus,
    /// Profile name used by this agent
    pub profile: Option<String>,
    /// Parent agent ID (if this is a child agent)
    pub parent_id: Option<String>,
    /// Purpose/task description for this agent
    pub purpose: String,
}

#[allow(dead_code)] // Foundation for future multi-agent UI implementation
impl AgentView {
    /// Creates a new agent view.
    pub fn new(
        id: String,
        name: String,
        chat_widget: ChatWidget,
        profile: Option<String>,
        parent_id: Option<String>,
        purpose: String,
    ) -> Self {
        Self {
            id,
            name,
            chat_widget,
            status: AgentStatus::Active,
            profile,
            parent_id,
            purpose,
        }
    }

    /// Returns the conversation ID for this agent.
    pub fn conversation_id(&self) -> Option<ConversationId> {
        self.chat_widget.conversation_id()
    }

    /// Returns the token usage for this agent.
    pub fn token_usage(&self) -> TokenUsage {
        self.chat_widget.token_usage()
    }

    /// Returns whether this agent is the orchestrator (has no parent).
    pub fn is_orchestrator(&self) -> bool {
        self.parent_id.is_none()
    }

    /// Updates the agent's status.
    pub fn set_status(&mut self, status: AgentStatus) {
        self.status = status;
    }
}
