//! Integration tests for the orchestrator.

use codex_orchestrator::spec::ChecklistItem;
use codex_orchestrator::spec::TaskSpec;
use codex_orchestrator::validation::validate_checklist;
use codex_orchestrator::validation::ResultAggregator;

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
