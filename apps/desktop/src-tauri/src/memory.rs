//! Native memory sessions. Retrieval authority stays with the completed source
//! job; accounting alone never grants source or model-processing permission.
#[path = "memory-budget.rs"]
pub mod budget;
#[path = "memory-context-input.rs"]
mod context_input;
#[path = "memory-model-budget.rs"]
pub mod model_budget;
#[path = "memory-model-contexts.rs"]
mod model_contexts;
#[path = "memory-model-input.rs"]
pub mod model_input;
#[path = "memory-receipt-history.rs"]
mod receipt_history;
#[path = "memory-session-registry.rs"]
pub mod registry;
#[path = "memory-research-support.rs"]
pub(crate) mod research_support;
#[path = "memory-session.rs"]
pub mod session;
#[path = "memory-types.rs"]
pub mod types;
