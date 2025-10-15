#![cfg(not(target_os = "windows"))]
#![allow(clippy::unwrap_used)]

use std::time::Duration;
use std::time::Instant;

use codex_core::model_family::find_family_for_model;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::EventMsg;
use codex_core::protocol::InputItem;
use codex_core::protocol::Op;
use codex_core::protocol::SandboxPolicy;
use codex_protocol::config_types::ReasoningSummary;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use serde_json::json;

async fn run_turn_and_measure(
    test: &core_test_support::test_codex::TestCodex,
    prompt: &str,
) -> anyhow::Result<Duration> {
    let session_model = test.session_configured.model.clone();

    let start = Instant::now();
    test.codex
        .submit(Op::UserTurn {
            items: vec![InputItem::Text {
                text: prompt.into(),
            }],
            final_output_json_schema: None,
            cwd: test.cwd.path().to_path_buf(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::DangerFullAccess,
            model: session_model,
            effort: None,
            summary: ReasoningSummary::Auto,
        })
        .await?;

    wait_for_event(&test.codex, |ev| matches!(ev, EventMsg::TaskComplete(_))).await;
    Ok(start.elapsed())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spawn_agent_runs_children_in_parallel_and_blocks_parent() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;

    // Build a Codex configured for orchestrator + child agent profiles so the spawn_agent tool is present.
    let mut builder = test_codex().with_config(|config| {
        config.model = "test-gpt-5-codex".to_string();
        config.model_family =
            find_family_for_model("test-gpt-5-codex").expect("test-gpt-5-codex model family");
        config.active_orchestrator_profile = Some("orchestrator".to_string());
        config.active_agent_profiles = vec!["agent-a".to_string()];
    });
    let test = builder.build(&server).await?;

    // Parent turn: request two spawn_agent tool calls.
    let spawn_args_1 = json!({
        "task_id": "child-1",
        "purpose": "test child 1",
        "prompt": "run sync tool and finish",
        "profile": "agent-a"
    })
    .to_string();
    let spawn_args_2 = json!({
        "task_id": "child-2",
        "purpose": "test child 2",
        "prompt": "run sync tool and finish",
        "profile": "agent-a"
    })
    .to_string();

    let parent_first = sse(vec![
        ev_response_created("p-resp-1"),
        ev_function_call("call-1", "spawn_agent", &spawn_args_1),
        ev_function_call("call-2", "spawn_agent", &spawn_args_2),
        ev_completed("p-resp-1"),
    ]);
    // Serve parent first response once (first POST).
    let _parent_first_mock = mount_sse_once(&server, parent_first).await;

    // Child turn 1 for both children: model asks to call the barrier test tool.
    let barrier_args = json!({
        "sleep_after_ms": 300,
        "barrier": {"id": "spawn-agent-parallel", "participants": 2, "timeout_ms": 2_000}
    })
    .to_string();
    let child_first = sse(vec![
        ev_response_created("c-resp-1"),
        ev_function_call("child-call-1", "test_sync_tool", &barrier_args),
        ev_completed("c-resp-1"),
    ]);
    // Children may arrive in any order; allow unlimited matches for these bodies.
    let _child_first_a = mount_sse(&server, child_first.clone()).await;
    let _child_first_b = mount_sse(&server, child_first).await;

    // Child turn 2 for both children: assistant replies "done".
    let child_second = sse(vec![
        ev_assistant_message("c-msg-1", "done"),
        ev_completed("c-resp-2"),
    ]);
    let _child_second_a = mount_sse(&server, child_second.clone()).await;
    let _child_second_b = mount_sse(&server, child_second).await;

    // Parent second turn: after both children complete, the parent proceeds.
    let parent_second = sse(vec![
        ev_assistant_message("p-msg-1", "parent done"),
        ev_completed("p-resp-2"),
    ]);
    let _parent_second_mock = mount_sse_once(&server, parent_second).await;

    let duration = run_turn_and_measure(&test, "spawn two subagents").await?;

    // Expect the parent turn to block until both subagents finish (~300ms),
    // but still complete within a reasonable bound (< 1 second).
    assert!(
        duration >= Duration::from_millis(300),
        "expected at least ~300ms, got {:?}",
        duration
    );
    assert!(
        duration < Duration::from_millis(1_000),
        "expected to complete under 1s, got {:?}",
        duration
    );

    Ok(())
}
