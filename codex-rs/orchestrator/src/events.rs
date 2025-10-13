//! Event handling and fan-out for orchestrator.

use codex_protocol::protocol::AgentCompletedEvent;
use codex_protocol::protocol::AgentProgressEvent;
use codex_protocol::protocol::AgentSpawnedEvent;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;

/// Wraps an event with agent context.
pub struct TaggedEvent {
    pub agent_id: String,
    pub event: Event,
}

impl TaggedEvent {
    pub fn new(agent_id: String, event: Event) -> Self {
        Self { agent_id, event }
    }

    /// Creates a tagged event from an existing event, adding agent context.
    pub fn tag(agent_id: String, event: Event) -> Self {
        Self::new(agent_id, event)
    }
}

/// Helper to emit orchestrator-specific events.
pub struct EventEmitter {
    parent_submit_id: String,
}

impl EventEmitter {
    pub fn new(parent_submit_id: String) -> Self {
        Self { parent_submit_id }
    }

    pub fn agent_spawned(
        &self,
        agent_id: String,
        parent_id: Option<String>,
        profile: Option<String>,
        purpose: String,
    ) -> Event {
        Event {
            id: self.parent_submit_id.clone(),
            msg: EventMsg::AgentSpawned(AgentSpawnedEvent {
                agent_id,
                parent_id,
                profile,
                purpose,
            }),
        }
    }

    pub fn agent_progress(&self, agent_id: String, message: String) -> Event {
        Event {
            id: self.parent_submit_id.clone(),
            msg: EventMsg::AgentProgress(AgentProgressEvent { agent_id, message }),
        }
    }

    pub fn agent_completed(&self, agent_id: String, success: bool, summary: String) -> Event {
        Event {
            id: self.parent_submit_id.clone(),
            msg: EventMsg::AgentCompleted(AgentCompletedEvent {
                agent_id,
                success,
                summary,
            }),
        }
    }
}
