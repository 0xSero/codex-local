# Multi‑Agent Orchestrator: Scope and Tasks

This document captures the plan to introduce a multi‑agent system into Codex, centered around an Orchestrator that decomposes work into focused child agents with isolated context windows, distinct profiles, and a TUI that lets users switch between agents and track progress.

The plan references specific code locations to anchor changes. Paths are workspace‑relative; line numbers point to the start of relevant items and may shift as we implement changes.

## Goals

- Orchestrator agent that plans, curates context, and spawns task‑focused child agents; each child receives a prompt + checklist and returns a result.
- Orchestrator validates, tests, and enforces the checklist, keeping the main thread’s context slim by isolating child agent contexts.
- Separate profiles/config for orchestrator vs worker agents (models, providers, prompts), selectable at runtime.
- TUI supports multi‑threaded agent UX: switch between agents, see per‑agent progress, plan status, and review results cleanly.

## Current Architecture (as‑is)

- Session lifecycle and streaming
  - `codex-rs/core/src/codex.rs:147` `Codex::spawn` – creates a session and streams events.
  - `codex-rs/core/src/codex.rs:310` `Session::new` – builds `TurnContext`, `SessionServices`, and dispatches `SessionConfigured`.
  - `codex-rs/core/src/client.rs:94` `ModelClient` – Responses/Chat streaming; SSE parsing in `process_sse` at `codex-rs/core/src/client.rs:640`.
  - Conversation wrapper: `codex-rs/core/src/codex_conversation.rs:11`.
  - Conversation manager: `codex-rs/core/src/conversation_manager.rs:28` manages many conversations; `new_conversation`, `resume_*`, `fork_conversation` exist.

- Context model
  - Turn context: `codex-rs/core/src/codex.rs:250` `TurnContext` (model client, cwd, approval/sandbox policy, tools config, etc.).
  - Session mutable state: `codex-rs/core/src/state/session.rs:12` `SessionState` maintains in‑memory history + token/rate info.
  - Per‑turn state: `codex-rs/core/src/state/turn.rs:21` `ActiveTurn` and `TurnState` (pending approvals/inputs). Today the server runs a single task at a time (new task aborts prior at `codex-rs/core/src/tasks/mod.rs:35`).
  - Review threads are isolated “in‑memory history” turns; see `is_review_mode` plumbing in `codex-rs/core/src/codex.rs`.

- Tool calls and decision‑making
  - The model decides when to call tools. Router maps streamed `ResponseItem` → handler:
    - `codex-rs/core/src/tools/router.rs:57` `build_tool_call` and `dispatch_tool_call` route Function/Local/MCP.
    - Registry/handlers: `codex-rs/core/src/tools/registry.rs:56` and `codex-rs/core/src/tools/handlers/*` (shell, read_file, grep_files, apply_patch, MCP, etc.).
    - Exec tool execution and approval flow via executor: `codex-rs/core/src/tools/mod.rs:19`, `handle_container_exec_with_params` and `run_exec_with_events` in `codex-rs/core/src/codex.rs:1003`.

- Profiles and config
  - Effective config: `codex-rs/core/src/config.rs:66` `struct Config` with provider/model, sandbox/approval, tools toggles, etc.
  - Profiles: `codex-rs/core/src/config_profile.rs:11` `ConfigProfile` (model, provider, reasoning, verbosity, chatgpt_base_url, instructions_file).
  - Loading/overrides: `codex-rs/core/src/config.rs:980` `load_from_base_config_with_overrides` merges CLI/profile/defaults.
  - CLI overrides collector shared across tools: `codex-rs/common/src/config_override.rs:14` `CliConfigOverrides`.

- TUI/CLI
  - CLI entry: `codex-rs/cli/src/main.rs:125` forwards to TUI (`codex_tui::run_main`).
  - TUI app: `codex-rs/tui/src/app.rs:27` owns a single `ChatWidget` and a single `ConversationManager`.
  - Agent bootstrap: `codex-rs/tui/src/chatwidget/agent.rs:14` spawns a single agent loop per chat view.
  - Plan tool already exists (for display): `codex-rs/core/src/tools/handlers/plan.rs:1` `update_plan` emits `EventMsg::PlanUpdate`.

## Proposed Design Overview

- Introduce an Orchestrator that runs as a parent controller for one “main” thread and coordinates child agents:
  - Curates context and produces a spec + checklist per task.
  - Spawns child agents as independent Codex conversations (separate `ConversationId`) using per‑agent profiles.
  - Supplies each child with a tight prompt + checklist; collects results and validates against the checklist.
  - Streams status (plan, child lifecycle, progress, test results) back to the UI via new protocol events.

- Preserve backwards‑compatibility: single‑agent mode remains default; orchestrator UX and protocol events are additive.

## Protocol Extensions (codex-rs/protocol)

Add new events and lightweight identifiers so the UI can present multiple agents cleanly:

- New events in `codex-rs/protocol/src/protocol.rs`:
  - `AgentSpawnedEvent { agent_id: String, parent_id: Option<String>, profile: Option<String>, purpose: String }`.
  - `AgentProgressEvent { agent_id: String, message: String }`.
  - `AgentCompletedEvent { agent_id: String, success: bool, summary: String }`.
  - `OrchestratorPlanEvent` – reuse existing `PlanUpdate` for steps; add optional `agent_id` scope.
  - `AgentSwitchedEvent { agent_id: String }` (TUI hint for focus changes).

Notes:
- Keep existing `Event` envelope; extend `EventMsg` enum with new variants near other agent/task events (see `TaskStartedEvent` at `codex-rs/protocol/src/protocol.rs:545`).
- Derive `TS` for bindings, update `generate-ts` path consumers if any.

Tasks:
- Extend `EventMsg` and add structs; update `serde`/`ts-rs` derives.
- Wire through `codex-rs/exec` and any loggers if they mirror `EventMsg`.

## Config/Profile Changes (codex-rs/core, codex-rs/common)

Enable distinct profiles for orchestrator and workers; keep today’s profiles backward‑compatible.

- `ConfigToml` additions in `codex-rs/core/src/config.rs`:
  - Top‑level keys: `orchestrator_profile = "name"`, `agent_profiles = ["nameA", "nameB"]`.
  - Optional `profiles.<name>.system_prompt_file`, `profiles.<name>.tools` overrides to tune per‑role instructions and tool availability.

- Loader changes:
  - Parse new fields in `ConfigToml` and expose them on `Config` (e.g., `active_orchestrator_profile`, `active_agent_profiles: Vec<String>`).
  - Ensure `CliConfigOverrides` (`codex-rs/common/src/config_override.rs`) can override `profile`, `orchestrator_profile`, and enable/disable MCP servers by name (already supported in `process_mcp_flags()`).

- Persist/merge behavior:
  - Keep `persist_model_selection` semantics; do not auto‑persist orchestrator decisions unless explicit.

## Core Orchestrator (codex-rs/core)

Add a new module (or crate) implementing the orchestrator runtime. Preference: a new crate `codex-orchestrator` under `codex-rs/orchestrator` for separation, depending on `codex-core` and `codex-protocol`.

Minimum viable API:

- Orchestrator runtime
  - Manages one parent `CodexConversation` (the “main” thread) and N child `CodexConversation`s (one per task) via `ConversationManager` (`codex-rs/core/src/conversation_manager.rs:28`).
  - Emits protocol events on lifecycle (spawn/progress/complete) and forwards children’s `Event`s with `agent_id` context.
  - Provides a simple child agent contract: `run_task(prompt, checklist, profile) -> Result<ChildResult>`.

- Main thread interaction hooks
  - When the main model produces a plan (via `update_plan` tool at `codex-rs/core/src/tools/handlers/plan.rs:1`), the orchestrator interprets and enqueues child tasks.
  - Child agents run with isolated `InitialHistory::New` (or `Forked` from a scoped subset) to prevent parent context bloat.

Implementation touchpoints:

- New crate (preferred):
  - `codex-rs/orchestrator/Cargo.toml` (crate name `codex-orchestrator`).
  - Modules: `runtime.rs`, `spawner.rs` (uses `ConversationManager`), `events.rs` (maps to new protocol events), `profiles.rs` (profile selection), `validation.rs` (checklist/result validation), `spec.rs` (task spec model + serde).

- If implemented inside `core` instead:
  - New module `codex-rs/core/src/orchestrator/mod.rs` with the above pieces, re‑exporting a thin API for TUI.

## TUI UX (codex-rs/tui)

Support multi‑agent views, agent switching, and per‑agent progress.

- App structure
  - `codex-rs/tui/src/app.rs:27` currently holds a single `ChatWidget`. Refactor to own a collection of “threads”:
    - Add `struct AgentView { id: String, name: String, chat: ChatWidget, status, profile }`.
    - Keep a `Vec<AgentView>` and an `active_index`.
  - `chatwidget/agent.rs:14` can already spawn loops for an existing `CodexConversation`. Add a helper to spawn agent views using orchestrator spawner.

- Navigation and status
  - Add a left sidebar with agent list and status (icons/colors per status). Key bindings to switch agent focus.
  - Render orchestrator plan and per‑step status using existing `PlanUpdate` events, grouped by `agent_id`.
  - Show per‑agent token usage; reuse `TokenCountEvent` aggregation but scoped per conversation.

- Event wiring
  - Introduce an App‑level event router that tags incoming `Event` with `agent_id` based on conversation; route to the matching `ChatWidget`.
  - Handle new protocol events in TUI (spawn/switch/progress/complete). Update `codex-rs/tui/src/chatwidget.rs` to be agent‑agnostic where needed.

- Snapshot tests
  - Update `codex-rs/tui/src/snapshots/*` to cover new UI. Use `cargo insta` flow documented in the repo (see Tests section below).

Styling: follow `tui/styles.md` and ratatui Stylize helpers; keep spans/lines compact as per local conventions.

## CLI (codex-rs/cli)

- Add a flag or subcommand for orchestrator mode:
  - Example: `codex --orchestrator` or `codex orchestrate` to start in orchestrator UI mode.
  - Wire additional config flags: `--orchestrator-profile`, `--agent-profiles`, `--agent <name:model>`.
  - Parse and pass through to TUI init.

Files:
- `codex-rs/cli/src/main.rs:125` – extend `TuiCli` and boot logic; forward new flags.

## Validation and Testing

- Core orchestrator
  - Unit tests with the “test” model family and `test_sync_tool` (`codex-rs/core/src/tools/spec.rs:980` tests) to validate parallelism/barrier semantics as needed.
  - End‑to‑end: orchestrator spawns 2 children; each produces a deterministic message; orchestrator aggregates and emits `AgentCompleted` events.

- Config/profile
  - TOML parsing tests for new fields; ensure fallback/back‑compat (`ConfigToml` roundtrip in `codex-rs/core/src/config.rs:1180+`).

- TUI
  - Snapshot tests: multi‑agent list, switching, plan rendering.
  - Token usage panel shows per‑agent usage correctly.

Tooling:
- After Rust changes in `codex-rs`, run `just fmt`.
- Lint per‑crate with `just fix -p <project>` (ask before full workspace `just fix`).
- Tests: run crate tests for modified projects (e.g., `cargo test -p codex-tui`); ask before `cargo test --all-features` when core/common/protocol changed.
- Install tools if missing: `just`, `rg`, `cargo-insta`.

## Milestones and Tasks

1) Protocol: new events and TS bindings
- [ ] Add `AgentSpawnedEvent`, `AgentProgressEvent`, `AgentCompletedEvent`, `AgentSwitchedEvent` to `codex-rs/protocol/src/protocol.rs`.
- [ ] Re‑export/consume in core event mapping as needed.
- [ ] Update any TS generation consumers.

2) Config/profile: multi‑agent fields
- [ ] Extend `ConfigToml` with orchestrator/agent profile settings; thread through to `Config`.
- [ ] Update `CliConfigOverrides` parsing where relevant (already supports `-c key=value`).
- [ ] Tests for TOML parse + defaults.

3) Orchestrator runtime
- [ ] Create `codex-rs/orchestrator` crate (name: `codex-orchestrator`) or `core::orchestrator` module.
- [ ] Implement `Orchestrator::new`, `spawn_child(prompt, checklist, profile)`, and event fan‑out.
- [ ] Integrate with plan tool output to enqueue tasks.
- [ ] Validate results against checklist; emit `AgentCompletedEvent`.

4) TUI: multi‑agent UX
- [ ] Refactor `App` to manage multiple `AgentView`s and focus switching (`codex-rs/tui/src/app.rs:27`).
- [ ] Add sidebar and status badges; wire new events.
- [ ] Route events to correct `ChatWidget`; support per‑agent token usage aggregation.
- [ ] Snapshot tests + `cargo insta accept -p codex-tui` when intended.

5) CLI: flags and boot
- [ ] Extend CLI to accept orchestrator flags; forward to TUI init (`codex-rs/cli/src/main.rs:125`).

6) Docs and examples
- [ ] Update `codex-rs/README.md` and `codex-rs/config.md` with orchestrator/agent profiles & TUI usage.
- [ ] Add a minimal example config showing orchestrator + two worker profiles.

## Open Questions / Decisions

- Should the orchestrator be a new crate (`codex-orchestrator`) or part of `core`? Preference is a new crate for separation and to keep `core` focused.
- Child context seeding: always `InitialHistory::New`, or allow `Forked` slices from parent? Start with `New`; add forking later.
- Persisting model selections: keep parent’s persisted selection behavior; do not persist children’s transient choices.
- How strict should “checklist validation” be (programmatic tests vs prompt‑only)? Start with prompt‑level structured output and evolve toward programmatic checks.
