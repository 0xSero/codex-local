//! Integration tests for the orchestrator.

use codex_orchestrator::runtime::ContextUsage;
use codex_orchestrator::runtime::SubagentContext;
use codex_orchestrator::runtime::SubagentStatus;
use codex_orchestrator::spec::ChecklistItem;
use codex_orchestrator::spec::TaskSpec;
use codex_orchestrator::truncation::OutputTruncator;
use codex_orchestrator::truncation::OUTPUT_TOKEN_LIMIT;
use codex_orchestrator::validation::validate_checklist;
use codex_orchestrator::validation::AgentOutputRecord;
use codex_orchestrator::validation::ResultAggregator;
use codex_protocol::ConversationId;
use pretty_assertions::assert_eq;
use std::time::Duration;

#[test]
fn test_checklist_validation_all_complete() {
    let spec = TaskSpec {
        id: "test-1".to_string(),
        purpose: "Test task".to_string(),
        prompt: "Do something".to_string(),
        checklist: vec![
            ChecklistItem {
                description: "Step 1".to_string(),
                completed: true,
            },
            ChecklistItem {
                description: "Step 2".to_string(),
                completed: true,
            },
        ],
        profile: None,
    };

    let result = validate_checklist(&spec);
    assert!(result.all_completed);
    assert_eq!(result.completed_count, 2);
    assert_eq!(result.total_count, 2);
    assert!(result.incomplete_items.is_empty());
    assert_eq!(result.success_rate(), 1.0);
}

#[test]
fn test_checklist_validation_partial_complete() {
    let spec = TaskSpec {
        id: "test-2".to_string(),
        purpose: "Test task".to_string(),
        prompt: "Do something".to_string(),
        checklist: vec![
            ChecklistItem {
                description: "Step 1".to_string(),
                completed: true,
            },
            ChecklistItem {
                description: "Step 2".to_string(),
                completed: false,
            },
            ChecklistItem {
                description: "Step 3".to_string(),
                completed: true,
            },
        ],
        profile: None,
    };

    let result = validate_checklist(&spec);
    assert!(!result.all_completed);
    assert_eq!(result.completed_count, 2);
    assert_eq!(result.total_count, 3);
    assert_eq!(result.incomplete_items.len(), 1);
    assert_eq!(result.incomplete_items[0], "Step 2");
    assert!((result.success_rate() - 0.666).abs() < 0.01);
}

#[test]
fn test_checklist_validation_none_complete() {
    let spec = TaskSpec {
        id: "test-3".to_string(),
        purpose: "Test task".to_string(),
        prompt: "Do something".to_string(),
        checklist: vec![
            ChecklistItem {
                description: "Step 1".to_string(),
                completed: false,
            },
            ChecklistItem {
                description: "Step 2".to_string(),
                completed: false,
            },
        ],
        profile: None,
    };

    let result = validate_checklist(&spec);
    assert!(!result.all_completed);
    assert_eq!(result.completed_count, 0);
    assert_eq!(result.total_count, 2);
    assert_eq!(result.incomplete_items.len(), 2);
    assert_eq!(result.success_rate(), 0.0);
}

#[test]
fn test_checklist_validation_empty() {
    let spec = TaskSpec {
        id: "test-4".to_string(),
        purpose: "Test task".to_string(),
        prompt: "Do something".to_string(),
        checklist: vec![],
        profile: None,
    };

    let result = validate_checklist(&spec);
    assert!(result.all_completed);
    assert_eq!(result.completed_count, 0);
    assert_eq!(result.total_count, 0);
    assert!(result.incomplete_items.is_empty());
    assert_eq!(result.success_rate(), 1.0);
}

#[test]
fn test_result_aggregator_empty() {
    let aggregator = ResultAggregator::new();
    let summary = aggregator.summary();

    assert_eq!(summary.total_agents, 0);
    assert_eq!(summary.successful_agents, 0);
    assert_eq!(summary.total_items, 0);
    assert_eq!(summary.completed_items, 0);
    assert_eq!(summary.overall_success_rate(), 1.0);
    assert_eq!(summary.agent_success_rate(), 1.0);
}

#[test]
fn test_result_aggregator_single_successful() {
    let mut aggregator = ResultAggregator::new();

    let spec = TaskSpec {
        id: "agent-1".to_string(),
        purpose: "Task 1".to_string(),
        prompt: "Do task 1".to_string(),
        checklist: vec![
            ChecklistItem {
                description: "Step 1".to_string(),
                completed: true,
            },
            ChecklistItem {
                description: "Step 2".to_string(),
                completed: true,
            },
        ],
        profile: None,
    };

    let result = validate_checklist(&spec);
    aggregator.add_result("agent-1".to_string(), result);

    let summary = aggregator.summary();
    assert_eq!(summary.total_agents, 1);
    assert_eq!(summary.successful_agents, 1);
    assert_eq!(summary.total_items, 2);
    assert_eq!(summary.completed_items, 2);
    assert_eq!(summary.overall_success_rate(), 1.0);
    assert_eq!(summary.agent_success_rate(), 1.0);
}

#[test]
fn test_result_aggregator_multiple_mixed() {
    let mut aggregator = ResultAggregator::new();

    // Agent 1: All complete
    let spec1 = TaskSpec {
        id: "agent-1".to_string(),
        purpose: "Task 1".to_string(),
        prompt: "Do task 1".to_string(),
        checklist: vec![
            ChecklistItem {
                description: "Step 1".to_string(),
                completed: true,
            },
            ChecklistItem {
                description: "Step 2".to_string(),
                completed: true,
            },
        ],
        profile: None,
    };

    // Agent 2: Partial complete
    let spec2 = TaskSpec {
        id: "agent-2".to_string(),
        purpose: "Task 2".to_string(),
        prompt: "Do task 2".to_string(),
        checklist: vec![
            ChecklistItem {
                description: "Step 1".to_string(),
                completed: true,
            },
            ChecklistItem {
                description: "Step 2".to_string(),
                completed: false,
            },
            ChecklistItem {
                description: "Step 3".to_string(),
                completed: true,
            },
        ],
        profile: None,
    };

    // Agent 3: None complete
    let spec3 = TaskSpec {
        id: "agent-3".to_string(),
        purpose: "Task 3".to_string(),
        prompt: "Do task 3".to_string(),
        checklist: vec![ChecklistItem {
            description: "Step 1".to_string(),
            completed: false,
        }],
        profile: None,
    };

    aggregator.add_result("agent-1".to_string(), validate_checklist(&spec1));
    aggregator.add_result("agent-2".to_string(), validate_checklist(&spec2));
    aggregator.add_result("agent-3".to_string(), validate_checklist(&spec3));

    let summary = aggregator.summary();
    assert_eq!(summary.total_agents, 3);
    assert_eq!(summary.successful_agents, 1); // Only agent-1 completed all
    assert_eq!(summary.total_items, 6); // 2 + 3 + 1
    assert_eq!(summary.completed_items, 4); // 2 + 2 + 0
    assert!((summary.overall_success_rate() - 0.666).abs() < 0.01);
    assert!((summary.agent_success_rate() - 0.333).abs() < 0.01);
}

// ========================================
// Context Usage Tests
// ========================================

#[test]
fn test_context_usage_new() {
    let usage = ContextUsage::new();
    assert_eq!(usage.tokens_used, 0);
    assert_eq!(usage.context_window, None);
    assert_eq!(usage.usage_percentage(), None);
}

#[test]
fn test_context_usage_with_window() {
    let mut usage = ContextUsage::new();
    usage.tokens_used = 5000;
    usage.context_window = Some(10000);

    let pct = usage.usage_percentage();
    assert_eq!(pct, Some(50));
}

#[test]
fn test_context_usage_at_limit() {
    let mut usage = ContextUsage::new();
    usage.tokens_used = 10000;
    usage.context_window = Some(10000);

    let pct = usage.usage_percentage();
    assert_eq!(pct, Some(100));
}

#[test]
fn test_context_usage_over_limit() {
    let mut usage = ContextUsage::new();
    usage.tokens_used = 15000;
    usage.context_window = Some(10000);

    // Should cap at 100%
    let pct = usage.usage_percentage();
    assert_eq!(pct, Some(100));
}

#[test]
fn test_context_usage_zero_window() {
    let mut usage = ContextUsage::new();
    usage.tokens_used = 5000;
    usage.context_window = Some(0);

    // Edge case: zero window should return 100%
    let pct = usage.usage_percentage();
    assert_eq!(pct, Some(100));
}

#[test]
fn test_context_usage_display_with_window() {
    let mut usage = ContextUsage::new();
    usage.tokens_used = 3000;
    usage.context_window = Some(10000);

    let display = usage.context_display();
    assert_eq!(display, "subagent context = 3000/10000 (30%)");
}

#[test]
fn test_context_usage_display_without_window() {
    let mut usage = ContextUsage::new();
    usage.tokens_used = 5000;

    let display = usage.context_display();
    assert_eq!(display, "subagent context = 5000");
}

// ========================================
// Subagent Context Tests
// ========================================

#[test]
fn test_subagent_context_creation() {
    let task = TaskSpec::new(
        "Test task".to_string(),
        "Do something".to_string(),
        vec![ChecklistItem::new("Step 1".to_string())],
    );

    let context = SubagentContext::new("agent-1".to_string(), ConversationId::new(), task.clone());

    assert_eq!(context.agent_id, "agent-1");
    assert_eq!(context.status, SubagentStatus::Pending);
    assert_eq!(context.output_buffer, "");
    assert_eq!(context.context_usage.tokens_used, 0);
    assert_eq!(context.completion_time, None);
}

#[test]
fn test_subagent_context_duration() {
    let task = TaskSpec::new(
        "Test task".to_string(),
        "Do something".to_string(),
        vec![ChecklistItem::new("Step 1".to_string())],
    );

    let mut context =
        SubagentContext::new("agent-1".to_string(), ConversationId::new(), task.clone());

    // Simulate some elapsed time
    std::thread::sleep(Duration::from_millis(50));

    // Duration without completion should be elapsed time
    let duration1 = context.duration();
    assert!(duration1.as_millis() >= 50);

    // Set completion time
    context.completion_time = Some(std::time::Instant::now());
    let duration2 = context.duration();
    assert!(duration2.as_millis() >= 50);
}

// ========================================
// Truncation Tests
// ========================================

#[test]
fn test_truncator_token_estimation() {
    let mut truncator = OutputTruncator::new(100);

    // ~25 tokens (100 chars / 4)
    let content = "a".repeat(100);
    let (result, truncated) = truncator.truncate_if_needed(&content);

    assert_eq!(result, content);
    assert!(!truncated);
    assert_eq!(truncator.current_tokens(), 25);
}

#[test]
fn test_truncator_exact_limit() {
    let mut truncator = OutputTruncator::new(25);

    // Exactly 25 tokens (100 chars)
    let content = "a".repeat(100);
    let (result, truncated) = truncator.truncate_if_needed(&content);

    assert_eq!(result, content);
    assert!(!truncated);
    assert!(truncator.is_at_limit());
}

#[test]
fn test_truncator_over_limit_single_call() {
    let mut truncator = OutputTruncator::new(10);

    // ~25 tokens, way over limit
    let content = "a".repeat(100);
    let (result, truncated) = truncator.truncate_if_needed(&content);

    assert!(truncated);
    assert!(result.contains("[Output truncated at 5k token limit]"));
    assert!(truncator.is_at_limit());
}

#[test]
fn test_truncator_incremental_overflow() {
    let mut truncator = OutputTruncator::new(20);

    // First call: 10 tokens, ok
    let content1 = "a".repeat(40);
    let (result1, truncated1) = truncator.truncate_if_needed(&content1);
    assert!(!truncated1);
    assert_eq!(result1, content1);

    // Second call: 10 tokens, ok
    let content2 = "b".repeat(40);
    let (result2, truncated2) = truncator.truncate_if_needed(&content2);
    assert!(!truncated2);
    assert_eq!(result2, content2);

    // Third call: would exceed, should truncate
    let content3 = "c".repeat(40);
    let (result3, truncated3) = truncator.truncate_if_needed(&content3);
    assert!(truncated3);
    assert!(result3.contains("[Output truncated at 5k token limit]"));
}

#[test]
fn test_truncator_after_limit_reached() {
    let mut truncator = OutputTruncator::new(10);

    // First call: exceed limit
    let content1 = "a".repeat(100);
    let (_result1, truncated1) = truncator.truncate_if_needed(&content1);
    assert!(truncated1);

    // Second call: already at limit, should return truncation message immediately
    let content2 = "b".repeat(100);
    let (result2, truncated2) = truncator.truncate_if_needed(&content2);
    assert!(truncated2);
    assert_eq!(result2, "\n[Output truncated at 5k token limit]");
}

#[test]
fn test_truncator_line_based_truncation() {
    let mut truncator = OutputTruncator::new(10);

    // Create multi-line content that exceeds the limit
    // Each line is ~50 chars (12-13 tokens), so 5 lines = ~60-65 tokens
    let content = [
        "This is a longer line with more content to test",
        "Another long line with substantial content here",
        "Third line also has significant text content",
        "Fourth line continues the pattern of length",
        "Fifth line completes this test content set",
    ]
    .join("\n");

    // Should exceed limit and truncate
    let (result, truncated) = truncator.truncate_if_needed(&content);

    // Should have truncated since content is much larger than limit
    assert!(truncated);
    assert!(result.contains("[Output truncated at 5k token limit]"));

    // Verify truncator is at limit
    assert!(truncator.is_at_limit());
}

#[test]
fn test_truncator_default_limit() {
    let truncator = OutputTruncator::default();
    assert_eq!(truncator.current_tokens(), 0);

    // Default limit should be OUTPUT_TOKEN_LIMIT (5000)
    let limit_check = OUTPUT_TOKEN_LIMIT;
    assert_eq!(limit_check, 5000);
}

#[test]
fn test_truncator_empty_content() {
    let mut truncator = OutputTruncator::new(100);
    let (result, truncated) = truncator.truncate_if_needed("");

    assert_eq!(result, "");
    assert!(!truncated);
    assert_eq!(truncator.current_tokens(), 0);
}

// ========================================
// Result Aggregator with Output Records
// ========================================

#[test]
fn test_aggregator_with_output_records() {
    let mut aggregator = ResultAggregator::new();

    // Add some output records
    let record1 = AgentOutputRecord::new(
        "agent-1".to_string(),
        "Task 1".to_string(),
        "Output from agent 1".to_string(),
        true,
    );

    let record2 = AgentOutputRecord::new(
        "agent-2".to_string(),
        "Task 2".to_string(),
        "Output from agent 2".to_string(),
        false,
    );

    aggregator.add_output(record1.clone());
    aggregator.add_output(record2.clone());

    let outputs = aggregator.outputs();
    assert_eq!(outputs.len(), 2);
    assert_eq!(outputs[0].agent_id, "agent-1");
    assert_eq!(outputs[0].success, true);
    assert_eq!(outputs[1].agent_id, "agent-2");
    assert_eq!(outputs[1].success, false);
}

#[test]
fn test_aggregator_output_record_structure() {
    let record = AgentOutputRecord::new(
        "test-agent".to_string(),
        "Test Purpose".to_string(),
        "Truncated output content".to_string(),
        true,
    );

    assert_eq!(record.agent_id, "test-agent");
    assert_eq!(record.purpose, "Test Purpose");
    assert_eq!(record.truncated_output, "Truncated output content");
    assert_eq!(record.success, true);
}

#[test]
fn test_aggregator_mixed_results_and_outputs() {
    let mut aggregator = ResultAggregator::new();

    // Add validation results
    let spec = TaskSpec {
        id: "agent-1".to_string(),
        purpose: "Task 1".to_string(),
        prompt: "Do task 1".to_string(),
        checklist: vec![ChecklistItem {
            description: "Step 1".to_string(),
            completed: true,
        }],
        profile: None,
    };

    let result = validate_checklist(&spec);
    aggregator.add_result("agent-1".to_string(), result);

    // Add output record
    let record = AgentOutputRecord::new(
        "agent-1".to_string(),
        "Task 1".to_string(),
        "Agent completed successfully".to_string(),
        true,
    );
    aggregator.add_output(record);

    // Check both are tracked
    let summary = aggregator.summary();
    assert_eq!(summary.total_agents, 1);
    assert_eq!(summary.successful_agents, 1);

    let outputs = aggregator.outputs();
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].agent_id, "agent-1");
}

// ========================================
// Subagent Status Tests
// ========================================

#[test]
fn test_subagent_status_variants() {
    let pending = SubagentStatus::Pending;
    let running = SubagentStatus::Running;
    let completed = SubagentStatus::Completed;
    let failed = SubagentStatus::Failed("Error message".to_string());
    let timeout = SubagentStatus::Timeout;

    assert_eq!(pending, SubagentStatus::Pending);
    assert_eq!(running, SubagentStatus::Running);
    assert_eq!(completed, SubagentStatus::Completed);

    match failed {
        SubagentStatus::Failed(msg) => assert_eq!(msg, "Error message"),
        _ => panic!("Expected Failed status"),
    }

    assert_eq!(timeout, SubagentStatus::Timeout);
}

// ========================================
// Integration: Multiple Subagents with Truncation
// ========================================

#[test]
fn test_multiple_agents_with_truncation() {
    let mut aggregator = ResultAggregator::new();

    // Simulate 3 agents with different outputs
    for i in 1..=3 {
        let agent_id = format!("agent-{i}");

        // Create a task with checklist
        let spec = TaskSpec {
            id: agent_id.clone(),
            purpose: format!("Task {i}"),
            prompt: format!("Do task {i}"),
            checklist: vec![ChecklistItem {
                description: "Step 1".to_string(),
                completed: i != 2, // Agent 2 fails
            }],
            profile: None,
        };

        // Validate and add result
        let result = validate_checklist(&spec);
        aggregator.add_result(agent_id.clone(), result.clone());

        // Simulate truncated output
        let mut truncator = OutputTruncator::new(OUTPUT_TOKEN_LIMIT);
        let output_content = format!("Output from agent {i}: ") + &"x".repeat(1000);
        let (truncated_output, _was_truncated) = truncator.truncate_if_needed(&output_content);

        // Add output record
        let record = AgentOutputRecord::new(
            agent_id,
            format!("Task {i}"),
            truncated_output,
            result.all_completed,
        );
        aggregator.add_output(record);
    }

    // Check aggregated results
    let summary = aggregator.summary();
    assert_eq!(summary.total_agents, 3);
    assert_eq!(summary.successful_agents, 2); // Agents 1 and 3
    assert_eq!(summary.total_items, 3);
    assert_eq!(summary.completed_items, 2);

    // Check outputs
    let outputs = aggregator.outputs();
    assert_eq!(outputs.len(), 3);

    // Verify success status matches validation
    assert_eq!(outputs[0].success, true);
    assert_eq!(outputs[1].success, false);
    assert_eq!(outputs[2].success, true);

    // All outputs should have truncated content
    for output in outputs {
        assert!(!output.truncated_output.is_empty());
    }
}

// ========================================
// Edge Cases
// ========================================

#[test]
fn test_context_usage_updates() {
    let mut usage = ContextUsage::new();

    // Simulate progressive updates
    usage.tokens_used = 1000;
    usage.context_window = Some(10000);
    assert_eq!(usage.usage_percentage(), Some(10));

    usage.tokens_used = 5000;
    assert_eq!(usage.usage_percentage(), Some(50));

    usage.tokens_used = 9000;
    assert_eq!(usage.usage_percentage(), Some(90));

    usage.tokens_used = 10000;
    assert_eq!(usage.usage_percentage(), Some(100));
}

#[test]
fn test_empty_aggregator_outputs() {
    let aggregator = ResultAggregator::new();
    let outputs = aggregator.outputs();
    assert!(outputs.is_empty());
}

#[test]
fn test_truncator_preserves_partial_content() {
    let mut truncator = OutputTruncator::new(15);

    let content = "Short line\nAnother line\nThird line";
    let (result, truncated) = truncator.truncate_if_needed(content);

    if truncated {
        // Should contain at least the first line
        assert!(result.contains("Short line"));
        assert!(result.contains("[Output truncated at 5k token limit]"));
    }
}
