use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use async_trait::async_trait;
use crate::config::Config as CoreConfig;
use crate::config::ConfigOverrides;
use crate::config::ConfigToml;
use crate::config::load_config_as_toml_with_cli_overrides;
use codex_protocol::protocol::AgentSpawnedEvent;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InputItem;
use codex_protocol::protocol::Op;
use serde::Deserialize;

pub struct SpawnAgentHandler;

#[derive(Debug, Deserialize)]
struct SpawnAgentArgs {
    task_id: String,
    purpose: String,
    prompt: String,
    #[serde(default)]
    checklist: Vec<String>,
    #[serde(default)]
    profile: Option<String>,
}

#[async_trait]
impl ToolHandler for SpawnAgentHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let ToolInvocation {
            session,
            
            payload,
            sub_id,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "spawn_agent requires function arguments".to_string(),
                ));
            }
        };

        let args: SpawnAgentArgs = serde_json::from_str(&arguments).map_err(|e| {
            FunctionCallError::RespondToModel(format!("Failed to parse spawn_agent arguments: {e}"))
        })?;

        // Get the conversation manager and parent config from session services
        let conversation_manager = &session.services.conversation_manager;
        let parent_config = (*session.services.config).clone();

        // Determine the profile to use for the child agent
        let selected_profile: Option<String> = match args.profile {
            Some(ref p) => Some(p.clone()),
            None => parent_config.active_agent_profiles.first().cloned(),
        };

        // Load a fresh Config for the child with the selected profile (if any).
        // This ensures model, provider, approvals, etc. reflect the intended role.
        let child_config: CoreConfig = if let Some(profile_name) = selected_profile.clone() {
            let codex_home = parent_config.codex_home.clone();
            let cfg_toml: ConfigToml = load_config_as_toml_with_cli_overrides(&codex_home, vec![])
                .await
                .map_err(|e| FunctionCallError::RespondToModel(format!(
                    "Failed to load config.toml for child agent: {e}"
                )))?;
            CoreConfig::load_from_base_config_with_overrides(
                cfg_toml,
                ConfigOverrides {
                    model: None,
                    review_model: None,
                    cwd: Some(parent_config.cwd.clone()),
                    approval_policy: None,
                    sandbox_mode: None,
                    model_provider: None,
                    config_profile: Some(profile_name),
                    codex_linux_sandbox_exe: parent_config.codex_linux_sandbox_exe.clone(),
                    base_instructions: None,
                    include_plan_tool: None,
                    include_apply_patch_tool: None,
                    include_view_image_tool: None,
                    show_raw_agent_reasoning: None,
                    tools_web_search_request: None,
                },
                parent_config.codex_home.clone(),
            )
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!(
                    "Failed to build child agent config for profile: {e}"
                ))
            })?
        } else {
            // Fall back to inheriting the parent config
            (*session.services.config).clone()
        };

        // Spawn the child conversation
        let child = conversation_manager
            .new_conversation(child_config)
            .await
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!("Failed to spawn child agent: {e}"))
            })?;

        // Emit an AgentSpawned event so UIs can reflect multi-agent activity
        let _ = session
            .send_event(Event {
                id: sub_id.clone(),
                msg: EventMsg::AgentSpawned(AgentSpawnedEvent {
                    agent_id: args.task_id.clone(),
                    parent_id: Some("orchestrator-main".to_string()),
                    profile: selected_profile.clone(),
                    purpose: args.purpose.clone(),
                }),
            })
            .await;

        // Submit the task prompt to the child agent
        child
            .conversation
            .submit(Op::UserInput {
                items: vec![InputItem::Text { text: args.prompt }],
            })
            .await
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!(
                    "Failed to submit prompt to child agent: {e}"
                ))
            })?;

        let checklist_str = if args.checklist.is_empty() {
            String::new()
        } else {
            format!(
                "\nChecklist:\n{}",
                args.checklist
                    .iter()
                    .map(|item| format!("- {item}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };

        let profile_str = selected_profile
            .as_ref()
            .map(|p| format!(" (profile: {p})"))
            .unwrap_or_default();

        let response = format!(
            "Child agent spawned successfully!\n\
             Task ID: {}\n\
             Purpose: {}\n\
             Conversation ID: {}{}\n\
             \n\
             The child agent is now working on this task in a separate conversation context.",
            args.task_id, args.purpose, child.conversation_id, profile_str
        );

        Ok(ToolOutput::Function {
            content: response,
            success: Some(true),
        })
    }
}
