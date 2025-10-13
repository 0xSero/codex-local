//! Orchestrator runtime for multi-agent coordination in Codex.
//!
//! This crate provides the orchestrator runtime that manages a parent "main" thread
//! and coordinates multiple child agents. Each child agent runs as an independent
//! Codex conversation with isolated context.

pub mod events;
pub mod profiles;
pub mod runtime;
pub mod spawner;
pub mod spec;
pub mod validation;

pub use runtime::Orchestrator;
pub use spec::ChecklistItem;
pub use spec::TaskSpec;
