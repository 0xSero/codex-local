use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use async_trait::async_trait;
use codex_protocol::protocol::AgentProgressEvent;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use serde::Deserialize;

pub struct ReturnProgressHandler;

#[derive(Debug, Deserialize)]
struct ReturnProgressArgs {
    #[serde(default)]
    task_id: Option<String>,
    progress: String,
    #[serde(default)]
    is_final: bool,
}

#[async_trait]
impl ToolHandler for ReturnProgressHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            session,
            payload,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "return_progress requires function arguments".to_string(),
                ));
            }
        };

        let args: ReturnProgressArgs = serde_json::from_str(&arguments).map_err(|e| {
            FunctionCallError::RespondToModel(format!(
                "Failed to parse return_progress arguments: {e}"
            ))
        })?;

        let conversation_id = session.conversation_id();
        let bridge = session
            .services
            .conversation_manager
            .get_child_agent_bridge(&conversation_id)
            .await
            .ok_or_else(|| {
                FunctionCallError::RespondToModel(
                    "return_progress can only be used by active subagents".to_string(),
                )
            })?;

        if let Some(task_id) = args.task_id.as_ref() {
            if task_id != &bridge.agent_id {
                return Err(FunctionCallError::RespondToModel(format!(
                    "return_progress task_id `{task_id}` does not match active agent `{}`",
                    bridge.agent_id
                )));
            }
        }

        let parent_session = bridge.parent_session().ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "The parent agent is no longer active for this subagent".to_string(),
            )
        })?;

        let message = args.progress;
        bridge.set_last_progress(message.clone()).await;
        if args.is_final {
            bridge.set_final_markdown(message.clone()).await;
        }

        let progress_event = Event {
            id: bridge.parent_sub_id.clone(),
            msg: EventMsg::AgentProgress(AgentProgressEvent {
                agent_id: bridge.agent_id.clone(),
                message: message.clone(),
            }),
        };
        let _ = parent_session.send_event(progress_event).await;

        let response = serde_json::json!({
            "status": "ok",
            "is_final": args.is_final,
        });

        Ok(ToolOutput::Function {
            content: response.to_string(),
            success: Some(true),
        })
    }
}
