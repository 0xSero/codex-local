//! Core orchestrator runtime.

use crate::events::EventEmitter;
use crate::events::TaggedEvent;
use crate::profiles::ProfileSelector;
use crate::spawner::AgentSpawner;
use crate::spawner::SpawnedAgent;
use crate::spec::TaskSpec;
use crate::validation::validate_checklist;
use crate::validation::ResultAggregator;
use codex_core::config::Config;
use codex_core::CodexConversation;
use codex_core::ConversationManager;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::InputItem;
use codex_protocol::protocol::Op;
use codex_protocol::ConversationId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// The main orchestrator that coordinates child agents.
pub struct Orchestrator {
    /// Parent conversation (the "main" thread)
    parent_conversation_id: ConversationId,
    #[allow(dead_code)]
    parent_conversation: Arc<CodexConversation>,

    /// Agent spawner
    spawner: AgentSpawner,

    /// Profile selector
    profile_selector: ProfileSelector,

    /// Active child agents
    children: Arc<RwLock<HashMap<String, ChildAgent>>>,

    /// Event emitter
    event_emitter: EventEmitter,

    /// Task queue
    task_queue: Arc<RwLock<Vec<TaskSpec>>>,

    /// Result aggregator
    result_aggregator: Arc<RwLock<ResultAggregator>>,
}

/// Represents an active child agent.
#[derive(Clone)]
#[allow(dead_code)]
struct ChildAgent {
    agent_id: String,
    conversation_id: ConversationId,
    conversation: Arc<CodexConversation>,
    task: TaskSpec,
}

impl Orchestrator {
    /// Creates a new orchestrator instance.
    pub fn new(
        parent_conversation_id: ConversationId,
        parent_conversation: Arc<CodexConversation>,
        conversation_manager: Arc<ConversationManager>,
        config: &Config,
        parent_submit_id: String,
    ) -> Self {
        let parent_agent_id = "orchestrator-main".to_string();
        let spawner = AgentSpawner::new(conversation_manager, parent_agent_id);
        let profile_selector = ProfileSelector::new(config);
        let event_emitter = EventEmitter::new(parent_submit_id);

        Self {
            parent_conversation_id,
            parent_conversation,
            spawner,
            profile_selector,
            children: Arc::new(RwLock::new(HashMap::new())),
            event_emitter,
            task_queue: Arc::new(RwLock::new(Vec::new())),
            result_aggregator: Arc::new(RwLock::new(ResultAggregator::new())),
        }
    }

    /// Enqueues a task to be executed by a child agent.
    pub async fn enqueue_task(&self, mut task: TaskSpec) {
        // Select a profile if not specified
        if task.profile.is_none() {
            task.profile = self.profile_selector.select_profile(None);
        }

        let mut queue = self.task_queue.write().await;
        queue.push(task);
    }

    /// Spawns a child agent for a task.
    pub async fn spawn_child(&self, task: TaskSpec, config: Config) -> anyhow::Result<String> {
        let agent_id = task.id.clone();

        // Emit spawn event
        let _spawn_event = self.event_emitter.agent_spawned(
            agent_id.clone(),
            Some(self.spawner.parent_agent_id().to_string()),
            task.profile.clone(),
            task.purpose.clone(),
        );

        // Spawn the agent
        let SpawnedAgent {
            agent_id: spawned_id,
            conversation_id,
            conversation,
        } = self
            .spawner
            .spawn_child(&task, config, &self.event_emitter)
            .await?;

        // Store the child agent
        let child = ChildAgent {
            agent_id: spawned_id.clone(),
            conversation_id,
            conversation: conversation.clone(),
            task: task.clone(),
        };

        {
            let mut children = self.children.write().await;
            children.insert(spawned_id.clone(), child);
        }

        // Submit the task prompt to the child agent
        let _submit_id = conversation
            .submit(Op::UserInput {
                items: vec![InputItem::Text { text: task.prompt }],
            })
            .await?;

        Ok(agent_id)
    }

    /// Processes events from a child agent, tagging them with agent context.
    pub async fn process_child_event(&self, agent_id: &str, event: Event) -> Option<TaggedEvent> {
        Some(TaggedEvent::tag(agent_id.to_string(), event))
    }

    /// Marks a child agent as completed and validates its results.
    pub async fn complete_child(&self, agent_id: &str) -> anyhow::Result<()> {
        let child_opt = {
            let children = self.children.read().await;
            children.get(agent_id).cloned()
        };

        if let Some(child) = child_opt {
            // Validate the checklist
            let validation = validate_checklist(&child.task);

            // Add to results
            {
                let mut aggregator = self.result_aggregator.write().await;
                aggregator.add_result(agent_id.to_string(), validation.clone());
            }

            // Emit completion event
            let summary = format!(
                "Completed {}/{} checklist items",
                validation.completed_count, validation.total_count
            );

            let _complete_event = self.event_emitter.agent_completed(
                agent_id.to_string(),
                validation.all_completed,
                summary,
            );

            // Remove from active children
            {
                let mut children = self.children.write().await;
                children.remove(agent_id);
            }
        }

        Ok(())
    }

    /// Returns the current number of active children.
    pub async fn active_child_count(&self) -> usize {
        let children = self.children.read().await;
        children.len()
    }

    /// Returns the parent conversation ID.
    pub fn parent_conversation_id(&self) -> ConversationId {
        self.parent_conversation_id
    }

    /// Returns the aggregated results from all completed children.
    pub async fn get_summary(&self) -> crate::validation::AggregateSummary {
        let aggregator = self.result_aggregator.read().await;
        aggregator.summary()
    }
}
