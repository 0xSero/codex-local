use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use codex_core::config::Config;
use codex_core::config_types::Notifications;
use codex_core::git_info::current_branch_name;
use codex_core::git_info::local_git_branches;
use codex_core::protocol::AgentEvent;
use codex_core::protocol::AgentMessageDeltaEvent;
use codex_core::protocol::AgentMessageEvent;
use codex_core::protocol::AgentReasoningDeltaEvent;
use codex_core::protocol::AgentReasoningEvent;
use codex_core::protocol::AgentReasoningRawContentDeltaEvent;
use codex_core::protocol::AgentReasoningRawContentEvent;
use codex_core::protocol::ApplyPatchApprovalRequestEvent;
use codex_core::protocol::BackgroundEventEvent;
use codex_core::protocol::ErrorEvent;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::ExecApprovalRequestEvent;
use codex_core::protocol::ExecCommandBeginEvent;
use codex_core::protocol::ExecCommandEndEvent;
use codex_core::protocol::ExitedReviewModeEvent;
use codex_core::protocol::InputItem;
use codex_core::protocol::InputMessageKind;
use codex_core::protocol::ListCustomPromptsResponseEvent;
use codex_core::protocol::McpListToolsResponseEvent;
use codex_core::protocol::McpToolCallBeginEvent;
use codex_core::protocol::McpToolCallEndEvent;
use codex_core::protocol::Op;
use codex_core::protocol::PatchApplyBeginEvent;
use codex_core::protocol::RateLimitSnapshot;
use codex_core::protocol::ReviewRequest;
use codex_core::protocol::StreamErrorEvent;
use codex_core::protocol::TaskCompleteEvent;
use codex_core::protocol::TokenUsage;
use codex_core::protocol::TokenUsageInfo;
use codex_core::protocol::TurnAbortReason;
use codex_core::protocol::TurnDiffEvent;
use codex_core::protocol::UserMessageEvent;
use codex_core::protocol::ViewImageToolCallEvent;
use codex_core::protocol::WebSearchBeginEvent;
use codex_core::protocol::WebSearchEndEvent;
use codex_protocol::ConversationId;
use codex_protocol::parse_command::ParsedCommand;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use rand::Rng;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;
use tokio::sync::mpsc::UnboundedSender;
use tracing::debug;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::ApprovalRequest;
use crate::bottom_pane::BottomPane;
use crate::bottom_pane::BottomPaneParams;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::FooterSubagentInfo;
use crate::bottom_pane::InputResult;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::custom_prompt_view::CustomPromptView;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::clipboard_paste::paste_image_to_temp_png;
use crate::diff_render::display_path_for;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::ExecCell;
use crate::exec_cell::new_active_exec_command;
use crate::get_git_diff::get_git_diff;
use crate::history_cell;
use crate::history_cell::AgentDecoratedCell;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::McpToolCallCell;
use crate::history_cell::PlainHistoryCell;
use crate::markdown::append_markdown;
use crate::slash_command::SlashCommand;
use crate::status::RateLimitSnapshotDisplay;
use crate::text_formatting::truncate_text;
use crate::tui::FrameRequester;
mod interrupts;
use self::interrupts::InterruptManager;
mod agent;
use self::agent::spawn_agent;
use self::agent::spawn_agent_from_existing;
mod session_header;
use self::session_header::SessionHeader;
use crate::streaming::controller::StreamController;
use std::path::Path;

use chrono::Local;
use codex_common::approval_presets::ApprovalPreset;
use codex_common::approval_presets::builtin_approval_presets;
use codex_common::model_presets::ModelPreset;
use codex_common::model_presets::builtin_model_presets;
use codex_core::AuthManager;
use codex_core::ConversationManager;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::SandboxPolicy;
use codex_core::protocol_config_types::ReasoningEffort as ReasoningEffortConfig;
use codex_file_search::FileMatch;
use codex_git_tooling::CreateGhostCommitOptions;
use codex_git_tooling::GhostCommit;
use codex_git_tooling::GitToolingError;
use codex_git_tooling::create_ghost_commit;
use codex_git_tooling::restore_ghost_commit;
use codex_protocol::plan_tool::UpdatePlanArgs;
use strum::IntoEnumIterator;

const MAX_TRACKED_GHOST_COMMITS: usize = 20;

// Track information about an in-flight exec command.
struct RunningCommand {
    command: Vec<String>,
    parsed_cmd: Vec<ParsedCommand>,
}

const RATE_LIMIT_WARNING_THRESHOLDS: [f64; 3] = [75.0, 90.0, 95.0];

#[derive(Default)]
struct RateLimitWarningState {
    secondary_index: usize,
    primary_index: usize,
}

impl RateLimitWarningState {
    fn take_warnings(
        &mut self,
        secondary_used_percent: Option<f64>,
        secondary_window_minutes: Option<u64>,
        primary_used_percent: Option<f64>,
        primary_window_minutes: Option<u64>,
    ) -> Vec<String> {
        let reached_secondary_cap =
            matches!(secondary_used_percent, Some(percent) if percent == 100.0);
        let reached_primary_cap = matches!(primary_used_percent, Some(percent) if percent == 100.0);
        if reached_secondary_cap || reached_primary_cap {
            return Vec::new();
        }

        let mut warnings = Vec::new();

        if let Some(secondary_used_percent) = secondary_used_percent {
            let mut highest_secondary: Option<f64> = None;
            while self.secondary_index < RATE_LIMIT_WARNING_THRESHOLDS.len()
                && secondary_used_percent >= RATE_LIMIT_WARNING_THRESHOLDS[self.secondary_index]
            {
                highest_secondary = Some(RATE_LIMIT_WARNING_THRESHOLDS[self.secondary_index]);
                self.secondary_index += 1;
            }
            if let Some(threshold) = highest_secondary {
                let limit_label = secondary_window_minutes
                    .map(get_limits_duration)
                    .unwrap_or_else(|| "weekly".to_string());
                warnings.push(format!(
                    "Heads up, you've used over {threshold:.0}% of your {limit_label} limit. Run /status for a breakdown."
                ));
            }
        }

        if let Some(primary_used_percent) = primary_used_percent {
            let mut highest_primary: Option<f64> = None;
            while self.primary_index < RATE_LIMIT_WARNING_THRESHOLDS.len()
                && primary_used_percent >= RATE_LIMIT_WARNING_THRESHOLDS[self.primary_index]
            {
                highest_primary = Some(RATE_LIMIT_WARNING_THRESHOLDS[self.primary_index]);
                self.primary_index += 1;
            }
            if let Some(threshold) = highest_primary {
                let limit_label = primary_window_minutes
                    .map(get_limits_duration)
                    .unwrap_or_else(|| "5h".to_string());
                warnings.push(format!(
                    "Heads up, you've used over {threshold:.0}% of your {limit_label} limit. Run /status for a breakdown."
                ));
            }
        }

        warnings
    }
}

pub(crate) fn get_limits_duration(windows_minutes: u64) -> String {
    const MINUTES_PER_HOUR: u64 = 60;
    const MINUTES_PER_DAY: u64 = 24 * MINUTES_PER_HOUR;
    const MINUTES_PER_WEEK: u64 = 7 * MINUTES_PER_DAY;
    const MINUTES_PER_MONTH: u64 = 30 * MINUTES_PER_DAY;
    const ROUNDING_BIAS_MINUTES: u64 = 3;

    if windows_minutes <= MINUTES_PER_DAY.saturating_add(ROUNDING_BIAS_MINUTES) {
        let adjusted = windows_minutes.saturating_add(ROUNDING_BIAS_MINUTES);
        let hours = std::cmp::max(1, adjusted / MINUTES_PER_HOUR);
        format!("{hours}h")
    } else if windows_minutes <= MINUTES_PER_WEEK.saturating_add(ROUNDING_BIAS_MINUTES) {
        "weekly".to_string()
    } else if windows_minutes <= MINUTES_PER_MONTH.saturating_add(ROUNDING_BIAS_MINUTES) {
        "monthly".to_string()
    } else {
        "annual".to_string()
    }
}

#[derive(Clone, Debug, Default)]
struct AgentMeta {
    profile: Option<String>,
    purpose: String,
    // None = in progress, Some(true) = success, Some(false) = failed
    status: Option<bool>,
    last_summary: Option<String>,
    transcript: Vec<String>,
    context_percent: Option<u8>,
}

struct SubAgentState {
    color: Color,
    active_cell: Option<Box<dyn HistoryCell>>,
    running_commands: HashMap<String, (Vec<String>, Vec<ParsedCommand>)>,
    message_buffer: String,
    reasoning_buffer: String,
    full_reasoning_buffer: String,
    context_percent: Option<u8>,
    last_status: Option<String>,
}

impl SubAgentState {
    fn new(color: Color) -> Self {
        Self {
            color,
            active_cell: None,
            running_commands: HashMap::new(),
            message_buffer: String::new(),
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            context_percent: None,
            last_status: None,
        }
    }

    fn color(&self) -> Color {
        self.color
    }

    fn footer_status(&self) -> Option<String> {
        self.last_status.clone()
    }

    fn context_percent(&self) -> Option<u8> {
        self.context_percent
    }

    fn set_context_percent(&mut self, percent: u8) {
        self.context_percent = Some(percent);
    }

    fn handle_event(&mut self, event: Event, config: &Config) -> Vec<Box<dyn HistoryCell>> {
        let mut outputs: Vec<Box<dyn HistoryCell>> = Vec::new();
        match event.msg {
            EventMsg::TaskStarted(_) => {
                self.record_status("started");
            }
            EventMsg::AgentMessageDelta(delta) => {
                self.message_buffer.push_str(&delta.delta);
            }
            EventMsg::AgentMessage(ev) => {
                // For subagents, avoid spam in the transcript while still
                // keeping status fresh. Only record the status here; final
                // message will be rendered via AgentCompleted summary.
                self.message_buffer = ev.message;
                self.record_status("responded");
                self.message_buffer.clear();
            }
            EventMsg::AgentReasoningDelta(delta) => {
                self.reasoning_buffer.push_str(&delta.delta);
            }
            EventMsg::AgentReasoning(ev) => {
                if !ev.text.trim().is_empty() {
                    self.reasoning_buffer.push_str(&ev.text);
                }
                // Collapse reasoning blocks for subagents to keep the
                // transcript clean; surface activity via footer status only.
                self.reasoning_buffer.clear();
                self.full_reasoning_buffer.clear();
                self.record_status("reasoning");
            }
            EventMsg::AgentReasoningRawContentDelta(delta) => {
                self.reasoning_buffer.push_str(&delta.delta);
            }
            EventMsg::AgentReasoningRawContent(ev) => {
                if !ev.text.trim().is_empty() {
                    self.reasoning_buffer.push_str(&ev.text);
                }
                // Do not render raw reasoning for subagents; keep footer updated.
                self.reasoning_buffer.clear();
                self.full_reasoning_buffer.clear();
                self.record_status("reasoning");
            }
            EventMsg::AgentReasoningSectionBreak(_) => {
                self.reasoning_section_break();
            }
            EventMsg::ExecCommandBegin(ev) => {
                // Suppress per-call exec rendering for subagents; keep a concise status
                let snippet = join_command_preview(&ev.command);
                self.record_status(&format!("exec {snippet}"));
            }
            EventMsg::ExecCommandEnd(ev) => {
                // Suppress per-call exec rendering for subagents; update status
                self.record_status(&format!("exec exit {}", ev.exit_code));
            }
            EventMsg::McpToolCallBegin(ev) => {
                // Avoid verbose tool call content for subagents
                self.record_status(&format!(
                    "tool {}.{}",
                    ev.invocation.server, ev.invocation.tool
                ));
            }
            EventMsg::McpToolCallEnd(ev) => {
                // Only reflect final status
                self.record_status(&format!(
                    "tool {}.{} {}",
                    ev.invocation.server,
                    ev.invocation.tool,
                    if ev.is_success() { "ok" } else { "failed" }
                ));
            }
            EventMsg::ViewImageToolCall(ev) => {
                // Skip image/tool visualization for subagents; update status only
                self.record_status("viewed image");
            }
            EventMsg::WebSearchBegin(_) => {
                self.record_status("searching…");
            }
            EventMsg::WebSearchEnd(ev) => {
                // Keep a short status only
                self.record_status(&format!("search \"{}\"", ev.query));
            }
            EventMsg::TokenCount(ev) => {
                if let Some(info) = ev.info
                    && let Some(ctx) = info.model_context_window
                {
                    let pct = info
                        .last_token_usage
                        .percent_of_context_window_remaining(ctx);
                    self.set_context_percent(pct);
                }
            }
            EventMsg::StreamError(ev) => {
                self.record_status(&format!("stream error: {}", ev.message));
            }
            EventMsg::BackgroundEvent(ev) => {
                self.record_status(&ev.message);
            }
            EventMsg::TaskComplete(TaskCompleteEvent { last_agent_message }) => {
                self.flush_message(last_agent_message, config, &mut outputs);
                self.flush_reasoning(config, &mut outputs);
                self.flush_active_cell(&mut outputs);
            }
            EventMsg::Error(err) => {
                outputs.push(
                    self.decorate_body(Self::boxed(history_cell::new_error_event(
                        err.message.clone(),
                    ))),
                );
                self.record_status(&format!("error: {}", err.message));
            }
            EventMsg::PlanUpdate(_) | EventMsg::TurnDiff(_) | EventMsg::ConversationPath(_) => {}
            EventMsg::ExecCommandOutputDelta(_) => {}
            EventMsg::TurnAborted(ev) => {
                self.record_status(&format!("aborted ({:?})", ev.reason));
                self.flush_message(None, config, &mut outputs);
                self.flush_reasoning(config, &mut outputs);
                self.flush_active_cell(&mut outputs);
            }
            _ => {}
        }
        outputs
    }

    fn completion_cells(
        &mut self,
        success: bool,
        summary: String,
        config: &Config,
    ) -> Vec<Box<dyn HistoryCell>> {
        let mut outputs = Vec::new();
        self.flush_message(None, config, &mut outputs);
        self.flush_reasoning(config, &mut outputs);
        self.flush_active_cell(&mut outputs);

        let status_line: Line<'static> = if success {
            "✓ completed".green().bold().into()
        } else {
            "✗ failed".red().bold().into()
        };

        let mut lines: Vec<Line<'static>> = vec![status_line];
        let trimmed_summary = summary.trim().to_owned();
        if !trimmed_summary.is_empty() {
            lines.push(Line::from(trimmed_summary));
        }
        if let Some(pct) = self.context_percent {
            lines.push(format!("context remaining: {pct}%").dim().into());
        }
        let footer_cell = PlainHistoryCell::new(lines);
        outputs.push(self.decorate_footer(Self::boxed(footer_cell)));

        self.last_status = Some(summary);
        outputs
    }

    fn record_status(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        let first_line = trimmed.lines().next().unwrap_or(trimmed);
        let mut shortened: String = first_line.chars().take(80).collect();
        if first_line.chars().count() > 80 {
            shortened.push('…');
        }
        self.last_status = Some(shortened);
    }

    fn decorate_with(
        &self,
        cell: Box<dyn HistoryCell>,
        first_prefix: Line<'static>,
        other_prefix: Line<'static>,
    ) -> Box<dyn HistoryCell> {
        Box::new(AgentDecoratedCell::new(cell, first_prefix, other_prefix))
    }

    fn boxed<T: HistoryCell + 'static>(cell: T) -> Box<dyn HistoryCell> {
        Box::new(cell)
    }

    fn decorate_header(&self, cell: Box<dyn HistoryCell>) -> Box<dyn HistoryCell> {
        // Header gets a clean, subtle treatment with the agent color
        let header_bullet = Span::styled("▪ ", Style::default().fg(self.color));
        let header_prefix = Line::from(vec![header_bullet]);
        let body_prefix = Line::from(vec![Span::styled(
            "  ",
            Style::default().fg(self.color).dim(),
        )]);
        self.decorate_with(cell, header_prefix, body_prefix)
    }

    fn decorate_body(&self, cell: Box<dyn HistoryCell>) -> Box<dyn HistoryCell> {
        // Create a clean card-like layout with subtle indentation
        let indent = Span::styled("   ", Style::default().fg(self.color));
        let prefix = Line::from(vec![indent]);
        self.decorate_with(cell, prefix.clone(), prefix)
    }

    fn decorate_footer(&self, cell: Box<dyn HistoryCell>) -> Box<dyn HistoryCell> {
        // Footer gets minimal treatment
        let prefix = Line::from(vec![Span::styled(
            "   ",
            Style::default().fg(self.color).dim(),
        )]);
        self.decorate_with(cell, prefix.clone(), prefix)
    }

    fn make_prefix(symbol: &str, color: Color) -> Line<'static> {
        let span = Span::styled(symbol.to_string(), Style::default().fg(color));
        Line::from(vec![span])
    }

    fn flush_message(
        &mut self,
        message: Option<String>,
        config: &Config,
        outputs: &mut Vec<Box<dyn HistoryCell>>,
    ) {
        if let Some(msg) = message {
            self.message_buffer = msg;
        }
        let trimmed = self.message_buffer.trim();
        if trimmed.is_empty() {
            self.message_buffer.clear();
            return;
        }
        let mut lines: Vec<Line<'static>> = Vec::new();
        append_markdown(trimmed, None, &mut lines, config);
        let cell = AgentMessageCell::new(lines, true);
        outputs.push(self.decorate_body(Self::boxed(cell)));
        let status = trimmed.to_string();
        self.record_status(&status);
        self.message_buffer.clear();
    }

    fn flush_reasoning(&mut self, config: &Config, outputs: &mut Vec<Box<dyn HistoryCell>>) {
        if self.reasoning_buffer.trim().is_empty() && self.full_reasoning_buffer.trim().is_empty() {
            self.reasoning_buffer.clear();
            self.full_reasoning_buffer.clear();
            return;
        }

        if !self.full_reasoning_buffer.is_empty() && !self.reasoning_buffer.is_empty() {
            self.full_reasoning_buffer.push_str("\n\n");
        }
        self.full_reasoning_buffer
            .push_str(self.reasoning_buffer.trim_end());

        let summary = self.full_reasoning_buffer.trim();
        if !summary.is_empty() {
            let cell = history_cell::new_reasoning_summary_block(summary.to_string(), config);
            // Don't decorate reasoning blocks - let them render with same style as main agent
            outputs.push(cell);
            self.record_status("reasoned");
        }
        self.reasoning_buffer.clear();
        self.full_reasoning_buffer.clear();
    }

    fn reasoning_section_break(&mut self) {
        // Skip if reasoning buffer is empty or contains only whitespace
        if self.reasoning_buffer.trim().is_empty() {
            self.reasoning_buffer.clear();
            return;
        }
        if !self.full_reasoning_buffer.is_empty() {
            self.full_reasoning_buffer.push_str("\n\n");
        }
        self.full_reasoning_buffer
            .push_str(self.reasoning_buffer.trim_end());
        self.reasoning_buffer.clear();
    }

    fn flush_active_cell(&mut self, outputs: &mut Vec<Box<dyn HistoryCell>>) {
        if let Some(cell) = self.active_cell.take() {
            outputs.push(self.decorate_body(cell));
        }
    }

    fn handle_exec_begin(
        &mut self,
        ev: ExecCommandBeginEvent,
        config: &Config,
        outputs: &mut Vec<Box<dyn HistoryCell>>,
    ) {
        self.flush_message(None, config, outputs);
        self.flush_reasoning(config, outputs);
        self.flush_active_cell(outputs);

        self.running_commands.insert(
            ev.call_id.clone(),
            (ev.command.clone(), ev.parsed_cmd.clone()),
        );

        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<ExecCell>())
            && let Some(new_cell) = cell.with_added_call(
                ev.call_id.clone(),
                ev.command.clone(),
                ev.parsed_cmd.clone(),
            )
        {
            *cell = new_cell;
        } else {
            self.active_cell = Some(Box::new(new_active_exec_command(
                ev.call_id.clone(),
                ev.command.clone(),
                ev.parsed_cmd.clone(),
            )));
        }

        let snippet = join_command_preview(&ev.command);
        self.record_status(&format!("exec {snippet}"));
    }

    fn handle_exec_end(
        &mut self,
        ev: ExecCommandEndEvent,
        outputs: &mut Vec<Box<dyn HistoryCell>>,
    ) {
        let (command, parsed) = self
            .running_commands
            .remove(&ev.call_id)
            .unwrap_or_else(|| (vec![ev.call_id.clone()], Vec::new()));

        let needs_new = self
            .active_cell
            .as_ref()
            .map(|cell| cell.as_any().downcast_ref::<ExecCell>().is_none())
            .unwrap_or(true);
        if needs_new {
            self.flush_active_cell(outputs);
            self.active_cell = Some(Box::new(new_active_exec_command(
                ev.call_id.clone(),
                command,
                parsed,
            )));
        }

        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<ExecCell>())
        {
            cell.complete_call(
                &ev.call_id,
                CommandOutput {
                    exit_code: ev.exit_code,
                    stdout: ev.stdout.clone(),
                    stderr: ev.stderr.clone(),
                    formatted_output: ev.formatted_output.clone(),
                },
                ev.duration,
            );
            if cell.should_flush() {
                self.flush_active_cell(outputs);
            }
        }
        self.record_status(&format!("exec exit {}", ev.exit_code));
    }

    fn handle_mcp_begin(
        &mut self,
        ev: McpToolCallBeginEvent,
        config: &Config,
        outputs: &mut Vec<Box<dyn HistoryCell>>,
    ) {
        self.flush_message(None, config, outputs);
        self.flush_reasoning(config, outputs);
        self.flush_active_cell(outputs);
        self.active_cell = Some(Box::new(history_cell::new_active_mcp_tool_call(
            ev.call_id.clone(),
            ev.invocation.clone(),
        )));
        self.record_status(&format!(
            "tool {}.{}",
            ev.invocation.server, ev.invocation.tool
        ));
    }

    fn handle_mcp_end(&mut self, ev: McpToolCallEndEvent, outputs: &mut Vec<Box<dyn HistoryCell>>) {
        let extra_cell = match self
            .active_cell
            .as_mut()
            .and_then(|cell| cell.as_any_mut().downcast_mut::<McpToolCallCell>())
        {
            Some(cell) if cell.call_id() == ev.call_id => {
                cell.complete(ev.duration, ev.result.clone())
            }
            _ => {
                let mut cell = history_cell::new_active_mcp_tool_call(
                    ev.call_id.clone(),
                    ev.invocation.clone(),
                );
                let extra = cell.complete(ev.duration, ev.result.clone());
                self.active_cell = Some(Box::new(cell));
                extra
            }
        };

        self.flush_active_cell(outputs);
        if let Some(extra) = extra_cell {
            outputs.push(self.decorate_body(extra));
        }

        self.record_status(&format!(
            "tool {}.{} {}",
            ev.invocation.server,
            ev.invocation.tool,
            if ev.is_success() { "ok" } else { "failed" }
        ));
    }
}

fn join_command_preview(parts: &[String]) -> String {
    let joined =
        shlex::try_join(parts.iter().map(String::as_str)).unwrap_or_else(|_| parts.join(" "));
    let mut truncated: String = joined.chars().take(60).collect();
    if joined.chars().count() > 60 {
        truncated.push('…');
    }
    truncated
}
/// Common initialization parameters shared by all `ChatWidget` constructors.
pub(crate) struct ChatWidgetInit {
    pub(crate) config: Config,
    pub(crate) frame_requester: FrameRequester,
    pub(crate) app_event_tx: AppEventSender,
    pub(crate) initial_prompt: Option<String>,
    pub(crate) initial_images: Vec<PathBuf>,
    pub(crate) enhanced_keys_supported: bool,
    pub(crate) auth_manager: Arc<AuthManager>,
}

pub(crate) struct ChatWidget {
    app_event_tx: AppEventSender,
    codex_op_tx: UnboundedSender<Op>,
    bottom_pane: BottomPane,
    active_cell: Option<Box<dyn HistoryCell>>,
    config: Config,
    auth_manager: Arc<AuthManager>,
    session_header: SessionHeader,
    initial_user_message: Option<UserMessage>,
    token_info: Option<TokenUsageInfo>,
    rate_limit_snapshot: Option<RateLimitSnapshotDisplay>,
    rate_limit_warnings: RateLimitWarningState,
    // Stream lifecycle controller
    stream_controller: Option<StreamController>,
    running_commands: HashMap<String, RunningCommand>,
    task_complete_pending: bool,
    // Queue of interruptive UI events deferred during an active write cycle
    interrupts: InterruptManager,
    // Accumulates the current reasoning block text to extract a header
    reasoning_buffer: String,
    // Accumulates full reasoning content for transcript-only recording
    full_reasoning_buffer: String,
    // Current status header shown in the status indicator.
    current_status_header: String,
    // Previous status header to restore after a transient stream retry.
    retry_status_header: Option<String>,
    conversation_id: Option<ConversationId>,
    frame_requester: FrameRequester,
    // Whether to include the initial welcome banner on session configured
    show_welcome_banner: bool,
    // When resuming an existing session (selected via resume picker), avoid an
    // immediate redraw on SessionConfigured to prevent a gratuitous UI flicker.
    suppress_session_configured_redraw: bool,
    // User messages queued while a turn is in progress
    queued_user_messages: VecDeque<UserMessage>,
    // Pending notification to show when unfocused on next Draw
    pending_notification: Option<Notification>,
    // Simple review mode flag; used to adjust layout and banners.
    is_review_mode: bool,
    // List of ghost commits corresponding to each turn.
    ghost_snapshots: Vec<GhostCommit>,
    ghost_snapshots_disabled: bool,
    // Whether to add a final message separator after the last message
    needs_final_message_separator: bool,

    last_rendered_width: std::cell::Cell<Option<usize>>,

    // Token tracking for codex-local
    session_input_tokens: u64,
    session_output_tokens: u64,
    current_turn_input_tokens: u64,
    // Tracked spawned agents for overview rendering
    agents: HashMap<String, AgentMeta>,
    subagent_states: HashMap<String, SubAgentState>,
    active_subagent: Option<String>,
}

struct UserMessage {
    text: String,
    image_paths: Vec<PathBuf>,
}

impl From<String> for UserMessage {
    fn from(text: String) -> Self {
        Self {
            text,
            image_paths: Vec::new(),
        }
    }
}

fn create_initial_user_message(text: String, image_paths: Vec<PathBuf>) -> Option<UserMessage> {
    if text.is_empty() && image_paths.is_empty() {
        None
    } else {
        Some(UserMessage { text, image_paths })
    }
}

impl ChatWidget {
    fn model_description_for(slug: &str) -> Option<&'static str> {
        if slug.starts_with("gpt-5-codex") {
            Some("Optimized for coding tasks with many tools.")
        } else if slug.starts_with("gpt-5") {
            Some("Broad world knowledge with strong general reasoning.")
        } else {
            None
        }
    }

    fn flush_answer_stream_with_separator(&mut self) {
        if let Some(mut controller) = self.stream_controller.take()
            && let Some(cell) = controller.finalize()
        {
            self.add_boxed_history(cell);
        }
    }

    fn set_status_header(&mut self, header: String) {
        if self.current_status_header == header {
            return;
        }
        self.current_status_header = header.clone();
        self.bottom_pane.update_status_header(header);
    }

    fn refresh_footer_subagent(&mut self) {
        let info = self.active_subagent.as_ref().and_then(|agent_id| {
            let state = self.subagent_states.get(agent_id)?;

            // Find the agent number for display
            let agent_ids: Vec<String> = self.agents.keys().cloned().collect();
            let agent_number = agent_ids
                .iter()
                .position(|id| id == agent_id)
                .map(|i| i + 1);
            let total_agents = agent_ids.len();

            let label = self
                .agents
                .get(agent_id)
                .and_then(|meta| {
                    let purpose = meta.purpose.trim();
                    if purpose.is_empty() {
                        None
                    } else {
                        Some(purpose.to_string())
                    }
                })
                .unwrap_or_else(|| agent_id.clone());

            // Add agent number to label if available
            let label_with_number = if let Some(num) = agent_number {
                format!("{num}. {label} (Ctrl+{num})")
            } else {
                format!("{label} ({total_agents})")
            };

            Some(FooterSubagentInfo {
                label: label_with_number,
                status: state.footer_status(),
                context_percent: state.context_percent(),
                color: state.color(),
            })
        });
        self.bottom_pane.set_active_subagent(info);
    }

    // --- Small event handlers ---
    fn on_session_configured(&mut self, event: codex_core::protocol::SessionConfiguredEvent) {
        self.bottom_pane
            .set_history_metadata(event.history_log_id, event.history_entry_count);
        self.conversation_id = Some(event.session_id);
        let initial_messages = event.initial_messages.clone();
        let model_for_header = event.model.clone();
        self.session_header.set_model(&model_for_header);
        self.add_to_history(history_cell::new_session_info(
            &self.config,
            event,
            self.show_welcome_banner,
        ));
        if let Some(messages) = initial_messages {
            self.replay_initial_messages(messages);
        }
        // Ask codex-core to enumerate custom prompts for this session.
        self.submit_op(Op::ListCustomPrompts);
        if let Some(user_message) = self.initial_user_message.take() {
            self.submit_user_message(user_message);
        }
        if !self.suppress_session_configured_redraw {
            self.request_redraw();
        }
    }

    fn on_agent_message(&mut self, message: String) {
        // If we have a stream_controller, then the final agent message is redundant and will be a
        // duplicate of what has already been streamed.
        if self.stream_controller.is_none() {
            self.handle_streaming_delta(message);
        }
        self.flush_answer_stream_with_separator();
        self.handle_stream_finished();
        self.request_redraw();
    }

    fn on_agent_message_delta(&mut self, delta: String) {
        self.handle_streaming_delta(delta);
    }

    fn on_agent_reasoning_delta(&mut self, delta: String) {
        // For reasoning deltas, do not stream to history. Accumulate the
        // current reasoning block and extract the first bold element
        // (between **/**) as the chunk header. Show this header as status.
        self.reasoning_buffer.push_str(&delta);

        if let Some(header) = extract_first_bold(&self.reasoning_buffer) {
            // Update the shimmer header to the extracted reasoning chunk header.
            self.set_status_header(header);
        } else {
            // Fallback while we don't yet have a bold header: leave existing header as-is.
        }
        self.request_redraw();
    }

    fn on_agent_reasoning_final(&mut self) {
        // At the end of a reasoning block, record transcript-only content.
        self.full_reasoning_buffer.push_str(&self.reasoning_buffer);
        // Only create reasoning cell if there's actual content (not just whitespace)
        if !self.full_reasoning_buffer.trim().is_empty() {
            let cell = history_cell::new_reasoning_summary_block(
                self.full_reasoning_buffer.clone(),
                &self.config,
            );
            self.add_boxed_history(cell);
        }
        self.reasoning_buffer.clear();
        self.full_reasoning_buffer.clear();
        self.request_redraw();
    }

    fn on_reasoning_section_break(&mut self) {
        // Start a new reasoning block for header extraction and accumulate transcript.
        // Skip if reasoning buffer is empty or contains only whitespace
        if self.reasoning_buffer.trim().is_empty() {
            self.reasoning_buffer.clear();
            return;
        }

        self.full_reasoning_buffer.push_str(&self.reasoning_buffer);
        self.full_reasoning_buffer.push_str("\n\n");
        self.reasoning_buffer.clear();
    }

    // Raw reasoning uses the same flow as summarized reasoning

    fn on_task_started(&mut self) {
        self.bottom_pane.clear_ctrl_c_quit_hint();
        self.bottom_pane.set_task_running(true);
        self.retry_status_header = None;
        self.set_status_header(String::from("Working"));
        self.full_reasoning_buffer.clear();
        self.reasoning_buffer.clear();
        self.request_redraw();
    }

    fn on_task_complete(&mut self, last_agent_message: Option<String>) {
        // If a stream is currently active, finalize it.
        self.flush_answer_stream_with_separator();
        // Mark task stopped and request redraw now that all content is in history.
        self.bottom_pane.set_task_running(false);
        self.running_commands.clear();
        self.request_redraw();

        // If there is a queued user message, send exactly one now to begin the next turn.
        self.maybe_send_next_queued_input();
        // Emit a notification when the turn completes (suppressed if focused).
        self.notify(Notification::AgentTurnComplete {
            response: last_agent_message.unwrap_or_default(),
        });
    }

    pub(crate) fn set_token_info(&mut self, info: Option<TokenUsageInfo>) {
        if let Some(info) = info {
            let context_window = info
                .model_context_window
                .or(self.config.model_context_window);
            let percent = context_window.map(|window| {
                info.last_token_usage
                    .percent_of_context_window_remaining(window)
            });
            self.bottom_pane.set_context_window_percent(percent);

            // Pass detailed token information to footer
            let used_tokens = Some(info.last_token_usage.tokens_in_context_window());
            let max_tokens = context_window;
            let total_session_tokens = Some(info.total_token_usage.total_tokens);
            self.bottom_pane
                .set_context_tokens(used_tokens, max_tokens, total_session_tokens);

            self.token_info = Some(info);
        } else {
            // Fallback: Show config values even when model doesn't send token info
            self.update_token_display();
        }
    }

    /// Update token display with client-side tracked tokens
    fn update_token_display(&mut self) {
        if let Some(context_window) = self.config.model_context_window {
            let total_tokens = self.session_input_tokens + self.session_output_tokens;
            let percent = if context_window > 0 {
                ((context_window - total_tokens).saturating_mul(100) / context_window) as u8
            } else {
                100
            };

            self.bottom_pane.set_context_window_percent(Some(percent));
            self.bottom_pane.set_context_tokens(
                Some(total_tokens),
                Some(context_window),
                Some(self.session_input_tokens + self.session_output_tokens),
            );
        }
    }

    fn on_rate_limit_snapshot(&mut self, snapshot: Option<RateLimitSnapshot>) {
        if let Some(snapshot) = snapshot {
            let warnings = self.rate_limit_warnings.take_warnings(
                snapshot
                    .secondary
                    .as_ref()
                    .map(|window| window.used_percent),
                snapshot
                    .secondary
                    .as_ref()
                    .and_then(|window| window.window_minutes),
                snapshot.primary.as_ref().map(|window| window.used_percent),
                snapshot
                    .primary
                    .as_ref()
                    .and_then(|window| window.window_minutes),
            );

            let display = crate::status::rate_limit_snapshot_display(&snapshot, Local::now());
            self.rate_limit_snapshot = Some(display);

            if !warnings.is_empty() {
                for warning in warnings {
                    self.add_to_history(history_cell::new_warning_event(warning));
                }
                self.request_redraw();
            }
        } else {
            self.rate_limit_snapshot = None;
        }
    }
    /// Finalize any active exec as failed and stop/clear running UI state.
    fn finalize_turn(&mut self) {
        // Ensure any spinner is replaced by a red ✗ and flushed into history.
        self.finalize_active_cell_as_failed();
        // Reset running state and clear streaming buffers.
        self.bottom_pane.set_task_running(false);
        self.running_commands.clear();
        self.stream_controller = None;
    }

    fn on_error(&mut self, message: String) {
        self.finalize_turn();
        self.add_to_history(history_cell::new_error_event(message));
        self.request_redraw();

        // After an error ends the turn, try sending the next queued input.
        self.maybe_send_next_queued_input();
    }

    /// Handle a turn aborted due to user interrupt (Esc).
    /// When there are queued user messages, restore them into the composer
    /// separated by newlines rather than auto‑submitting the next one.
    fn on_interrupted_turn(&mut self, reason: TurnAbortReason) {
        // Finalize, log a gentle prompt, and clear running state.
        self.finalize_turn();

        if reason != TurnAbortReason::ReviewEnded {
            self.add_to_history(history_cell::new_error_event(
                "Conversation interrupted - tell the model what to do differently".to_owned(),
            ));
        }

        // If any messages were queued during the task, restore them into the composer.
        if !self.queued_user_messages.is_empty() {
            let queued_text = self
                .queued_user_messages
                .iter()
                .map(|m| m.text.clone())
                .collect::<Vec<_>>()
                .join("\n");
            let existing_text = self.bottom_pane.composer_text();
            let combined = if existing_text.is_empty() {
                queued_text
            } else if queued_text.is_empty() {
                existing_text
            } else {
                format!("{queued_text}\n{existing_text}")
            };
            self.bottom_pane.set_composer_text(combined);
            // Clear the queue and update the status indicator list.
            self.queued_user_messages.clear();
            self.refresh_queued_user_messages();
        }

        self.request_redraw();
    }

    fn on_plan_update(&mut self, update: UpdatePlanArgs) {
        self.add_to_history(history_cell::new_plan_update(update));
    }

    fn on_exec_approval_request(&mut self, id: String, ev: ExecApprovalRequestEvent) {
        let id2 = id.clone();
        let ev2 = ev.clone();
        self.defer_or_handle(
            |q| q.push_exec_approval(id, ev),
            |s| s.handle_exec_approval_now(id2, ev2),
        );
    }

    fn on_apply_patch_approval_request(&mut self, id: String, ev: ApplyPatchApprovalRequestEvent) {
        let id2 = id.clone();
        let ev2 = ev.clone();
        self.defer_or_handle(
            |q| q.push_apply_patch_approval(id, ev),
            |s| s.handle_apply_patch_approval_now(id2, ev2),
        );
    }

    fn on_exec_command_begin(&mut self, ev: ExecCommandBeginEvent) {
        self.flush_answer_stream_with_separator();
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_exec_begin(ev), |s| s.handle_exec_begin_now(ev2));
    }

    fn on_exec_command_output_delta(
        &mut self,
        _ev: codex_core::protocol::ExecCommandOutputDeltaEvent,
    ) {
        // TODO: Handle streaming exec output if/when implemented
    }

    fn on_patch_apply_begin(&mut self, event: PatchApplyBeginEvent) {
        self.add_to_history(history_cell::new_patch_event(
            event.changes,
            &self.config.cwd,
        ));
    }

    fn on_view_image_tool_call(&mut self, event: ViewImageToolCallEvent) {
        self.flush_answer_stream_with_separator();
        self.add_to_history(history_cell::new_view_image_tool_call(
            event.path,
            &self.config.cwd,
        ));
        self.request_redraw();
    }

    fn on_patch_apply_end(&mut self, event: codex_core::protocol::PatchApplyEndEvent) {
        let ev2 = event.clone();
        self.defer_or_handle(
            |q| q.push_patch_end(event),
            |s| s.handle_patch_apply_end_now(ev2),
        );
    }

    fn on_exec_command_end(&mut self, ev: ExecCommandEndEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_exec_end(ev), |s| s.handle_exec_end_now(ev2));
    }

    fn on_mcp_tool_call_begin(&mut self, ev: McpToolCallBeginEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_mcp_begin(ev), |s| s.handle_mcp_begin_now(ev2));
    }

    fn on_mcp_tool_call_end(&mut self, ev: McpToolCallEndEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_mcp_end(ev), |s| s.handle_mcp_end_now(ev2));
    }

    fn on_web_search_begin(&mut self, _ev: WebSearchBeginEvent) {
        self.flush_answer_stream_with_separator();
    }

    fn on_web_search_end(&mut self, ev: WebSearchEndEvent) {
        self.flush_answer_stream_with_separator();
        self.add_to_history(history_cell::new_web_search_call(format!(
            "Searched: {}",
            ev.query
        )));
    }

    fn on_get_history_entry_response(
        &mut self,
        event: codex_core::protocol::GetHistoryEntryResponseEvent,
    ) {
        let codex_core::protocol::GetHistoryEntryResponseEvent {
            offset,
            log_id,
            entry,
        } = event;
        self.bottom_pane
            .on_history_entry_response(log_id, offset, entry.map(|e| e.text));
    }

    fn on_shutdown_complete(&mut self) {
        self.app_event_tx.send(AppEvent::ExitRequest);
    }

    fn on_turn_diff(&mut self, unified_diff: String) {
        debug!("TurnDiffEvent: {unified_diff}");
    }

    fn on_background_event(&mut self, message: String) {
        debug!("BackgroundEvent: {message}");
    }

    fn on_stream_error(&mut self, message: String) {
        if self.retry_status_header.is_none() {
            self.retry_status_header = Some(self.current_status_header.clone());
        }
        self.set_status_header(message);
    }

    fn on_agent_event(&mut self, agent_event: AgentEvent) {
        let agent_id = agent_event.agent_id.clone();
        let event = *agent_event.event;

        // Assign different colors to different agents for better visual distinction
        let agent_colors = [Color::Cyan,
            Color::Green,
            Color::Yellow,
            Color::Blue,
            Color::Magenta,
            Color::Red,
            Color::White,
            Color::LightGreen,
            Color::LightBlue];

        // Find consistent color assignment based on agent order
        let agent_ids: Vec<String> = self.agents.keys().cloned().collect();
        let agent_index = agent_ids
            .iter()
            .position(|id| id == &agent_id)
            .unwrap_or(self.subagent_states.len());
        let color = agent_colors
            .get(agent_index % agent_colors.len())
            .copied()
            .unwrap_or(Color::Magenta);

        let state = self
            .subagent_states
            .entry(agent_id.clone())
            .or_insert_with(|| SubAgentState::new(color));
        let outputs = state.handle_event(event, &self.config);
        // We intentionally do not add intermediate subagent output cells
        // to the transcript to keep the UI compact; the footer shows a
        // concise, colored status bar for the active subagent.
        self.active_subagent = Some(agent_id);
        self.refresh_footer_subagent();
        self.request_redraw();
    }

    /// Periodic tick to commit at most one queued line to history with a small delay,
    /// animating the output.
    pub(crate) fn on_commit_tick(&mut self) {
        if let Some(controller) = self.stream_controller.as_mut() {
            let (cell, is_idle) = controller.on_commit_tick();
            if let Some(cell) = cell {
                self.bottom_pane.hide_status_indicator();
                self.add_boxed_history(cell);
            }
            if is_idle {
                self.app_event_tx.send(AppEvent::StopCommitAnimation);
            }
        }
    }

    fn flush_interrupt_queue(&mut self) {
        let mut mgr = std::mem::take(&mut self.interrupts);
        mgr.flush_all(self);
        self.interrupts = mgr;
    }

    #[inline]
    fn defer_or_handle(
        &mut self,
        push: impl FnOnce(&mut InterruptManager),
        handle: impl FnOnce(&mut Self),
    ) {
        // Preserve deterministic FIFO across queued interrupts: once anything
        // is queued due to an active write cycle, continue queueing until the
        // queue is flushed to avoid reordering (e.g., ExecEnd before ExecBegin).
        if self.stream_controller.is_some() || !self.interrupts.is_empty() {
            push(&mut self.interrupts);
        } else {
            handle(self);
        }
    }

    fn handle_stream_finished(&mut self) {
        if self.task_complete_pending {
            self.bottom_pane.hide_status_indicator();
            self.task_complete_pending = false;
        }
        // A completed stream indicates non-exec content was just inserted.
        self.flush_interrupt_queue();
    }

    #[inline]
    fn handle_streaming_delta(&mut self, delta: String) {
        // Before streaming agent content, flush any active exec cell group.
        self.flush_active_cell();

        // Track output tokens from streaming response
        let output_tokens = crate::token_counter::count_tokens(&delta, &self.config.model) as u64;
        self.session_output_tokens += output_tokens;

        // Update token display with new output tokens
        self.update_token_display();

        if self.stream_controller.is_none() {
            if self.needs_final_message_separator {
                let elapsed_seconds = self
                    .bottom_pane
                    .status_widget()
                    .map(super::status_indicator_widget::StatusIndicatorWidget::elapsed_seconds);
                self.add_to_history(history_cell::FinalMessageSeparator::new(elapsed_seconds));
                self.needs_final_message_separator = false;
            }
            self.stream_controller = Some(StreamController::new(
                self.config.clone(),
                self.last_rendered_width.get().map(|w| w.saturating_sub(2)),
            ));
        }
        if let Some(controller) = self.stream_controller.as_mut()
            && controller.push(&delta)
        {
            self.app_event_tx.send(AppEvent::StartCommitAnimation);
        }
        self.request_redraw();
    }

    pub(crate) fn handle_exec_end_now(&mut self, ev: ExecCommandEndEvent) {
        let running = self.running_commands.remove(&ev.call_id);
        let (command, parsed) = match running {
            Some(rc) => (rc.command, rc.parsed_cmd),
            None => (vec![ev.call_id.clone()], Vec::new()),
        };

        let needs_new = self
            .active_cell
            .as_ref()
            .map(|cell| cell.as_any().downcast_ref::<ExecCell>().is_none())
            .unwrap_or(true);
        if needs_new {
            self.flush_active_cell();
            self.active_cell = Some(Box::new(new_active_exec_command(
                ev.call_id.clone(),
                command,
                parsed,
            )));
        }

        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<ExecCell>())
        {
            cell.complete_call(
                &ev.call_id,
                CommandOutput {
                    exit_code: ev.exit_code,
                    stdout: ev.stdout.clone(),
                    stderr: ev.stderr.clone(),
                    formatted_output: ev.formatted_output.clone(),
                },
                ev.duration,
            );
            if cell.should_flush() {
                self.flush_active_cell();
            }
        }
    }

    pub(crate) fn handle_patch_apply_end_now(
        &mut self,
        event: codex_core::protocol::PatchApplyEndEvent,
    ) {
        // If the patch was successful, just let the "Edited" block stand.
        // Otherwise, add a failure block.
        if !event.success {
            self.add_to_history(history_cell::new_patch_apply_failure(event.stderr));
        }
    }

    pub(crate) fn handle_exec_approval_now(&mut self, id: String, ev: ExecApprovalRequestEvent) {
        // Validate command array - skip if it looks malformed (e.g., from broken thinking block parsing)
        // Check if command has shell operators as separate elements which indicates parsing error
        let has_malformed_operators = ev.command.iter().any(|arg| {
            // Detect shell operators that shouldn't be standalone array elements
            // These indicate the API incorrectly parsed tool calls from thinking blocks
            matches!(
                arg.as_str(),
                ">" | ">>" | "<" | "|" | "&&" | "||" | "2>" | "2>&1"
            )
        });

        if has_malformed_operators {
            tracing::warn!(
                "Skipping malformed exec command with standalone shell operators: {:?}",
                ev.command
            );
            // Add error message to history instead
            self.flush_answer_stream_with_separator();
            self.add_to_history(history_cell::new_error_event(
                "Skipped malformed tool call (likely from thinking block)".to_string(),
            ));
            self.request_redraw();
            return;
        }

        self.flush_answer_stream_with_separator();
        let command = shlex::try_join(ev.command.iter().map(String::as_str))
            .unwrap_or_else(|_| ev.command.join(" "));
        self.notify(Notification::ExecApprovalRequested { command });

        let request = ApprovalRequest::Exec {
            id,
            command: ev.command,
            reason: ev.reason,
        };
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
    }

    pub(crate) fn handle_apply_patch_approval_now(
        &mut self,
        id: String,
        ev: ApplyPatchApprovalRequestEvent,
    ) {
        self.flush_answer_stream_with_separator();

        let request = ApprovalRequest::ApplyPatch {
            id,
            reason: ev.reason,
            changes: ev.changes.clone(),
            cwd: self.config.cwd.clone(),
        };
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
        self.notify(Notification::EditApprovalRequested {
            cwd: self.config.cwd.clone(),
            changes: ev.changes.keys().cloned().collect(),
        });
    }

    pub(crate) fn handle_exec_begin_now(&mut self, ev: ExecCommandBeginEvent) {
        // Ensure the status indicator is visible while the command runs.
        self.running_commands.insert(
            ev.call_id.clone(),
            RunningCommand {
                command: ev.command.clone(),
                parsed_cmd: ev.parsed_cmd.clone(),
            },
        );
        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<ExecCell>())
            && let Some(new_exec) = cell.with_added_call(
                ev.call_id.clone(),
                ev.command.clone(),
                ev.parsed_cmd.clone(),
            )
        {
            *cell = new_exec;
        } else {
            self.flush_active_cell();

            self.active_cell = Some(Box::new(new_active_exec_command(
                ev.call_id.clone(),
                ev.command.clone(),
                ev.parsed_cmd,
            )));
        }

        self.request_redraw();
    }

    pub(crate) fn handle_mcp_begin_now(&mut self, ev: McpToolCallBeginEvent) {
        self.flush_answer_stream_with_separator();
        self.flush_active_cell();
        self.active_cell = Some(Box::new(history_cell::new_active_mcp_tool_call(
            ev.call_id,
            ev.invocation,
        )));
        self.request_redraw();
    }
    pub(crate) fn handle_mcp_end_now(&mut self, ev: McpToolCallEndEvent) {
        self.flush_answer_stream_with_separator();

        let McpToolCallEndEvent {
            call_id,
            invocation,
            duration,
            result,
        } = ev;

        let extra_cell = match self
            .active_cell
            .as_mut()
            .and_then(|cell| cell.as_any_mut().downcast_mut::<McpToolCallCell>())
        {
            Some(cell) if cell.call_id() == call_id => cell.complete(duration, result),
            _ => {
                self.flush_active_cell();
                let mut cell = history_cell::new_active_mcp_tool_call(call_id, invocation);
                let extra_cell = cell.complete(duration, result);
                self.active_cell = Some(Box::new(cell));
                extra_cell
            }
        };

        self.flush_active_cell();
        if let Some(extra) = extra_cell {
            self.add_boxed_history(extra);
        }
    }

    fn layout_areas(&self, area: Rect) -> [Rect; 3] {
        let bottom_min = self.bottom_pane.desired_height(area.width).min(area.height);
        let remaining = area.height.saturating_sub(bottom_min);

        let active_desired = self
            .active_cell
            .as_ref()
            .map_or(0, |c| c.desired_height(area.width) + 1);
        let active_height = active_desired.min(remaining);
        // Note: no header area; remaining is not used beyond computing active height.

        let header_height = 0u16;

        Layout::vertical([
            Constraint::Length(header_height),
            Constraint::Length(active_height),
            Constraint::Min(bottom_min),
        ])
        .areas(area)
    }

    pub(crate) fn new(
        common: ChatWidgetInit,
        conversation_manager: Arc<ConversationManager>,
    ) -> Self {
        let ChatWidgetInit {
            config,
            frame_requester,
            app_event_tx,
            initial_prompt,
            initial_images,
            enhanced_keys_supported,
            auth_manager,
        } = common;
        let mut rng = rand::rng();
        let placeholder = EXAMPLE_PROMPTS[rng.random_range(0..EXAMPLE_PROMPTS.len())].to_string();
        let codex_op_tx = spawn_agent(config.clone(), app_event_tx.clone(), conversation_manager);

        let mut instance = Self {
            app_event_tx: app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            codex_op_tx,
            bottom_pane: BottomPane::new(BottomPaneParams {
                frame_requester,
                app_event_tx,
                has_input_focus: true,
                enhanced_keys_supported,
                placeholder_text: placeholder,
                disable_paste_burst: config.disable_paste_burst,
            }),
            active_cell: None,
            config: config.clone(),
            auth_manager,
            session_header: SessionHeader::new(config.model),
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            token_info: None,
            rate_limit_snapshot: None,
            rate_limit_warnings: RateLimitWarningState::default(),
            stream_controller: None,
            running_commands: HashMap::new(),
            task_complete_pending: false,
            interrupts: InterruptManager::new(),
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: String::from("Working"),
            retry_status_header: None,
            conversation_id: None,
            queued_user_messages: VecDeque::new(),
            show_welcome_banner: true,
            suppress_session_configured_redraw: false,
            pending_notification: None,
            is_review_mode: false,
            ghost_snapshots: Vec::new(),
            ghost_snapshots_disabled: true,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            session_input_tokens: 0,
            session_output_tokens: 0,
            current_turn_input_tokens: 0,
            agents: HashMap::new(),
            subagent_states: HashMap::new(),
            active_subagent: None,
        };

        // Initialize token display with config values
        instance.set_token_info(None);
        // Initialize footer model label
        instance
            .bottom_pane
            .set_current_model(instance.config.model.clone());
        instance
    }

    /// Create a ChatWidget attached to an existing conversation (e.g., a fork).
    pub(crate) fn new_from_existing(
        common: ChatWidgetInit,
        conversation: std::sync::Arc<codex_core::CodexConversation>,
        session_configured: codex_core::protocol::SessionConfiguredEvent,
    ) -> Self {
        let ChatWidgetInit {
            config,
            frame_requester,
            app_event_tx,
            initial_prompt,
            initial_images,
            enhanced_keys_supported,
            auth_manager,
        } = common;
        let mut rng = rand::rng();
        let placeholder = EXAMPLE_PROMPTS[rng.random_range(0..EXAMPLE_PROMPTS.len())].to_string();

        let codex_op_tx =
            spawn_agent_from_existing(conversation, session_configured, app_event_tx.clone());

        let mut instance = Self {
            app_event_tx: app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            codex_op_tx,
            bottom_pane: BottomPane::new(BottomPaneParams {
                frame_requester,
                app_event_tx,
                has_input_focus: true,
                enhanced_keys_supported,
                placeholder_text: placeholder,
                disable_paste_burst: config.disable_paste_burst,
            }),
            active_cell: None,
            config: config.clone(),
            auth_manager,
            session_header: SessionHeader::new(config.model),
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            token_info: None,
            rate_limit_snapshot: None,
            rate_limit_warnings: RateLimitWarningState::default(),
            stream_controller: None,
            running_commands: HashMap::new(),
            task_complete_pending: false,
            interrupts: InterruptManager::new(),
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: String::from("Working"),
            retry_status_header: None,
            conversation_id: None,
            queued_user_messages: VecDeque::new(),
            show_welcome_banner: true,
            suppress_session_configured_redraw: true,
            pending_notification: None,
            is_review_mode: false,
            ghost_snapshots: Vec::new(),
            ghost_snapshots_disabled: true,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            session_input_tokens: 0,
            session_output_tokens: 0,
            current_turn_input_tokens: 0,
            agents: HashMap::new(),
            subagent_states: HashMap::new(),
            active_subagent: None,
        };

        // Initialize footer model label
        instance
            .bottom_pane
            .set_current_model(instance.config.model.clone());
        instance
    }

    pub fn desired_height(&self, width: u16) -> u16 {
        self.bottom_pane.desired_height(width)
            + self
                .active_cell
                .as_ref()
                .map_or(0, |c| c.desired_height(width) + 1)
    }

    pub(crate) fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event {
            KeyEvent {
                code: KeyCode::Char('a'),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::ALT) => {
                // Show a compact overview of all agents.
                self.render_agents_overview_card();
                return;
            }
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) && c.eq_ignore_ascii_case(&'c') => {
                self.on_ctrl_c();
                return;
            }
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) && c.eq_ignore_ascii_case(&'v') => {
                if let Ok((path, info)) = paste_image_to_temp_png() {
                    self.attach_image(path, info.width, info.height, info.encoded_format.label());
                }
                return;
            }
            // Quick agent navigation shortcuts - Ctrl+1, Ctrl+2, etc.
            KeyEvent {
                code: KeyCode::Char(c @ '1'..='9'),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) => {
                let agent_index = c.to_digit(10).unwrap() as usize - 1;
                let agent_ids: Vec<String> = self.agents.keys().cloned().collect();
                if let Some(agent_id) = agent_ids.get(agent_index) {
                    // Switch to the selected agent
                    self.active_subagent = Some(agent_id.clone());
                    self.refresh_footer_subagent();
                    self.request_redraw();
                }
                return;
            }
            // Ctrl+0 to return to main agent (no active subagent)
            KeyEvent {
                code: KeyCode::Char('0'),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) => {
                self.active_subagent = None;
                self.refresh_footer_subagent();
                self.request_redraw();
                return;
            }
            // Ctrl+H to show agent navigation help
            KeyEvent {
                code: KeyCode::Char('h'),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) => {
                self.add_agent_navigation_info();
                return;
            }
            KeyEvent {
                code: KeyCode::Esc,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                ..
            } if self.active_subagent.is_some() && self.is_normal_backtrack_mode() => {
                if let Some(agent_id) = self.active_subagent.clone() {
                    self.submit_op(Op::InterruptAgent { agent_id });
                }
                return;
            }
            other if other.kind == KeyEventKind::Press => {
                self.bottom_pane.clear_ctrl_c_quit_hint();
            }
            _ => {}
        }

        match key_event {
            KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::ALT,
                kind: KeyEventKind::Press,
                ..
            } if !self.queued_user_messages.is_empty() => {
                // Prefer the most recently queued item.
                if let Some(user_message) = self.queued_user_messages.pop_back() {
                    self.bottom_pane.set_composer_text(user_message.text);
                    self.refresh_queued_user_messages();
                    self.request_redraw();
                }
            }
            _ => {
                match self.bottom_pane.handle_key_event(key_event) {
                    InputResult::Submitted(text) => {
                        // If a task is running, queue the user input to be sent after the turn completes.
                        let user_message = UserMessage {
                            text,
                            image_paths: self.bottom_pane.take_recent_submission_images(),
                        };
                        if self.bottom_pane.is_task_running() {
                            self.queued_user_messages.push_back(user_message);
                            self.refresh_queued_user_messages();
                        } else {
                            self.submit_user_message(user_message);
                        }
                    }
                    InputResult::Command(cmd) => {
                        self.dispatch_command(cmd);
                    }
                    InputResult::None => {}
                }
            }
        }
    }

    fn render_agents_overview_card(&mut self) {
        let rows: Vec<history_cell::AgentOverviewEntry> = self
            .agents
            .iter()
            .map(|(id, meta)| {
                (
                    id.clone(),
                    meta.profile.clone(),
                    meta.purpose.clone(),
                    meta.status,
                    meta.last_summary.clone(),
                    meta.context_percent,
                )
            })
            .collect();
        let cell = history_cell::new_agents_overview(rows);
        self.add_to_history(cell);
        self.request_redraw();
    }

    fn open_agents_popup(&mut self) {
        let mut items: Vec<SelectionItem> = Vec::new();
        for (id, meta) in self.agents.iter() {
            let mut name = id.clone();
            if let Some(p) = &meta.profile {
                name.push_str(&format!(" [{p}]"));
            }
            let status = match meta.status {
                None => "in progress",
                Some(true) => "completed",
                Some(false) => "failed",
            };
            let description = Some(format!("{} — {}", meta.purpose, status));
            let id_for_action = id.clone();
            let profile_for_action = meta.profile.clone();
            let purpose_for_action = meta.purpose.clone();
            let status_for_action = meta.status;
            let summary_for_action = meta.last_summary.clone();
            let context_for_action = meta.context_percent;
            let transcript_for_action = meta.transcript.clone();
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                let view = history_cell::AgentCardView {
                    agent_id: &id_for_action,
                    profile: profile_for_action.as_deref(),
                    purpose: &purpose_for_action,
                    status: status_for_action,
                    summary: summary_for_action.as_deref(),
                    context_percent: context_for_action,
                    transcript: &transcript_for_action,
                };
                let cell = history_cell::new_agent_detail_card(view);
                tx.send(AppEvent::InsertHistoryCell(Box::new(cell)));
            })];
            items.push(SelectionItem {
                name,
                description,
                is_current: false,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        if items.is_empty() {
            // If no agents are tracked yet, just show a message card.
            self.add_to_history(history_cell::new_warning_event(
                "No spawned agents yet in this session.".to_string(),
            ));
            self.request_redraw();
            return;
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Agents".to_string()),
            subtitle: Some("Use ↑/↓ to navigate, Enter to show details".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn attach_image(
        &mut self,
        path: PathBuf,
        width: u32,
        height: u32,
        format_label: &str,
    ) {
        tracing::info!(
            "attach_image path={path:?} width={width} height={height} format={format_label}",
        );
        self.bottom_pane
            .attach_image(path, width, height, format_label);
        self.request_redraw();
    }

    fn dispatch_command(&mut self, cmd: SlashCommand) {
        if !cmd.available_during_task() && self.bottom_pane.is_task_running() {
            let message = format!(
                "'/{}' is disabled while a task is in progress.",
                cmd.command()
            );
            self.add_to_history(history_cell::new_error_event(message));
            self.request_redraw();
            return;
        }
        match cmd {
            SlashCommand::New => {
                self.app_event_tx.send(AppEvent::NewSession);
            }
            SlashCommand::Init => {
                const INIT_PROMPT: &str = include_str!("../prompt_for_init_command.md");
                self.submit_text_message(INIT_PROMPT.to_string());
            }
            SlashCommand::Compact => {
                self.clear_token_usage();
                self.app_event_tx.send(AppEvent::CodexOp(Op::Compact));
            }
            SlashCommand::Review => {
                self.open_review_popup();
            }
            SlashCommand::Model => {
                self.open_model_popup();
            }
            SlashCommand::Approvals => {
                self.open_approvals_popup();
            }
            SlashCommand::Quit => {
                self.app_event_tx.send(AppEvent::ExitRequest);
            }
            SlashCommand::Logout => {
                if let Err(e) = codex_core::auth::logout(&self.config.codex_home) {
                    tracing::error!("failed to logout: {e}");
                }
                self.app_event_tx.send(AppEvent::ExitRequest);
            }
            SlashCommand::Undo => {
                self.undo_last_snapshot();
            }
            SlashCommand::Diff => {
                self.add_diff_in_progress();
                let tx = self.app_event_tx.clone();
                tokio::spawn(async move {
                    let text = match get_git_diff().await {
                        Ok((is_git_repo, diff_text)) => {
                            if is_git_repo {
                                diff_text
                            } else {
                                "`/diff` — _not inside a git repository_".to_string()
                            }
                        }
                        Err(e) => format!("Failed to compute diff: {e}"),
                    };
                    tx.send(AppEvent::DiffResult(text));
                });
            }
            SlashCommand::Mention => {
                self.insert_str("@");
            }
            SlashCommand::Status => {
                self.add_status_output();
            }
            SlashCommand::Mcp => {
                self.add_mcp_output();
            }
            // Codex-Local custom commands
            SlashCommand::Config => {
                self.add_config_output();
            }
            SlashCommand::Context => {
                self.add_context_info();
            }
            SlashCommand::Tokens => {
                self.add_tokens_info();
            }
            SlashCommand::Provider => {
                self.add_provider_info();
            }
            SlashCommand::Models => {
                self.add_models_list();
            }
            SlashCommand::CompactSettings => {
                self.add_compact_settings_info();
            }
            SlashCommand::Think => {
                self.add_think_toggle_info();
            }
            SlashCommand::Orchestrator => {
                self.add_orchestrator_toggle_info();
            }
            SlashCommand::Profiles => {
                self.add_profiles_info();
            }
            SlashCommand::Agents => {
                self.open_agents_popup();
            }
            #[cfg(debug_assertions)]
            SlashCommand::TestApproval => {
                use codex_core::protocol::EventMsg;
                use std::collections::HashMap;

                use codex_core::protocol::ApplyPatchApprovalRequestEvent;
                use codex_core::protocol::FileChange;

                self.app_event_tx.send(AppEvent::CodexEvent(Event {
                    id: "1".to_string(),
                    // msg: EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
                    //     call_id: "1".to_string(),
                    //     command: vec!["git".into(), "apply".into()],
                    //     cwd: self.config.cwd.clone(),
                    //     reason: Some("test".to_string()),
                    // }),
                    msg: EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
                        call_id: "1".to_string(),
                        changes: HashMap::from([
                            (
                                PathBuf::from("/tmp/test.txt"),
                                FileChange::Add {
                                    content: "test".to_string(),
                                },
                            ),
                            (
                                PathBuf::from("/tmp/test2.txt"),
                                FileChange::Update {
                                    unified_diff: "+test\n-test2".to_string(),
                                    move_path: None,
                                },
                            ),
                        ]),
                        reason: None,
                        grant_root: Some(PathBuf::from("/tmp")),
                    }),
                }));
            }
        }
    }

    pub(crate) fn handle_paste(&mut self, text: String) {
        self.bottom_pane.handle_paste(text);
    }

    // Returns true if caller should skip rendering this frame (a future frame is scheduled).
    pub(crate) fn handle_paste_burst_tick(&mut self, frame_requester: FrameRequester) -> bool {
        if self.bottom_pane.flush_paste_burst_if_due() {
            // A paste just flushed; request an immediate redraw and skip this frame.
            self.request_redraw();
            true
        } else if self.bottom_pane.is_in_paste_burst() {
            // While capturing a burst, schedule a follow-up tick and skip this frame
            // to avoid redundant renders between ticks.
            frame_requester.schedule_frame_in(
                crate::bottom_pane::ChatComposer::recommended_paste_flush_delay(),
            );
            true
        } else {
            false
        }
    }

    fn flush_active_cell(&mut self) {
        if let Some(active) = self.active_cell.take() {
            self.needs_final_message_separator = true;
            self.app_event_tx.send(AppEvent::InsertHistoryCell(active));
        }
    }

    fn add_to_history(&mut self, cell: impl HistoryCell + 'static) {
        self.add_boxed_history(Box::new(cell));
    }

    fn add_boxed_history(&mut self, cell: Box<dyn HistoryCell>) {
        if !cell.display_lines(u16::MAX).is_empty() {
            // Only break exec grouping if the cell renders visible lines.
            self.flush_active_cell();
            self.needs_final_message_separator = true;
        }
        self.app_event_tx.send(AppEvent::InsertHistoryCell(cell));
    }

    fn submit_user_message(&mut self, user_message: UserMessage) {
        let UserMessage { text, image_paths } = user_message;
        if text.is_empty() && image_paths.is_empty() {
            return;
        }

        self.capture_ghost_snapshot();

        let mut items: Vec<InputItem> = Vec::new();

        if !text.is_empty() {
            items.push(InputItem::Text { text: text.clone() });
        }

        for path in image_paths {
            items.push(InputItem::LocalImage { path });
        }

        self.codex_op_tx
            .send(Op::UserInput { items })
            .unwrap_or_else(|e| {
                tracing::error!("failed to send message: {e}");
            });

        // Persist the text to cross-session message history.
        if !text.is_empty() {
            self.codex_op_tx
                .send(Op::AddToHistory { text: text.clone() })
                .unwrap_or_else(|e| {
                    tracing::error!("failed to send AddHistory op: {e}");
                });
        }

        // Only show the text portion in conversation history.
        if !text.is_empty() {
            self.add_to_history(history_cell::new_user_prompt(text));
        }
        self.needs_final_message_separator = false;
    }

    fn capture_ghost_snapshot(&mut self) {
        if self.ghost_snapshots_disabled {
            return;
        }

        let options = CreateGhostCommitOptions::new(&self.config.cwd);
        match create_ghost_commit(&options) {
            Ok(commit) => {
                self.ghost_snapshots.push(commit);
                if self.ghost_snapshots.len() > MAX_TRACKED_GHOST_COMMITS {
                    self.ghost_snapshots.remove(0);
                }
            }
            Err(err) => {
                self.ghost_snapshots_disabled = true;
                let (message, hint) = match &err {
                    GitToolingError::NotAGitRepository { .. } => (
                        "Snapshots disabled: current directory is not a Git repository."
                            .to_string(),
                        None,
                    ),
                    _ => (
                        format!("Snapshots disabled after error: {err}"),
                        Some(
                            "Restart Codex after resolving the issue to re-enable snapshots."
                                .to_string(),
                        ),
                    ),
                };
                self.add_info_message(message, hint);
                tracing::warn!("failed to create ghost snapshot: {err}");
            }
        }
    }

    fn undo_last_snapshot(&mut self) {
        let Some(commit) = self.ghost_snapshots.pop() else {
            self.add_info_message("No snapshot available to undo.".to_string(), None);
            return;
        };

        if let Err(err) = restore_ghost_commit(&self.config.cwd, &commit) {
            self.add_error_message(format!("Failed to restore snapshot: {err}"));
            self.ghost_snapshots.push(commit);
            return;
        }

        let short_id: String = commit.id().chars().take(8).collect();
        self.add_info_message(format!("Restored workspace to snapshot {short_id}"), None);
    }

    /// Replay a subset of initial events into the UI to seed the transcript when
    /// resuming an existing session. This approximates the live event flow and
    /// is intentionally conservative: only safe-to-replay items are rendered to
    /// avoid triggering side effects. Event ids are passed as `None` to
    /// distinguish replayed events from live ones.
    fn replay_initial_messages(&mut self, events: Vec<EventMsg>) {
        for msg in events {
            if matches!(msg, EventMsg::SessionConfigured(_)) {
                continue;
            }
            // `id: None` indicates a synthetic/fake id coming from replay.
            self.dispatch_event_msg(None, msg, true);
        }
    }

    pub(crate) fn handle_codex_event(&mut self, event: Event) {
        let Event { id, msg } = event;
        self.dispatch_event_msg(Some(id), msg, false);
    }

    /// Dispatch a protocol `EventMsg` to the appropriate handler.
    ///
    /// `id` is `Some` for live events and `None` for replayed events from
    /// `replay_initial_messages()`. Callers should treat `None` as a "fake" id
    /// that must not be used to correlate follow-up actions.
    fn dispatch_event_msg(&mut self, id: Option<String>, msg: EventMsg, from_replay: bool) {
        match msg {
            EventMsg::AgentMessageDelta(_)
            | EventMsg::AgentReasoningDelta(_)
            | EventMsg::ExecCommandOutputDelta(_) => {}
            _ => {
                tracing::trace!("handle_codex_event: {:?}", msg);
            }
        }

        match msg {
            EventMsg::SessionConfigured(e) => self.on_session_configured(e),
            EventMsg::AgentMessage(AgentMessageEvent { message }) => self.on_agent_message(message),
            EventMsg::AgentMessageDelta(AgentMessageDeltaEvent { delta }) => {
                self.on_agent_message_delta(delta)
            }
            EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent { delta })
            | EventMsg::AgentReasoningRawContentDelta(AgentReasoningRawContentDeltaEvent {
                delta,
            }) => self.on_agent_reasoning_delta(delta),
            EventMsg::AgentReasoning(AgentReasoningEvent { .. }) => self.on_agent_reasoning_final(),
            EventMsg::AgentReasoningRawContent(AgentReasoningRawContentEvent { text }) => {
                self.on_agent_reasoning_delta(text);
                self.on_agent_reasoning_final()
            }
            EventMsg::AgentReasoningSectionBreak(_) => self.on_reasoning_section_break(),
            EventMsg::TaskStarted(_) => self.on_task_started(),
            EventMsg::TaskComplete(TaskCompleteEvent { last_agent_message }) => {
                self.on_task_complete(last_agent_message)
            }
            EventMsg::TokenCount(ev) => {
                self.set_token_info(ev.info);
                self.on_rate_limit_snapshot(ev.rate_limits);
            }
            EventMsg::Error(ErrorEvent { message }) => self.on_error(message),
            EventMsg::TurnAborted(ev) => match ev.reason {
                TurnAbortReason::Interrupted => {
                    self.on_interrupted_turn(ev.reason);
                }
                TurnAbortReason::Replaced => {
                    self.on_error("Turn aborted: replaced by a new task".to_owned())
                }
                TurnAbortReason::ReviewEnded => {
                    self.on_interrupted_turn(ev.reason);
                }
            },
            EventMsg::PlanUpdate(update) => self.on_plan_update(update),
            EventMsg::ExecApprovalRequest(ev) => {
                // For replayed events, synthesize an empty id (these should not occur).
                self.on_exec_approval_request(id.unwrap_or_default(), ev)
            }
            EventMsg::ApplyPatchApprovalRequest(ev) => {
                self.on_apply_patch_approval_request(id.unwrap_or_default(), ev)
            }
            EventMsg::ExecCommandBegin(ev) => self.on_exec_command_begin(ev),
            EventMsg::ExecCommandOutputDelta(delta) => self.on_exec_command_output_delta(delta),
            EventMsg::PatchApplyBegin(ev) => self.on_patch_apply_begin(ev),
            EventMsg::PatchApplyEnd(ev) => self.on_patch_apply_end(ev),
            EventMsg::ExecCommandEnd(ev) => self.on_exec_command_end(ev),
            EventMsg::ViewImageToolCall(ev) => self.on_view_image_tool_call(ev),
            EventMsg::McpToolCallBegin(ev) => self.on_mcp_tool_call_begin(ev),
            EventMsg::McpToolCallEnd(ev) => self.on_mcp_tool_call_end(ev),
            EventMsg::WebSearchBegin(ev) => self.on_web_search_begin(ev),
            EventMsg::WebSearchEnd(ev) => self.on_web_search_end(ev),
            EventMsg::GetHistoryEntryResponse(ev) => self.on_get_history_entry_response(ev),
            EventMsg::McpListToolsResponse(ev) => self.on_list_mcp_tools(ev),
            EventMsg::ListCustomPromptsResponse(ev) => self.on_list_custom_prompts(ev),
            EventMsg::AgentEvent(ev) => self.on_agent_event(ev),
            EventMsg::ShutdownComplete => self.on_shutdown_complete(),
            EventMsg::TurnDiff(TurnDiffEvent { unified_diff }) => self.on_turn_diff(unified_diff),
            EventMsg::BackgroundEvent(BackgroundEventEvent { message }) => {
                self.on_background_event(message)
            }
            EventMsg::StreamError(StreamErrorEvent { message }) => self.on_stream_error(message),
            EventMsg::UserMessage(ev) => {
                if from_replay {
                    self.on_user_message_event(ev);
                }
            }
            EventMsg::ConversationPath(ev) => {
                self.app_event_tx
                    .send(crate::app_event::AppEvent::ConversationHistory(ev));
            }
            EventMsg::EnteredReviewMode(review_request) => {
                self.on_entered_review_mode(review_request)
            }
            EventMsg::ExitedReviewMode(review) => self.on_exited_review_mode(review),
            // Orchestrator events
            EventMsg::AgentSpawned(ev) => {
                let agent_id = ev.agent_id.clone();
                self.agents.insert(
                    agent_id.clone(),
                    AgentMeta {
                        profile: ev.profile.clone(),
                        purpose: ev.purpose.clone(),
                        status: None,
                        last_summary: None,
                        transcript: Vec::new(),
                        context_percent: None,
                    },
                );
                let mut header_lines: Vec<Line<'static>> =
                    vec![vec![format!("Agent {agent_id}").bold()].into()];
                if let Some(profile) = ev.profile.clone()
                    && !profile.trim().is_empty()
                {
                    header_lines.push(vec!["profile: ".dim(), profile.into()].into());
                }
                if !ev.purpose.trim().is_empty() {
                    header_lines.push(vec!["purpose: ".dim(), ev.purpose.into()].into());
                }

                let header_cell = PlainHistoryCell::new(header_lines);
                let decorated_header = {
                    let state = self
                        .subagent_states
                        .entry(agent_id.clone())
                        .or_insert_with(|| SubAgentState::new(Color::Magenta));
                    state.decorate_header(SubAgentState::boxed(header_cell))
                };
                self.add_boxed_history(decorated_header);
                self.active_subagent = Some(agent_id);
                self.refresh_footer_subagent();
                self.request_redraw();
            }
            EventMsg::AgentProgress(ev) => {
                let agent_id = ev.agent_id.clone();
                let trimmed = ev.message.trim();

                if let Some(meta) = self.agents.get_mut(&agent_id) {
                    if let Some(rest) = trimmed.strip_prefix("context left: ") {
                        let percent_str = rest.trim().trim_end_matches('%');
                        if let Ok(percent) = percent_str.parse::<u8>() {
                            meta.context_percent = Some(percent);
                        }
                    } else if !trimmed.is_empty() {
                        for line in trimmed.lines() {
                            let line = line.trim_end();
                            if line.is_empty() {
                                continue;
                            }
                            meta.transcript.push(line.to_string());
                        }
                        const MAX_LOG_LINES: usize = 40;
                        if meta.transcript.len() > MAX_LOG_LINES {
                            let overflow = meta.transcript.len() - MAX_LOG_LINES;
                            meta.transcript.drain(0..overflow);
                        }
                    }

                    // Do not append a new progress card for each update to avoid spam.
                    // The footer shows current status + context percent; a single
                    // completion card will be added on AgentCompleted.
                } else {
                    // If we haven't seen this agent yet, fall back to nothing to keep UI clean
                }

                {
                    let state = self
                        .subagent_states
                        .entry(agent_id.clone())
                        .or_insert_with(|| SubAgentState::new(Color::Magenta));
                    if let Some(percent) = self
                        .agents
                        .get(&agent_id)
                        .and_then(|meta| meta.context_percent)
                    {
                        state.set_context_percent(percent);
                    }
                    if !trimmed.is_empty() && !trimmed.starts_with("context left: ") {
                        state.record_status(trimmed);
                    }
                }

                self.active_subagent = Some(agent_id);
                self.refresh_footer_subagent();
                self.request_redraw();
            }
            EventMsg::AgentCompleted(ev) => {
                let agent_id = ev.agent_id.clone();
                if let Some(meta) = self.agents.get_mut(&agent_id) {
                    meta.status = Some(ev.success);
                    meta.last_summary = Some(ev.summary.clone());
                }

                let mut fallback = Some(ev.clone());
                if let Some(mut state) = self.subagent_states.remove(&agent_id) {
                    let mut cells = state.completion_cells(ev.success, ev.summary, &self.config);
                    for cell in cells.drain(..) {
                        self.add_boxed_history(cell);
                    }
                    fallback = None;
                }

                // Always add a single compact completion summary so users
                // see the result without the intermediate noise.
                if let Some(ev) = fallback {
                    self.add_to_history(history_cell::new_agent_completed_event(ev));
                }

                if self
                    .active_subagent
                    .as_ref()
                    .is_some_and(|current| current == &agent_id)
                {
                    self.active_subagent = self.subagent_states.keys().next().cloned();
                }
                self.refresh_footer_subagent();
                self.request_redraw();
            }
            EventMsg::AgentSwitched(_) => {
                // Placeholder: will be handled when multi-agent UI is implemented
            }
        }
    }

    fn on_entered_review_mode(&mut self, review: ReviewRequest) {
        // Enter review mode and emit a concise banner
        self.is_review_mode = true;
        let banner = format!(">> Code review started: {} <<", review.user_facing_hint);
        self.add_to_history(history_cell::new_review_status_line(banner));
        self.request_redraw();
    }

    fn on_exited_review_mode(&mut self, review: ExitedReviewModeEvent) {
        // Leave review mode; if output is present, flush pending stream + show results.
        if let Some(output) = review.review_output {
            self.flush_answer_stream_with_separator();
            self.flush_interrupt_queue();
            self.flush_active_cell();

            if output.findings.is_empty() {
                let explanation = output.overall_explanation.trim().to_string();
                if explanation.is_empty() {
                    tracing::error!("Reviewer failed to output a response.");
                    self.add_to_history(history_cell::new_error_event(
                        "Reviewer failed to output a response.".to_owned(),
                    ));
                } else {
                    // Show explanation when there are no structured findings.
                    let mut rendered: Vec<ratatui::text::Line<'static>> = vec!["".into()];
                    append_markdown(&explanation, None, &mut rendered, &self.config);
                    let body_cell = AgentMessageCell::new(rendered, false);
                    self.app_event_tx
                        .send(AppEvent::InsertHistoryCell(Box::new(body_cell)));
                }
            } else {
                let message_text =
                    codex_core::review_format::format_review_findings_block(&output.findings, None);
                let mut message_lines: Vec<ratatui::text::Line<'static>> = Vec::new();
                append_markdown(&message_text, None, &mut message_lines, &self.config);
                let body_cell = AgentMessageCell::new(message_lines, true);
                self.app_event_tx
                    .send(AppEvent::InsertHistoryCell(Box::new(body_cell)));
            }
        }

        self.is_review_mode = false;
        // Append a finishing banner at the end of this turn.
        self.add_to_history(history_cell::new_review_status_line(
            "<< Code review finished >>".to_string(),
        ));
        self.request_redraw();
    }

    fn on_user_message_event(&mut self, event: UserMessageEvent) {
        match event.kind {
            Some(InputMessageKind::EnvironmentContext)
            | Some(InputMessageKind::UserInstructions) => {
                // Skip XML‑wrapped context blocks in the transcript.
            }
            Some(InputMessageKind::Plain) | None => {
                let message = event.message.trim();
                if !message.is_empty() {
                    self.add_to_history(history_cell::new_user_prompt(message.to_string()));
                }
            }
        }
    }

    fn request_redraw(&mut self) {
        self.frame_requester.schedule_frame();
    }

    fn notify(&mut self, notification: Notification) {
        if !notification.allowed_for(&self.config.tui_notifications) {
            return;
        }
        self.pending_notification = Some(notification);
        self.request_redraw();
    }

    pub(crate) fn maybe_post_pending_notification(&mut self, tui: &mut crate::tui::Tui) {
        if let Some(notif) = self.pending_notification.take() {
            tui.notify(notif.display());
        }
    }

    /// Mark the active cell as failed (✗) and flush it into history.
    fn finalize_active_cell_as_failed(&mut self) {
        if let Some(mut cell) = self.active_cell.take() {
            // Insert finalized cell into history and keep grouping consistent.
            if let Some(exec) = cell.as_any_mut().downcast_mut::<ExecCell>() {
                exec.mark_failed();
            } else if let Some(tool) = cell.as_any_mut().downcast_mut::<McpToolCallCell>() {
                tool.mark_failed();
            }
            self.add_boxed_history(cell);
        }
    }

    // If idle and there are queued inputs, submit exactly one to start the next turn.
    fn maybe_send_next_queued_input(&mut self) {
        if self.bottom_pane.is_task_running() {
            return;
        }
        if let Some(user_message) = self.queued_user_messages.pop_front() {
            self.submit_user_message(user_message);
        }
        // Update the list to reflect the remaining queued messages (if any).
        self.refresh_queued_user_messages();
    }

    /// Rebuild and update the queued user messages from the current queue.
    fn refresh_queued_user_messages(&mut self) {
        let messages: Vec<String> = self
            .queued_user_messages
            .iter()
            .map(|m| m.text.clone())
            .collect();
        self.bottom_pane.set_queued_user_messages(messages);
    }

    pub(crate) fn add_diff_in_progress(&mut self) {
        self.request_redraw();
    }

    pub(crate) fn on_diff_complete(&mut self) {
        self.request_redraw();
    }

    pub(crate) fn add_status_output(&mut self) {
        let default_usage = TokenUsage::default();
        let (total_usage, context_usage) = if let Some(ti) = &self.token_info {
            (&ti.total_token_usage, Some(&ti.last_token_usage))
        } else {
            (&default_usage, Some(&default_usage))
        };
        self.add_to_history(crate::status::new_status_output(
            &self.config,
            total_usage,
            context_usage,
            &self.conversation_id,
            self.rate_limit_snapshot.as_ref(),
        ));
    }

    /// Open a popup to choose the model (stage 1). After selecting a model,
    /// a second popup is shown to choose the reasoning effort.
    pub(crate) fn open_model_popup(&mut self) {
        let current_model = self.config.model.clone();
        let auth_mode = self.auth_manager.auth().map(|auth| auth.mode);
        let presets: Vec<ModelPreset> = builtin_model_presets(auth_mode);

        let mut grouped: Vec<(&str, Vec<ModelPreset>)> = Vec::new();
        for preset in presets.into_iter() {
            if let Some((_, entries)) = grouped.iter_mut().find(|(model, _)| *model == preset.model)
            {
                entries.push(preset);
            } else {
                grouped.push((preset.model, vec![preset]));
            }
        }

        let mut items: Vec<SelectionItem> = Vec::new();
        for (model_slug, entries) in grouped.into_iter() {
            let name = model_slug.to_string();
            let description = Self::model_description_for(model_slug)
                .map(std::string::ToString::to_string)
                .or_else(|| {
                    entries
                        .iter()
                        .find(|preset| !preset.description.is_empty())
                        .map(|preset| preset.description.to_string())
                })
                .or_else(|| entries.first().map(|preset| preset.description.to_string()));
            let is_current = model_slug == current_model;
            let model_slug_string = model_slug.to_string();
            let presets_for_model = entries.clone();
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::OpenReasoningPopup {
                    model: model_slug_string.clone(),
                    presets: presets_for_model.clone(),
                });
            })];
            items.push(SelectionItem {
                name,
                description,
                is_current,
                actions,
                dismiss_on_select: false,
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select Model and Effort".to_string()),
            subtitle: Some("Switch the model for this and future Codex CLI sessions".to_string()),
            footer_hint: Some("Press enter to select reasoning effort, or esc to dismiss.".into()),
            items,
            ..Default::default()
        });
    }

    /// Open a popup to choose the reasoning effort (stage 2) for the given model.
    pub(crate) fn open_reasoning_popup(&mut self, model_slug: String, presets: Vec<ModelPreset>) {
        let default_effort = ReasoningEffortConfig::default();

        let has_none_choice = presets.iter().any(|preset| preset.effort.is_none());
        struct EffortChoice {
            stored: Option<ReasoningEffortConfig>,
            display: ReasoningEffortConfig,
        }
        let mut choices: Vec<EffortChoice> = Vec::new();
        for effort in ReasoningEffortConfig::iter() {
            if presets.iter().any(|preset| preset.effort == Some(effort)) {
                choices.push(EffortChoice {
                    stored: Some(effort),
                    display: effort,
                });
            }
            if has_none_choice && default_effort == effort {
                choices.push(EffortChoice {
                    stored: None,
                    display: effort,
                });
            }
        }
        if choices.is_empty() {
            choices.push(EffortChoice {
                stored: Some(default_effort),
                display: default_effort,
            });
        }

        let default_choice: Option<ReasoningEffortConfig> = if has_none_choice {
            None
        } else if choices
            .iter()
            .any(|choice| choice.stored == Some(default_effort))
        {
            Some(default_effort)
        } else {
            choices
                .iter()
                .find_map(|choice| choice.stored)
                .or(Some(default_effort))
        };

        let is_current_model = self.config.model == model_slug;
        let highlight_choice = if is_current_model {
            self.config.model_reasoning_effort
        } else {
            default_choice
        };

        let mut items: Vec<SelectionItem> = Vec::new();
        for choice in choices.iter() {
            let effort = choice.display;
            let mut effort_label = effort.to_string();
            if let Some(first) = effort_label.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            if choice.stored == default_choice {
                effort_label.push_str(" (default)");
            }

            let description = presets
                .iter()
                .find(|preset| preset.effort == choice.stored && !preset.description.is_empty())
                .map(|preset| preset.description.to_string())
                .or_else(|| {
                    presets
                        .iter()
                        .find(|preset| preset.effort == choice.stored)
                        .map(|preset| preset.description.to_string())
                });

            let model_for_action = model_slug.clone();
            let effort_for_action = choice.stored;
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
                    cwd: None,
                    approval_policy: None,
                    sandbox_policy: None,
                    model: Some(model_for_action.clone()),
                    effort: Some(effort_for_action),
                    summary: None,
                }));
                tx.send(AppEvent::UpdateModel(model_for_action.clone()));
                tx.send(AppEvent::UpdateReasoningEffort(effort_for_action));
                tx.send(AppEvent::PersistModelSelection {
                    model: model_for_action.clone(),
                    effort: effort_for_action,
                });
                tracing::info!(
                    "Selected model: {}, Selected effort: {}",
                    model_for_action,
                    effort_for_action
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "default".to_string())
                );
            })];

            items.push(SelectionItem {
                name: effort_label,
                description,
                is_current: is_current_model && choice.stored == highlight_choice,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select Reasoning Level".to_string()),
            subtitle: Some(format!("Reasoning for model {model_slug}")),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    /// Open a popup to choose the approvals mode (ask for approval policy + sandbox policy).
    pub(crate) fn open_approvals_popup(&mut self) {
        let current_approval = self.config.approval_policy;
        let current_sandbox = self.config.sandbox_policy.clone();
        let mut items: Vec<SelectionItem> = Vec::new();
        let presets: Vec<ApprovalPreset> = builtin_approval_presets();
        for preset in presets.into_iter() {
            let is_current =
                current_approval == preset.approval && current_sandbox == preset.sandbox;
            let approval = preset.approval;
            let sandbox = preset.sandbox.clone();
            let name = preset.label.to_string();
            let description = Some(preset.description.to_string());
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
                    cwd: None,
                    approval_policy: Some(approval),
                    sandbox_policy: Some(sandbox.clone()),
                    model: None,
                    effort: None,
                    summary: None,
                }));
                tx.send(AppEvent::UpdateAskForApprovalPolicy(approval));
                tx.send(AppEvent::UpdateSandboxPolicy(sandbox.clone()));
            })];
            items.push(SelectionItem {
                name,
                description,
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select Approval Mode".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    /// Set the approval policy in the widget's config copy.
    pub(crate) fn set_approval_policy(&mut self, policy: AskForApproval) {
        self.config.approval_policy = policy;
    }

    /// Set the sandbox policy in the widget's config copy.
    pub(crate) fn set_sandbox_policy(&mut self, policy: SandboxPolicy) {
        self.config.sandbox_policy = policy;
    }

    /// Set the reasoning effort in the widget's config copy.
    pub(crate) fn set_reasoning_effort(&mut self, effort: Option<ReasoningEffortConfig>) {
        self.config.model_reasoning_effort = effort;
    }

    /// Set the model in the widget's config copy.
    pub(crate) fn set_model(&mut self, model: &str) {
        self.session_header.set_model(model);
        self.config.model = model.to_string();
        self.bottom_pane
            .set_current_model(self.config.model.clone());
    }

    pub(crate) fn add_info_message(&mut self, message: String, hint: Option<String>) {
        self.add_to_history(history_cell::new_info_event(message, hint));
        self.request_redraw();
    }

    pub(crate) fn add_error_message(&mut self, message: String) {
        self.add_to_history(history_cell::new_error_event(message));
        self.request_redraw();
    }

    pub(crate) fn add_mcp_output(&mut self) {
        if self.config.mcp_servers.is_empty() {
            self.add_to_history(history_cell::empty_mcp_output());
        } else {
            self.submit_op(Op::ListMcpTools);
        }
    }

    // Codex-Local custom command implementations
    fn add_config_output(&mut self) {
        let config_info = format!(
            "**Codex-Local Configuration**\n\n\
            Model: `{}`\n\
            Provider: `{}`\n\
            Context Window: `{}` tokens\n\
            Max Output: `{}` tokens\n\
            Auto-Compact: `{}` tokens\n\
            Config: `~/.codex-local/config.toml`",
            self.config.model,
            self.config.model_provider_id,
            self.config
                .model_context_window
                .map(|n| n.to_string())
                .unwrap_or("unknown".to_string()),
            self.config
                .model_max_output_tokens
                .map(|n| n.to_string())
                .unwrap_or("unknown".to_string()),
            self.config
                .model_auto_compact_token_limit
                .map(|n| n.to_string())
                .unwrap_or("disabled".to_string())
        );
        self.add_info_message(config_info, None);
    }

    fn add_context_info(&mut self) {
        let context = self.config.model_context_window.unwrap_or(0);
        let info = format!(
            "**Context Window**: `{}K` tokens\n\n\
            Use: `codex-local -c model_context_window=<tokens>` to override",
            context / 1000
        );
        self.add_info_message(info, None);
    }

    fn add_tokens_info(&mut self) {
        let tokens = self.config.model_max_output_tokens.unwrap_or(0);
        let info = format!(
            "**Max Output Tokens**: `{}K` tokens\n\n\
            Use: `codex-local -c model_max_output_tokens=<tokens>` to override",
            tokens / 1000
        );
        self.add_info_message(info, None);
    }

    fn add_provider_info(&mut self) {
        use codex_core::WireApi;

        let provider_info = format!(
            "**API Provider**\n\n\
            Name: `{}`\n\
            URL: `{}`\n\
            Wire API: `{}`\n\n\
            Use `codex-local-switch` to change provider settings",
            self.config.model_provider.name,
            self.config
                .model_provider
                .base_url
                .as_deref()
                .unwrap_or("not set"),
            match self.config.model_provider.wire_api {
                WireApi::Chat => "chat",
                WireApi::Responses => "responses",
            }
        );
        self.add_info_message(provider_info, None);
    }

    fn add_models_list(&mut self) {
        let info = "**Available Models**\n\nFetching models from `/v1/models` endpoint...\n\n\
                    This feature will list all available models from your API provider.";
        self.add_info_message(info.to_string(), None);
    }

    fn add_compact_settings_info(&mut self) {
        let limit = self.config.model_auto_compact_token_limit;
        let context = self.config.model_context_window.unwrap_or(0);
        let percentage = if let (Some(limit), true) = (limit, context > 0) {
            format!("{}%", (limit * 100) / context as i64)
        } else {
            "N/A".to_string()
        };

        let info = format!(
            "**Auto-Compaction Settings**\n\n\
            Threshold: `{}` tokens ({})\n\
            Context Window: `{}` tokens\n\n\
            Compaction triggers at {} of context capacity.\n\
            Use: `codex-local -c model_auto_compact_token_limit=<tokens>` to override",
            limit
                .map(|n| n.to_string())
                .unwrap_or_else(|| "disabled".to_string()),
            percentage.clone(),
            context,
            percentage
        );
        self.add_info_message(info, None);
    }

    fn add_think_toggle_info(&mut self) {
        let info = "**XML Thinking Block Rendering**\n\n\
                    Status: `Enabled`\n\n\
                    XML tags like `<think>`, `<thinking>`, `<thought>`, `<reasoning>`, and `<internal>` \
                    are rendered as beautiful bordered boxes.\n\n\
                    This feature is always enabled in codex-local.";
        self.add_info_message(info.to_string(), None);
    }

    fn add_agent_navigation_info(&mut self) {
        let total_agents = self.agents.len();
        if total_agents == 0 {
            return;
        }

        let mut info = "**Agent Navigation**\n\n".to_string();

        for (index, agent_id) in self.agents.keys().take(9).enumerate() {
            let num = index + 1;
            if let Some(meta) = self.agents.get(agent_id) {
                let purpose = if meta.purpose.trim().is_empty() {
                    agent_id.clone()
                } else {
                    meta.purpose.clone()
                };
                info.push_str(&format!("**Ctrl+{num}**: {purpose}\n"));
            }
        }

        if total_agents > 9 {
            info.push_str(&format!(
                "... and {} more (use **Alt+A** to see all)\n",
                total_agents - 9
            ));
        }

        info.push_str("\n**Ctrl+0**: Return to main agent\n");
        info.push_str("**Alt+A**: Show all agents overview");

        self.add_info_message(info, None);
    }

    fn add_orchestrator_toggle_info(&mut self) {
        let has_orchestrator_config = self.config.active_orchestrator_profile.is_some()
            && !self.config.active_agent_profiles.is_empty();

        let status = if has_orchestrator_config {
            let orch_profile = self
                .config
                .active_orchestrator_profile
                .as_deref()
                .unwrap_or("unknown");
            let agent_profiles = self.config.active_agent_profiles.join("`, `");
            format!(
                "**Status:** ✓ Configured\n\
                    - Orchestrator profile: `{orch_profile}`\n\
                    - Agent profiles: `{agent_profiles}`\n\n\
                    The orchestrator infrastructure is ready. When you ask the agent to break down\n\
                    complex tasks, it can coordinate multiple child agents automatically."
            )
        } else {
            "**Status:** ⚠ Not Configured\n\n\
            To enable orchestrator mode, add to `~/.codex-local/config.toml`:\n\n\
            ```toml\n\
            [profiles.orchestrator]\n\
            model = \"claude-sonnet-4\"\n\n\
            [profiles.worker]\n\
            model = \"claude-haiku-3-5\"\n\n\
            orchestrator_profile = \"orchestrator\"\n\
            agent_profiles = [\"worker\"]\n\
            ```"
            .to_string()
        };

        let info = format!(
            "**Multi-Agent Orchestrator Mode**\n\n\
                    This feature allows the main agent to coordinate multiple child agents, \
                    each working on separate subtasks in parallel.\n\n\
                    {status}\n\n\
                    **How it works:**\n\
                    - Main agent identifies subtasks that can be parallelized\n\
                    - Spawns child agents with isolated contexts\n\
                    - Each child works independently on its assigned task\n\
                    - Results are aggregated and validated via checklists\n\
                    - Protocol events show progress of all agents"
        );

        self.add_info_message(info, None);
    }

    fn add_profiles_info(&mut self) {
        let config_path = self.config.codex_home.join("config.toml");
        let config_path_str = config_path.to_string_lossy();

        let current_profile = self.config.active_profile.as_deref().unwrap_or("(default)");
        let orch_profile = self
            .config
            .active_orchestrator_profile
            .as_deref()
            .unwrap_or("(none)");
        let agent_profiles = if self.config.active_agent_profiles.is_empty() {
            "(none)".to_string()
        } else {
            self.config.active_agent_profiles.join(", ")
        };

        let current_model = &self.config.model;
        let current_provider = &self.config.model_provider_id;

        let info = format!(
            "**Configuration Profiles**\n\n\
                    **Current Active Settings:**\n\
                    - Main profile: `{current_profile}`\n\
                    - Model: `{current_model}`\n\
                    - Provider: `{current_provider}`\n\
                    - Orchestrator profile: `{orch_profile}`\n\
                    - Agent profiles: `{agent_profiles}`\n\n\
                    **To edit profiles:** Run `codex-local profiles edit`\n\
                    This will open `{config_path_str}` in your editor.\n\n\
                    **Example profile configuration:**\n\
                    ```toml\n\
                    # Main profile for general work\n\
                    [profiles.default]\n\
                    model = \"claude-sonnet-4\"\n\n\
                    # Fast profile for quick tasks\n\
                    [profiles.fast]\n\
                    model = \"claude-haiku-3-5\"\n\n\
                    # Orchestrator configuration\n\
                    [profiles.orchestrator]\n\
                    model = \"claude-sonnet-4\"\n\n\
                    [profiles.worker]\n\
                    model = \"claude-haiku-3-5\"\n\n\
                    # Activate profiles\n\
                    profile = \"default\"\n\
                    orchestrator_profile = \"orchestrator\"\n\
                    agent_profiles = [\"worker\"]\n\
                    ```\n\n\
                    **Note:** After editing config.toml, restart codex-local to apply changes."
        );

        self.add_info_message(info, None);
    }

    /// Forward file-search results to the bottom pane.
    pub(crate) fn apply_file_search_result(&mut self, query: String, matches: Vec<FileMatch>) {
        self.bottom_pane.on_file_search_result(query, matches);
    }

    /// Handle Ctrl-C key press.
    fn on_ctrl_c(&mut self) {
        if self.bottom_pane.on_ctrl_c() == CancellationEvent::Handled {
            return;
        }

        if let Some(agent_id) = self
            .active_subagent
            .as_ref()
            .cloned()
            .filter(|_| !self.bottom_pane.is_task_running())
        {
            self.bottom_pane.show_ctrl_c_quit_hint();
            self.submit_op(Op::InterruptAgent { agent_id });
            return;
        }

        if self.bottom_pane.is_task_running() {
            self.bottom_pane.show_ctrl_c_quit_hint();
            self.submit_op(Op::Interrupt);
            return;
        }

        self.submit_op(Op::Shutdown);
    }

    pub(crate) fn composer_is_empty(&self) -> bool {
        self.bottom_pane.composer_is_empty()
    }

    /// True when the UI is in the regular composer state with no running task,
    /// no modal overlay (e.g. approvals or status indicator), and no composer popups.
    /// In this state Esc-Esc backtracking is enabled.
    pub(crate) fn is_normal_backtrack_mode(&self) -> bool {
        self.bottom_pane.is_normal_backtrack_mode()
    }

    pub(crate) fn insert_str(&mut self, text: &str) {
        self.bottom_pane.insert_str(text);
    }

    /// Replace the composer content with the provided text and reset cursor.
    pub(crate) fn set_composer_text(&mut self, text: String) {
        self.bottom_pane.set_composer_text(text);
    }

    pub(crate) fn show_esc_backtrack_hint(&mut self) {
        self.bottom_pane.show_esc_backtrack_hint();
    }

    pub(crate) fn clear_esc_backtrack_hint(&mut self) {
        self.bottom_pane.clear_esc_backtrack_hint();
    }
    /// Forward an `Op` directly to codex.
    pub(crate) fn submit_op(&self, op: Op) {
        // Record outbound operation for session replay fidelity.
        crate::session_log::log_outbound_op(&op);
        if let Err(e) = self.codex_op_tx.send(op) {
            tracing::error!("failed to submit op: {e}");
        }
    }

    fn on_list_mcp_tools(&mut self, ev: McpListToolsResponseEvent) {
        self.add_to_history(history_cell::new_mcp_tools_output(
            &self.config,
            ev.tools,
            &ev.auth_statuses,
        ));
    }

    fn on_list_custom_prompts(&mut self, ev: ListCustomPromptsResponseEvent) {
        let len = ev.custom_prompts.len();
        debug!("received {len} custom prompts");
        // Forward to bottom pane so the slash popup can show them now.
        self.bottom_pane.set_custom_prompts(ev.custom_prompts);
    }

    pub(crate) fn open_review_popup(&mut self) {
        let mut items: Vec<SelectionItem> = Vec::new();

        items.push(SelectionItem {
            name: "Review against a base branch".to_string(),
            description: Some("(PR Style)".into()),
            actions: vec![Box::new({
                let cwd = self.config.cwd.clone();
                move |tx| {
                    tx.send(AppEvent::OpenReviewBranchPicker(cwd.clone()));
                }
            })],
            dismiss_on_select: false,
            ..Default::default()
        });

        items.push(SelectionItem {
            name: "Review uncommitted changes".to_string(),
            actions: vec![Box::new(
                move |tx: &AppEventSender| {
                    tx.send(AppEvent::CodexOp(Op::Review {
                        review_request: ReviewRequest {
                            prompt: "Review the current code changes (staged, unstaged, and untracked files) and provide prioritized findings.".to_string(),
                            user_facing_hint: "current changes".to_string(),
                        },
                    }));
                },
            )],
            dismiss_on_select: true,
            ..Default::default()
        });

        // New: Review a specific commit (opens commit picker)
        items.push(SelectionItem {
            name: "Review a commit".to_string(),
            actions: vec![Box::new({
                let cwd = self.config.cwd.clone();
                move |tx| {
                    tx.send(AppEvent::OpenReviewCommitPicker(cwd.clone()));
                }
            })],
            dismiss_on_select: false,
            ..Default::default()
        });

        items.push(SelectionItem {
            name: "Custom review instructions".to_string(),
            actions: vec![Box::new(move |tx| {
                tx.send(AppEvent::OpenReviewCustomPrompt);
            })],
            dismiss_on_select: false,
            ..Default::default()
        });

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select a review preset".into()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    pub(crate) async fn show_review_branch_picker(&mut self, cwd: &Path) {
        let branches = local_git_branches(cwd).await;
        let current_branch = current_branch_name(cwd)
            .await
            .unwrap_or_else(|| "(detached HEAD)".to_string());
        let mut items: Vec<SelectionItem> = Vec::with_capacity(branches.len());

        for option in branches {
            let branch = option.clone();
            items.push(SelectionItem {
                name: format!("{current_branch} -> {branch}"),
                actions: vec![Box::new(move |tx3: &AppEventSender| {
                    tx3.send(AppEvent::CodexOp(Op::Review {
                        review_request: ReviewRequest {
                            prompt: format!(
                                "Review the code changes against the base branch '{branch}'. Start by finding the merge diff between the current branch and {branch}'s upstream e.g. (`git merge-base HEAD \"$(git rev-parse --abbrev-ref \"{branch}@{{upstream}}\")\"`), then run `git diff` against that SHA to see what changes we would merge into the {branch} branch. Provide prioritized, actionable findings."
                            ),
                            user_facing_hint: format!("changes against '{branch}'"),
                        },
                    }));
                })],
                dismiss_on_select: true,
                search_value: Some(option),
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select a base branch".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            is_searchable: true,
            search_placeholder: Some("Type to search branches".to_string()),
            ..Default::default()
        });
    }

    pub(crate) async fn show_review_commit_picker(&mut self, cwd: &Path) {
        let commits = codex_core::git_info::recent_commits(cwd, 100).await;

        let mut items: Vec<SelectionItem> = Vec::with_capacity(commits.len());
        for entry in commits {
            let subject = entry.subject.clone();
            let sha = entry.sha.clone();
            let short = sha.chars().take(7).collect::<String>();
            let search_val = format!("{subject} {sha}");

            items.push(SelectionItem {
                name: subject.clone(),
                actions: vec![Box::new(move |tx3: &AppEventSender| {
                    let hint = format!("commit {short}");
                    let prompt = format!(
                        "Review the code changes introduced by commit {sha} (\"{subject}\"). Provide prioritized, actionable findings."
                    );
                    tx3.send(AppEvent::CodexOp(Op::Review {
                        review_request: ReviewRequest {
                            prompt,
                            user_facing_hint: hint,
                        },
                    }));
                })],
                dismiss_on_select: true,
                search_value: Some(search_val),
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select a commit to review".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            is_searchable: true,
            search_placeholder: Some("Type to search commits".to_string()),
            ..Default::default()
        });
    }

    pub(crate) fn show_review_custom_prompt(&mut self) {
        let tx = self.app_event_tx.clone();
        let view = CustomPromptView::new(
            "Custom review instructions".to_string(),
            "Type instructions and press Enter".to_string(),
            None,
            Box::new(move |prompt: String| {
                let trimmed = prompt.trim().to_string();
                if trimmed.is_empty() {
                    return;
                }
                tx.send(AppEvent::CodexOp(Op::Review {
                    review_request: ReviewRequest {
                        prompt: trimmed.clone(),
                        user_facing_hint: trimmed,
                    },
                }));
            }),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    /// Programmatically submit a user text message as if typed in the
    /// composer. The text will be added to conversation history and sent to
    /// the agent.
    pub(crate) fn submit_text_message(&mut self, text: String) {
        if text.is_empty() {
            return;
        }

        // Track input tokens
        let input_tokens = crate::token_counter::count_tokens(&text, &self.config.model) as u64;
        self.session_input_tokens += input_tokens;
        self.current_turn_input_tokens = input_tokens;

        // Update token display
        self.update_token_display();

        self.submit_user_message(text.into());
    }

    pub(crate) fn token_usage(&self) -> TokenUsage {
        self.token_info
            .as_ref()
            .map(|ti| ti.total_token_usage.clone())
            .unwrap_or_default()
    }

    pub(crate) fn conversation_id(&self) -> Option<ConversationId> {
        self.conversation_id
    }

    /// Return a reference to the widget's current config (includes any
    /// runtime overrides applied via TUI, e.g., model or approval policy).
    pub(crate) fn config_ref(&self) -> &Config {
        &self.config
    }

    pub(crate) fn clear_token_usage(&mut self) {
        self.token_info = None;
    }

    pub fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let [_, _, bottom_pane_area] = self.layout_areas(area);
        self.bottom_pane.cursor_pos(bottom_pane_area)
    }
}

impl WidgetRef for &ChatWidget {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let [_, active_cell_area, bottom_pane_area] = self.layout_areas(area);
        (&self.bottom_pane).render(bottom_pane_area, buf);
        if !active_cell_area.is_empty()
            && let Some(cell) = &self.active_cell
        {
            let mut area = active_cell_area;
            area.y = area.y.saturating_add(1);
            area.height = area.height.saturating_sub(1);
            if let Some(exec) = cell.as_any().downcast_ref::<ExecCell>() {
                exec.render_ref(area, buf);
            } else if let Some(tool) = cell.as_any().downcast_ref::<McpToolCallCell>() {
                tool.render_ref(area, buf);
            }
        }
        self.last_rendered_width.set(Some(area.width as usize));
    }
}

enum Notification {
    AgentTurnComplete { response: String },
    ExecApprovalRequested { command: String },
    EditApprovalRequested { cwd: PathBuf, changes: Vec<PathBuf> },
}

impl Notification {
    fn display(&self) -> String {
        match self {
            Notification::AgentTurnComplete { response } => {
                Notification::agent_turn_preview(response)
                    .unwrap_or_else(|| "Agent turn complete".to_string())
            }
            Notification::ExecApprovalRequested { command } => {
                format!("Approval requested: {}", truncate_text(command, 30))
            }
            Notification::EditApprovalRequested { cwd, changes } => {
                format!(
                    "Codex wants to edit {}",
                    if changes.len() == 1 {
                        #[allow(clippy::unwrap_used)]
                        display_path_for(changes.first().unwrap(), cwd)
                    } else {
                        format!("{} files", changes.len())
                    }
                )
            }
        }
    }

    fn type_name(&self) -> &str {
        match self {
            Notification::AgentTurnComplete { .. } => "agent-turn-complete",
            Notification::ExecApprovalRequested { .. }
            | Notification::EditApprovalRequested { .. } => "approval-requested",
        }
    }

    fn allowed_for(&self, settings: &Notifications) -> bool {
        match settings {
            Notifications::Enabled(enabled) => *enabled,
            Notifications::Custom(allowed) => allowed.iter().any(|a| a == self.type_name()),
        }
    }

    fn agent_turn_preview(response: &str) -> Option<String> {
        let mut normalized = String::new();
        for part in response.split_whitespace() {
            if !normalized.is_empty() {
                normalized.push(' ');
            }
            normalized.push_str(part);
        }
        let trimmed = normalized.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(truncate_text(trimmed, AGENT_NOTIFICATION_PREVIEW_GRAPHEMES))
        }
    }
}

const AGENT_NOTIFICATION_PREVIEW_GRAPHEMES: usize = 200;

const EXAMPLE_PROMPTS: [&str; 6] = [
    "Explain this codebase",
    "Summarize recent commits",
    "Implement {feature}",
    "Find and fix a bug in @filename",
    "Write tests for @filename",
    "Improve documentation in @filename",
];

// Extract the first bold (Markdown) element in the form **...** from `s`.
// Returns the inner text if found; otherwise `None`.
fn extract_first_bold(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() {
                if bytes[j] == b'*' && bytes[j + 1] == b'*' {
                    // Found closing **
                    let inner = &s[start..j];
                    let trimmed = inner.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    } else {
                        return None;
                    }
                }
                j += 1;
            }
            // No closing; stop searching (wait for more deltas)
            return None;
        }
        i += 1;
    }
    None
}

#[cfg(test)]
pub(crate) fn show_review_commit_picker_with_entries(
    chat: &mut ChatWidget,
    entries: Vec<codex_core::git_info::CommitLogEntry>,
) {
    let mut items: Vec<SelectionItem> = Vec::with_capacity(entries.len());
    for entry in entries {
        let subject = entry.subject.clone();
        let sha = entry.sha.clone();
        let short = sha.chars().take(7).collect::<String>();
        let search_val = format!("{subject} {sha}");

        items.push(SelectionItem {
            name: subject.clone(),
            actions: vec![Box::new(move |tx3: &AppEventSender| {
                let hint = format!("commit {short}");
                let prompt = format!(
                    "Review the code changes introduced by commit {sha} (\"{subject}\"). Provide prioritized, actionable findings."
                );
                tx3.send(AppEvent::CodexOp(Op::Review {
                    review_request: ReviewRequest {
                        prompt,
                        user_facing_hint: hint,
                    },
                }));
            })],
            dismiss_on_select: true,
            search_value: Some(search_val),
            ..Default::default()
        });
    }

    chat.bottom_pane.show_selection_view(SelectionViewParams {
        title: Some("Select a commit to review".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        is_searchable: true,
        search_placeholder: Some("Type to search commits".to_string()),
        ..Default::default()
    });
}

#[cfg(test)]
pub(crate) mod tests;
