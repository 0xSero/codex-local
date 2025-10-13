//! Checklist validation and result aggregation.

use crate::spec::TaskSpec;

/// Result of validating a task against its checklist.
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub all_completed: bool,
    pub completed_count: usize,
    pub total_count: usize,
    pub incomplete_items: Vec<String>,
}

impl ValidationResult {
    pub fn success_rate(&self) -> f64 {
        if self.total_count == 0 {
            return 1.0;
        }
        self.completed_count as f64 / self.total_count as f64
    }
}

/// Validates a task's completion against its checklist.
pub fn validate_checklist(spec: &TaskSpec) -> ValidationResult {
    let total_count = spec.checklist.len();
    let completed_count = spec.checklist.iter().filter(|item| item.completed).count();
    let all_completed = completed_count == total_count;

    let incomplete_items: Vec<String> = spec
        .checklist
        .iter()
        .filter(|item| !item.completed)
        .map(|item| item.description.clone())
        .collect();

    ValidationResult {
        all_completed,
        completed_count,
        total_count,
        incomplete_items,
    }
}

/// Aggregates results from multiple child agents.
pub struct ResultAggregator {
    results: Vec<(String, ValidationResult)>,
}

impl ResultAggregator {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
        }
    }

    pub fn add_result(&mut self, agent_id: String, result: ValidationResult) {
        self.results.push((agent_id, result));
    }

    pub fn summary(&self) -> AggregateSummary {
        let total_agents = self.results.len();
        let successful_agents = self.results.iter().filter(|(_, r)| r.all_completed).count();

        let total_items: usize = self.results.iter().map(|(_, r)| r.total_count).sum();
        let completed_items: usize = self.results.iter().map(|(_, r)| r.completed_count).sum();

        AggregateSummary {
            total_agents,
            successful_agents,
            total_items,
            completed_items,
        }
    }
}

impl Default for ResultAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct AggregateSummary {
    pub total_agents: usize,
    pub successful_agents: usize,
    pub total_items: usize,
    pub completed_items: usize,
}

impl AggregateSummary {
    pub fn overall_success_rate(&self) -> f64 {
        if self.total_items == 0 {
            return 1.0;
        }
        self.completed_items as f64 / self.total_items as f64
    }

    pub fn agent_success_rate(&self) -> f64 {
        if self.total_agents == 0 {
            return 1.0;
        }
        self.successful_agents as f64 / self.total_agents as f64
    }
}
