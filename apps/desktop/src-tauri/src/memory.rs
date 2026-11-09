//! Native memory sessions. Retrieval authority stays with the completed source
//! job; accounting alone never grants source or model-processing permission.
#[path = "memory-budget.rs"]
pub mod budget;
#[path = "memory-commands.rs"]
pub mod commands;
#[path = "memory-context-input.rs"]
mod context_input;
#[path = "memory-context-wire.rs"]
pub mod context_wire;
#[path = "memory-manifest-draft.rs"]
pub mod manifest_draft;
#[path = "memory-model-budget.rs"]
pub mod model_budget;
#[path = "memory-model-contexts.rs"]
mod model_contexts;
#[path = "memory-model-input.rs"]
pub mod model_input;
#[path = "memory-persona-workflow.rs"]
pub mod persona_workflow;
#[path = "memory-profile-snapshot.rs"]
pub mod profile_snapshot;
#[path = "memory-publication.rs"]
pub mod publication;
#[path = "memory-receipt-history.rs"]
mod receipt_history;
#[path = "memory-session-registry.rs"]
pub mod registry;
#[path = "memory-research-draft.rs"]
pub mod research_draft;
#[path = "memory-research-support.rs"]
pub(crate) mod research_support;
#[path = "memory-research-workflow.rs"]
pub mod research_workflow;
#[path = "memory-session.rs"]
pub mod session;
#[path = "memory-types.rs"]
pub mod types;

#[path = "memory-simulation.rs"]
pub mod simulation;
