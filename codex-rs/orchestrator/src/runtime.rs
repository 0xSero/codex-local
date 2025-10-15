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
use std::time::Duration;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio::task::JoinSet;

/// Context usage tracking for a subagent.
#[derive(Clone, Debug)]
pub struct ContextUsage {
    pub tokens_used: u64,
    pub context_window: Option<u64>,
    pub last_update: Instant,
}

impl ContextUsage {
    pub fn new() -> Self {
        Self {
            tokens_used: 0,
            context_window: None,
            last_update: Instant::now(),
        }
    }

    pub fn usage_percentage(&self) -> Option<u8> {
        self.context_window.map(|window| {
            if window == 0 {
                return 100;
            }
            ((self.tokens_used * 100) / window).min(100) as u8
        })
    }

    pub fn context_display(&self) -> String {
        match self.usage_percentage() {
            Some(pct) => format!(
                "subagent context = {}/{} ({}%)",
                self.tokens_used,
                self.context_window.unwrap_or(0),
                pct
            ),
            None => format!("subagent context = {}", self.tokens_used),
        }
    }
}

impl Default for ContextUsage {
    fn default() -> Self {
        Self::new()
    }
}

/// Status of a subagent.
#[derive(Clone, Debug, PartialEq)]
pub enum SubagentStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
    Timeout,
}

/// Complete context for a subagent, tracking execution and results.
#[derive(Clone, Debug)]
pub struct SubagentContext {
    pub agent_id: String,
    pub conversation_id: ConversationId,
    pub task: TaskSpec,
    pub status: SubagentStatus,
    pub context_usage: ContextUsage,
    pub output_buffer: String,
    pub start_time: Instant,
    pub completion_time: Option<Instant>,
}

impl SubagentContext {
    pub fn new(agent_id: String, conversation_id: ConversationId, task: TaskSpec) -> Self {
        Self {
            agent_id,
            conversation_id,
            task,
            status: SubagentStatus::Pending,
            context_usage: ContextUsage::new(),
            output_buffer: String::new(),
            start_time: Instant::now(),
            completion_time: None,
        }
    }

    pub fn duration(&self) -> Duration {
        self.completion_time
            .map(|end| end.duration_since(self.start_time))
            .unwrap_or_else(|| self.start_time.elapsed())
    }
}

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

    /// Subagent contexts for tracking execution
    subagent_contexts: Arc<RwLock<HashMap<String, SubagentContext>>>,

    /// Event emitter
    event_emitter: EventEmitter,

    /// Task queue
    task_queue: Arc<RwLock<Vec<TaskSpec>>>,

    /// Result aggregator
    result_aggregator: Arc<RwLock<ResultAggregator>>,

    /// Active task join set for parallel execution
    _active_tasks: Arc<RwLock<JoinSet<Result<AgentOutput, String>>>>,
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

/// Output from a completed subagent.
#[derive(Clone, Debug)]
pub struct AgentOutput {
    pub agent_id: String,
    pub task_spec: TaskSpec,
    pub truncated_output: String,
    pub completion_time: Duration,
    pub context_usage: ContextUsage,
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
            subagent_contexts: Arc::new(RwLock::new(HashMap::new())),
            event_emitter,
            task_queue: Arc::new(RwLock::new(Vec::new())),
            result_aggregator: Arc::new(RwLock::new(ResultAggregator::new())),
            _active_tasks: Arc::new(RwLock::new(JoinSet::new())),
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

    /// Updates context usage for a subagent.
    pub async fn update_context_usage(
        &self,
        agent_id: &str,
        tokens_used: u64,
        context_window: Option<u64>,
    ) {
        let mut contexts = self.subagent_contexts.write().await;
        if let Some(context) = contexts.get_mut(agent_id) {
            context.context_usage.tokens_used = tokens_used;
            context.context_usage.context_window = context_window;
            context.context_usage.last_update = Instant::now();
        }
    }

    /// Gets a clone of a subagent context.
    pub async fn get_subagent_context(&self, agent_id: &str) -> Option<SubagentContext> {
        let contexts = self.subagent_contexts.read().await;
        contexts.get(agent_id).cloned()
    }

    /// Lists all active subagent contexts.
    pub async fn list_subagent_contexts(&self) -> Vec<SubagentContext> {
        let contexts = self.subagent_contexts.read().await;
        contexts.values().cloned().collect()
    }

    /// Spawns multiple child agents in parallel and waits for all to complete.
    /// Returns a vector of agent outputs from all completed agents.
    pub async fn spawn_parallel_agents(
        &self,
        tasks: Vec<TaskSpec>,
        configs: Vec<Config>,
    ) -> anyhow::Result<Vec<AgentOutput>> {
        if tasks.len() != configs.len() {
            anyhow::bail!(
                "Task count ({}) does not match config count ({})",
                tasks.len(),
                configs.len()
            );
        }

        // Spawn all agents
        for (task, config) in tasks.into_iter().zip(configs.into_iter()) {
            let agent_id = self.spawn_child(task, config).await?;

            // Initialize context tracking
            let children = self.children.read().await;
            if let Some(child) = children.get(&agent_id) {
                let context = SubagentContext::new(
                    child.agent_id.clone(),
                    child.conversation_id,
                    child.task.clone(),
                );
                let mut contexts = self.subagent_contexts.write().await;
                contexts.insert(agent_id.clone(), context);
            }
        }

        // Wait for all to complete
        self.wait_for_all_children().await
    }

    /// Waits for all active child agents to complete and returns their outputs.
    pub async fn wait_for_all_children(&self) -> anyhow::Result<Vec<AgentOutput>> {
        let mut outputs = Vec::new();

        // Poll children until all complete
        loop {
            let count = self.active_child_count().await;
            if count == 0 {
                break;
            }

            // Small delay to avoid busy waiting
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Collect outputs from completed contexts
        let contexts = self.subagent_contexts.read().await;
        for context in contexts.values() {
            if context.status == SubagentStatus::Completed {
                outputs.push(AgentOutput {
                    agent_id: context.agent_id.clone(),
                    task_spec: context.task.clone(),
                    truncated_output: context.output_buffer.clone(),
                    completion_time: context.duration(),
                    context_usage: context.context_usage.clone(),
                });
            }
        }

        Ok(outputs)
    }

    /// Marks a subagent output buffer with content.
    pub async fn append_subagent_output(&self, agent_id: &str, content: &str) {
        let mut contexts = self.subagent_contexts.write().await;
        if let Some(context) = contexts.get_mut(agent_id) {
            context.output_buffer.push_str(content);
        }
    }

    /// Marks a subagent as completed with final status.
    pub async fn mark_subagent_completed(&self, agent_id: &str, success: bool) {
        let mut contexts = self.subagent_contexts.write().await;
        if let Some(context) = contexts.get_mut(agent_id) {
            context.status = if success {
                SubagentStatus::Completed
            } else {
                SubagentStatus::Failed("Agent failed".to_string())
            };
            context.completion_time = Some(Instant::now());
        }
    }
}
