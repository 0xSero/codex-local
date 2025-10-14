use serde::Deserialize;
use std::path::PathBuf;

use crate::protocol::AskForApproval;
use codex_protocol::config_types::ReasoningEffort;
use codex_protocol::config_types::ReasoningSummary;
use codex_protocol::config_types::Verbosity;

/// Collection of common configuration options that a user can define as a unit
/// in `config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ConfigProfile {
    pub model: Option<String>,
    /// The key in the `model_providers` map identifying the
    /// [`ModelProviderInfo`] to use.
    pub model_provider: Option<String>,
    pub approval_policy: Option<AskForApproval>,
    pub model_reasoning_effort: Option<ReasoningEffort>,
    pub model_reasoning_summary: Option<ReasoningSummary>,
    pub model_verbosity: Option<Verbosity>,
    pub chatgpt_base_url: Option<String>,
    pub experimental_instructions_file: Option<PathBuf>,
    /// Size of the context window for the model, in tokens.
    pub model_context_window: Option<u64>,
    /// Maximum number of output tokens.
    pub model_max_output_tokens: Option<u64>,
    /// Token usage threshold triggering auto-compaction of conversation history.
    pub model_auto_compact_token_limit: Option<i64>,
}

impl From<ConfigProfile> for codex_app_server_protocol::Profile {
    fn from(config_profile: ConfigProfile) -> Self {
        Self {
            model: config_profile.model,
            model_provider: config_profile.model_provider,
            approval_policy: config_profile.approval_policy,
            model_reasoning_effort: config_profile.model_reasoning_effort,
            model_reasoning_summary: config_profile.model_reasoning_summary,
            model_verbosity: config_profile.model_verbosity,
            chatgpt_base_url: config_profile.chatgpt_base_url,
            model_context_window: config_profile.model_context_window,
            model_max_output_tokens: config_profile.model_max_output_tokens,
            model_auto_compact_token_limit: config_profile.model_auto_compact_token_limit,
        }
    }
}
