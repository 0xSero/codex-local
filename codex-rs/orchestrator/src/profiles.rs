//! Profile selection and management for child agents.

use codex_core::config::Config;

/// Selects an appropriate profile for a child agent task.
pub struct ProfileSelector {
    available_profiles: Vec<String>,
}

impl ProfileSelector {
    pub fn new(config: &Config) -> Self {
        Self {
            available_profiles: config.active_agent_profiles.clone(),
        }
    }

    /// Selects a profile for a task. If a specific profile is requested and available,
    /// uses that. Otherwise, falls back to the first available profile or None.
    pub fn select_profile(&self, requested: Option<&str>) -> Option<String> {
        if let Some(req) = requested {
            if self.available_profiles.contains(&req.to_string()) {
                return Some(req.to_string());
            }
        }

        // Fallback to first available profile
        self.available_profiles.first().cloned()
    }

    /// Returns all available profiles.
    pub fn available_profiles(&self) -> &[String] {
        &self.available_profiles
    }
}
