//! What is left of `infra/` after the dependency-light half moved to
//! `komo-infra`: the pieces that reach *upward* into the agent and the services,
//! so they cannot live below them.
//!
//! - `llm` assembles the tiered system prompt (`agent::system_prompt`) and wires
//!   the tool executor in, over `komo-provider`'s wire layer.
//! - `messaging` hosts the chat channels, which dispatch real turns.
//! - `gateway_client` / `skill_install` speak the operator-control vocabulary.
pub mod gateway_client;
pub mod llm;
pub mod rendezvous;
pub mod skill_install;

pub mod messaging;
