//! # Athenaeum providers — model access
//!
//! Knowledge is local; intelligence is not (buildplan §4). This crate is the
//! only part of the workspace that talks to a network, and the knowledge
//! library does not depend on it — so switching providers, or losing access
//! to one entirely, cannot touch what the user has learned (§62).
//!
//! Two pieces:
//!
//! * [`client`] — a multi-provider HTTP client, copied from llm_wiki.
//! * [`roles`] — which model plays which part (§5), the piece that makes
//!   "Opus plans, Haiku navigates" a configuration rather than a fork.

#![deny(rust_2018_idioms)]

pub mod client;
pub mod proxy;
pub mod roles;
pub mod types;

pub use client::{AgentLlmProvider, LlmClient, LlmConfig, LlmReasoningConfig};
pub use roles::{ModelRole, RoleAssignments};
pub use types::AgentImage;
