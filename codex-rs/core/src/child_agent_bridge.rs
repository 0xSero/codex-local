use crate::codex::Session;
use codex_protocol::ConversationId;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Default, Clone)]
pub(crate) struct ChildAgentBridgeState {
    pub(crate) last_progress: Option<String>,
    pub(crate) final_markdown: Option<String>,
}

pub struct ChildAgentBridge {
    parent_session: std::sync::Weak<Session>,
    pub(crate) agent_id: String,
    pub(crate) parent_sub_id: String,
    pub(crate) conversation_id: ConversationId,
    state: Mutex<ChildAgentBridgeState>,
}

impl ChildAgentBridge {
    pub(crate) fn new(
        parent_session: &Arc<Session>,
        agent_id: String,
        parent_sub_id: String,
        conversation_id: ConversationId,
    ) -> Self {
        Self {
            parent_session: Arc::downgrade(parent_session),
            agent_id,
            parent_sub_id,
            conversation_id,
            state: Mutex::new(ChildAgentBridgeState::default()),
        }
    }

    pub(crate) fn parent_session(&self) -> Option<Arc<Session>> {
        self.parent_session.upgrade()
    }

    pub(crate) async fn set_last_progress(&self, message: String) {
        let mut state = self.state.lock().await;
        state.last_progress = Some(message);
    }

    pub(crate) async fn set_final_markdown(&self, markdown: String) {
        let mut state = self.state.lock().await;
        state.final_markdown = Some(markdown);
    }

    pub(crate) async fn last_progress(&self) -> Option<String> {
        let state = self.state.lock().await;
        state.last_progress.clone()
    }

    pub(crate) async fn final_markdown(&self) -> Option<String> {
        let state = self.state.lock().await;
        state.final_markdown.clone()
    }
}
