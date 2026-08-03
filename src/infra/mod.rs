// Cross-cutting infra (LLM backend, Codex OAuth, workday calendar)
pub mod codex;
pub mod gateway_client;
pub mod llm;
pub mod logs;
pub mod permissions_store;
pub mod provider;
pub mod rendezvous;
pub mod skill_install;
pub mod skills;
pub mod workday;

// Layered infra by concern
pub mod memory;
pub mod messaging;
pub mod persistence;
