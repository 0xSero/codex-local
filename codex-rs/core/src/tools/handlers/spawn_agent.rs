use crate::child_agent_bridge::ChildAgentBridge;
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
use std::sync::Arc;

pub struct SpawnAgentHandler;

#[derive(Debug, Deserialize)]
struct SpawnAgentArgs {
    task_id: String,
    purpose: String,
    prompt: String,
    #[serde(default)]
    _checklist: Vec<String>,
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
        let conversation_manager = Arc::clone(&session.services.conversation_manager);
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
                    model_reasoning_effort: Some(
                        codex_protocol::config_types::ReasoningEffort::Low,
                    ),
                },
                parent_config.codex_home.clone(),
            )
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!(
                    "Failed to build child agent config for profile: {e}"
                ))
            })?
        } else {
            // Fall back to inheriting the parent config with low reasoning effort for determinism
            let mut child_config = (*session.services.config).clone();
            // Override reasoning effort to low for more deterministic subagent responses
            child_config.model_reasoning_effort =
                Some(codex_protocol::config_types::ReasoningEffort::Low);
            child_config
        };

        // Spawn the child conversation
        let child = conversation_manager
            .new_conversation(child_config)
            .await
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!("Failed to spawn child agent: {e}"))
            })?;

        let child_conversation_id = child.conversation_id;
        let bridge = Arc::new(ChildAgentBridge::new(
            &session,
            args.task_id.clone(),
            sub_id.clone(),
            child_conversation_id,
        ));
        conversation_manager
            .register_child_agent_bridge(bridge.clone())
            .await
            .map_err(|e| {
                FunctionCallError::RespondToModel(format!(
                    "Failed to register child agent bridge: {e}"
                ))
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

        // Monitor the child conversation and emit UI events.
        // We also block the tool call until completion so the main agent
        // waits for the subagent's markdown result.
        let parent_session_for_ui = session.clone();
        let parent_sub_id_for_ui = sub_id.clone();
        let agent_id_for_ui = args.task_id.clone();
        let child_conversation_for_ui = child.conversation.clone();
        let bridge_for_monitor = bridge.clone();
        let conversation_manager_for_monitor = conversation_manager.clone();
        let child_conversation_id_for_monitor = child_conversation_id;

        // Channel used to signal final outcome back to the tool handler
        // so we can return a single markdown-formatted result.
        #[derive(Debug, Clone)]
        struct SubagentOutcome {
            success: bool,
            markdown: String,
            injected_into_turn: bool,
        }
        let (outcome_tx, outcome_rx) = tokio::sync::oneshot::channel::<SubagentOutcome>();

        tokio::spawn(async move {
            use std::time::Duration;
            use std::time::Instant;

            // Output truncation: limit subagent output to 5k tokens
            const OUTPUT_TOKEN_LIMIT: usize = 5000;
            let mut accumulated_output = String::new();
            let mut output_token_count = 0usize;
            let mut truncated = false;

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
                        // Accumulate output with truncation
                        if !truncated {
                            let delta_tokens = delta.delta.len().div_ceil(4); // Simple estimation
                            if output_token_count + delta_tokens <= OUTPUT_TOKEN_LIMIT {
                                accumulated_output.push_str(&delta.delta);
                                output_token_count += delta_tokens;
                            } else {
                                // Apply truncation
                                truncated = true;
                                let remaining_tokens =
                                    OUTPUT_TOKEN_LIMIT.saturating_sub(output_token_count);
                                let remaining_chars = remaining_tokens * 4;

                                if remaining_chars > 0 {
                                    let truncated_delta: String =
                                        delta.delta.chars().take(remaining_chars).collect();
                                    accumulated_output.push_str(&truncated_delta);
                                }
                                accumulated_output
                                    .push_str("\n\n[Output truncated at 5k token limit]");
                                output_token_count = OUTPUT_TOKEN_LIMIT;
                            }
                        }

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
                        // Accumulate full message with truncation
                        if !truncated {
                            let msg_tokens = msg.message.len().div_ceil(4);
                            if output_token_count + msg_tokens <= OUTPUT_TOKEN_LIMIT {
                                accumulated_output.push_str(&msg.message);
                                accumulated_output.push('\n');
                                output_token_count += msg_tokens;
                            } else {
                                truncated = true;
                                accumulated_output
                                    .push_str("\n\n[Output truncated at 5k token limit]");
                                output_token_count = OUTPUT_TOKEN_LIMIT;
                            }
                        }

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
                        if let Some(info) = tc.info {
                            // Emit context tracking information for orchestrator
                            if let Some(ctx) = info.model_context_window {
                                let pct = info
                                    .last_token_usage
                                    .percent_of_context_window_remaining(ctx);

                                // Emit progress with context info
                                emit_progress(format!("context left: {pct}%")).await;
                            }
                        }
                    }
                    codex_protocol::protocol::EventMsg::TaskComplete(TaskCompleteEvent {
                        last_agent_message,
                    }) => {
                        let fallback_message = if !accumulated_output.is_empty() {
                            accumulated_output.clone()
                        } else {
                            last_agent_message.or(last_message).unwrap_or_else(|| {
                                "Child agent completed without returning a message.".to_string()
                            })
                        };

                        let bridge_final_markdown = bridge_for_monitor.final_markdown().await;
                        let summary_body = bridge_final_markdown
                            .clone()
                            .unwrap_or_else(|| fallback_message.clone());
                        if bridge_final_markdown.is_none() {
                            bridge_for_monitor.set_final_markdown(summary_body.clone()).await;
                        }

                        let summary_heading = if truncated && bridge_final_markdown.is_none() {
                            format!(
                                "### Subagent `{agent_id_for_ui}` ✅ (output truncated to 5k tokens)"
                            )
                        } else {
                            format!("### Subagent `{agent_id_for_ui}` ✅")
                        };
                        let summary = format!("{summary_heading}\n\n{summary_body}");
                        let summary_for_event = summary.clone();

                        let _ = parent_session_for_ui
                            .send_event(Event {
                                id: parent_sub_id_for_ui.clone(),
                                msg: EventMsg::AgentCompleted(
                                    codex_protocol::protocol::AgentCompletedEvent {
                                        agent_id: agent_id_for_ui.clone(),
                                        success: true,
                                        summary: summary_for_event,
                                    },
                                ),
                            })
                            .await;

                        parent_session_for_ui
                            .unregister_child_agent(&agent_id_for_ui)
                            .await;

                        let injected = parent_session_for_ui
                            .inject_input(vec![InputItem::Text {
                                text: summary.clone(),
                            }])
                            .await
                            .is_ok();

                        if !injected {
                            let _ = parent_session_for_ui
                                .send_event(Event {
                                    id: parent_sub_id_for_ui.clone(),
                                    msg: EventMsg::BackgroundEvent(
                                        codex_protocol::protocol::BackgroundEventEvent {
                                            message: format!(
                                                "Subagent {agent_id_for_ui} completed but results could not be injected. Results: {summary_body}"
                                            ),
                                        },
                                    ),
                                })
                                .await;
                        }

                        let _ = outcome_tx.send(SubagentOutcome {
                            success: true,
                            markdown: summary,
                            injected_into_turn: injected,
                        });

                        let _ = conversation_manager_for_monitor
                            .remove_child_agent_bridge(&child_conversation_id_for_monitor)
                            .await;

                        break;
                    }
                    codex_protocol::protocol::EventMsg::Error(err) => {
                        let error_message =
                            format!("### Subagent `{agent_id_for_ui}` ❌\n\n{}", err.message);

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

                        let injected = parent_session_for_ui
                            .inject_input(vec![InputItem::Text {
                                text: error_message.clone(),
                            }])
                            .await
                            .is_ok();

                        if !injected {
                            let _ = parent_session_for_ui
                                .send_event(Event {
                                    id: parent_sub_id_for_ui.clone(),
                                    msg: EventMsg::BackgroundEvent(
                                        codex_protocol::protocol::BackgroundEventEvent {
                                            message: format!(
                                                "Subagent {} failed: {}",
                                                agent_id_for_ui, err.message
                                            ),
                                        },
                                    ),
                                })
                                .await;
                        }

                        let _ = outcome_tx.send(SubagentOutcome {
                            success: false,
                            markdown: error_message,
                            injected_into_turn: injected,
                        });

                        let _ = conversation_manager_for_monitor
                            .remove_child_agent_bridge(&child_conversation_id_for_monitor)
                            .await;

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

                        let abort_message = format!(
                            "### Subagent `{agent_id_for_ui}` ⚠️\n\nThe subagent was {reason_text}."
                        );

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

                        let injected = parent_session_for_ui
                            .inject_input(vec![InputItem::Text {
                                text: abort_message.clone(),
                            }])
                            .await
                            .is_ok();

                        if !injected {
                            let _ = parent_session_for_ui
                                .send_event(Event {
                                    id: parent_sub_id_for_ui.clone(),
                                    msg: EventMsg::BackgroundEvent(
                                        codex_protocol::protocol::BackgroundEventEvent {
                                            message: format!(
                                                "Subagent {agent_id_for_ui} aborted: {reason_text}"
                                            ),
                                        },
                                    ),
                                })
                                .await;
                        }

                        let _ = outcome_tx.send(SubagentOutcome {
                            success: false,
                            markdown: abort_message,
                            injected_into_turn: injected,
                        });

                        let _ = conversation_manager_for_monitor
                            .remove_child_agent_bridge(&child_conversation_id_for_monitor)
                            .await;

                        break;
                    }
                    _ => {}
                }
            }
            let _ = conversation_manager_for_monitor
                .remove_child_agent_bridge(&child_conversation_id_for_monitor)
                .await;

        });

        // Block until we receive the subagent outcome, then return its
        // markdown to the model so the main agent can use it directly.
        match outcome_rx.await {
            Ok(outcome) => {
                let payload = serde_json::json!({
                    "agent_id": args.task_id,
                    "status": if outcome.success { "completed" } else { "failed" },
                    "markdown_summary": outcome.markdown,
                    "injected_into_turn": outcome.injected_into_turn,
                });
                Ok(ToolOutput::Function {
                    content: payload.to_string(),
                    success: Some(outcome.success),
                })
            }
            Err(_recv_err) => Ok(ToolOutput::Function {
                content: format!(
                    "## Subagent `{}` did not report a result\n\nThe child agent terminated without a final outcome.",
                    args.task_id
                ),
                success: Some(false),
            }),
        }
    }
}
