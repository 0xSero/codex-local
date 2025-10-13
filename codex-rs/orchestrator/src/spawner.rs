//! Child agent spawning using ConversationManager.

use crate::events::EventEmitter;
use crate::spec::TaskSpec;
use codex_core::config::Config;
use codex_core::CodexConversation;
use codex_core::ConversationManager;
use codex_core::NewConversation;
use codex_protocol::ConversationId;
use std::sync::Arc;

/// Manages spawning of child agents.
pub struct AgentSpawner {
    conversation_manager: Arc<ConversationManager>,
    parent_agent_id: String,
}

/// Information about a spawned child agent.
pub struct SpawnedAgent {
    pub agent_id: String,
    pub conversation_id: ConversationId,
    pub conversation: Arc<CodexConversation>,
}

impl AgentSpawner {
    pub fn new(conversation_manager: Arc<ConversationManager>, parent_agent_id: String) -> Self {
        Self {
            conversation_manager,
            parent_agent_id,
        }
    }

    /// Spawns a child agent for the given task.
    pub async fn spawn_child(
        &self,
        task: &TaskSpec,
        config: Config,
        _event_emitter: &EventEmitter,
    ) -> anyhow::Result<SpawnedAgent> {
        // Spawn a new conversation with the provided config
        let NewConversation {
            conversation_id,
            conversation,
            session_configured: _,
        } = self.conversation_manager.new_conversation(config).await?;

        let agent_id = task.id.clone();

        // Emit agent spawned event
        tracing::info!(
            agent_id = %agent_id,
            conversation_id = %conversation_id,
            "Spawned child agent"
        );

        Ok(SpawnedAgent {
            agent_id,
            conversation_id,
            conversation,
        })
    }

    pub fn parent_agent_id(&self) -> &str {
        &self.parent_agent_id
    }
}
