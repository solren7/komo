// Cross-cutting infra (LLM backend, tool adapter)
pub mod llm;
pub mod rig_tool;

// Layered infra by concern
pub mod memory;
pub mod messaging;
pub mod persistence;
