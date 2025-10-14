use crate::config::Config as CoreConfig;
use crate::config::ConfigOverrides;
use crate::config::ConfigToml;
use crate::config::load_config_as_toml_with_cli_overrides;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::registry::ToolHandler;
use crate::tools::registry::ToolKind;
use async_trait::async_trait;
use codex_protocol::protocol::AgentEvent;
use codex_protocol::protocol::AgentSpawnedEvent;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InputItem;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::TaskCompleteEvent;
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
                .map_err(|e| {
                    FunctionCallError::RespondToModel(format!(
                        "Failed to load config.toml for child agent: {e}"
                    ))
                })?;
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

        session
            .register_child_agent(args.task_id.clone(), child.conversation.clone())
            .await;

        // Create a channel to communicate completion from the monitoring task to the main handler.
        // This avoids having two consumers of the same event stream (which causes race conditions).
        let (completion_tx, completion_rx) = tokio::sync::oneshot::channel();

        // Monitor the child conversation and emit UI events in the background.
        // When complete, send the final result through the completion channel.
        let parent_session_for_ui = session.clone();
        let parent_sub_id_for_ui = sub_id.clone();
        let agent_id_for_ui = args.task_id.clone();
        let child_conversation_for_ui = child.conversation.clone();
        tokio::spawn(async move {
            use std::time::Duration;
            use std::time::Instant;
            let mut progress_buffer = String::new();
            let mut last_progress_emit = Instant::now();
            let progress_interval = Duration::from_millis(900);
            let mut last_message: Option<String> = None;

            let emit_progress = |text: String| async {
                let _ = parent_session_for_ui
                    .send_event(Event {
                        id: parent_sub_id_for_ui.clone(),
                        msg: EventMsg::AgentProgress(
                            codex_protocol::protocol::AgentProgressEvent {
                                agent_id: agent_id_for_ui.clone(),
                                message: text,
                            },
                        ),
                    })
                    .await;
            };

            loop {
                let event = match child_conversation_for_ui.next_event().await {
                    Ok(event) => event,
                    Err(_) => break,
                };

                let forwarded = EventMsg::AgentEvent(AgentEvent {
                    agent_id: agent_id_for_ui.clone(),
                    event: Box::new(event.clone()),
                });
                let _ = parent_session_for_ui
                    .send_event(Event {
                        id: parent_sub_id_for_ui.clone(),
                        msg: forwarded,
                    })
                    .await;

                match event.msg {
                    codex_protocol::protocol::EventMsg::TaskStarted(_) => {
                        emit_progress("started".to_string()).await;
                    }
                    codex_protocol::protocol::EventMsg::AgentMessageDelta(delta) => {
                        progress_buffer.push_str(&delta.delta);
                        let should_flush = progress_buffer.contains('\n')
                            || progress_buffer.len() > 500
                            || last_progress_emit.elapsed() >= progress_interval;
                        if should_flush {
                            let snippet = if progress_buffer.len() > 400 {
                                progress_buffer
                                    .chars()
                                    .rev()
                                    .take(400)
                                    .collect::<String>()
                                    .chars()
                                    .rev()
                                    .collect::<String>()
                            } else {
                                progress_buffer.clone()
                            };
                            emit_progress(snippet).await;
                            progress_buffer.clear();
                            last_progress_emit = Instant::now();
                        }
                    }
                    codex_protocol::protocol::EventMsg::AgentMessage(msg) => {
                        last_message = Some(msg.message.clone());
                        emit_progress(msg.message).await;
                    }
                    codex_protocol::protocol::EventMsg::ExecCommandBegin(begin) => {
                        emit_progress(format!(
                            "exec: {:?} in {}",
                            begin.command,
                            begin.cwd.display()
                        ))
                        .await;
                    }
                    codex_protocol::protocol::EventMsg::ExecCommandEnd(end) => {
                        emit_progress(format!("exec: exit {}", end.exit_code)).await;
                    }
                    codex_protocol::protocol::EventMsg::McpToolCallBegin(begin) => {
                        let inv = begin.invocation;
                        emit_progress(format!("tool: {}.{}", inv.server, inv.tool)).await;
                    }
                    codex_protocol::protocol::EventMsg::McpToolCallEnd(end) => {
                        let inv = &end.invocation;
                        emit_progress(format!(
                            "tool: {}.{} {}",
                            inv.server,
                            inv.tool,
                            if end.is_success() { "ok" } else { "failed" }
                        ))
                        .await;
                    }
                    codex_protocol::protocol::EventMsg::TokenCount(tc) => {
                        if let Some(info) = tc.info
                            && let Some(ctx) = info.model_context_window
                        {
                            let pct = info
                                .last_token_usage
                                .percent_of_context_window_remaining(ctx);
                            emit_progress(format!("context left: {pct}%")).await;
                        }
                    }
                    codex_protocol::protocol::EventMsg::TaskComplete(TaskCompleteEvent {
                        last_agent_message,
                    }) => {
                        let final_message =
                            last_agent_message.or(last_message).unwrap_or_else(|| {
                                "Child agent completed without returning a message.".to_string()
                            });

                        let summary = format!(
                            "Child agent '{agent_id_for_ui}' completed successfully.\n\n\
                             --- Results from child agent ---\n{final_message}\n--- End results ---"
                        );

                        let _ = parent_session_for_ui
                            .send_event(Event {
                                id: parent_sub_id_for_ui.clone(),
                                msg: EventMsg::AgentCompleted(
                                    codex_protocol::protocol::AgentCompletedEvent {
                                        agent_id: agent_id_for_ui.clone(),
                                        success: true,
                                        summary,
                                    },
                                ),
                            })
                            .await;

                        parent_session_for_ui
                            .unregister_child_agent(&agent_id_for_ui)
                            .await;

                        // Send completion result through channel
                        let _ = completion_tx.send(Ok(final_message));
                        break;
                    }
                    codex_protocol::protocol::EventMsg::Error(err) => {
                        let error_message = format!(
                            "Child agent '{}' encountered an error:\n{}",
                            agent_id_for_ui, err.message
                        );

                        let _ = parent_session_for_ui
                            .send_event(Event {
                                id: parent_sub_id_for_ui.clone(),
                                msg: EventMsg::AgentCompleted(
                                    codex_protocol::protocol::AgentCompletedEvent {
                                        agent_id: agent_id_for_ui.clone(),
                                        success: false,
                                        summary: error_message.clone(),
                                    },
                                ),
                            })
                            .await;

                        parent_session_for_ui
                            .unregister_child_agent(&agent_id_for_ui)
                            .await;

                        // Send error through channel
                        let _ = completion_tx.send(Err(error_message));
                        break;
                    }
                    codex_protocol::protocol::EventMsg::TurnAborted(aborted) => {
                        let reason_text = match aborted.reason {
                            codex_protocol::protocol::TurnAbortReason::Interrupted => {
                                "interrupted by user"
                            }
                            codex_protocol::protocol::TurnAbortReason::Replaced => {
                                "replaced by another task"
                            }
                            codex_protocol::protocol::TurnAbortReason::ReviewEnded => {
                                "review ended"
                            }
                        };

                        let abort_message =
                            format!("Child agent '{agent_id_for_ui}' was {reason_text}");

                        let _ = parent_session_for_ui
                            .send_event(Event {
                                id: parent_sub_id_for_ui.clone(),
                                msg: EventMsg::AgentCompleted(
                                    codex_protocol::protocol::AgentCompletedEvent {
                                        agent_id: agent_id_for_ui.clone(),
                                        success: false,
                                        summary: abort_message.clone(),
                                    },
                                ),
                            })
                            .await;

                        parent_session_for_ui
                            .unregister_child_agent(&agent_id_for_ui)
                            .await;

                        // Send abort through channel
                        let _ = completion_tx.send(Err(abort_message));
                        break;
                    }
                    _ => {}
                }
            }
        });

        // WAIT for child agent to complete via the completion channel.
        // The background task monitors events and sends the final result.
        match completion_rx.await {
            Ok(Ok(final_message)) => {
                // Child completed successfully - return the final message as tool result.
                // This gets added to the parent conversation context.
                Ok(ToolOutput::Function {
                    content: final_message,
                    success: Some(true),
                })
            }
            Ok(Err(error_message)) => {
                // Child failed - return error
                Err(FunctionCallError::RespondToModel(error_message))
            }
            Err(_) => {
                // Channel closed unexpectedly (background task panicked?)
                session.unregister_child_agent(&args.task_id).await;
                Err(FunctionCallError::RespondToModel(
                    "Child agent monitoring task failed unexpectedly".to_string(),
                ))
            }
        }
    }
}
