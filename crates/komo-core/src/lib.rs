//! komo-core: the dependency-light heart of komo.
//!
//! Holds the pure domain layer (value types + repository trait signatures, no
//! I/O), the gateway rendezvous file reader, the operator view DTOs, and home
//! path resolution — everything an HTTP client (the Dioxus GUI) needs to talk to
//! a running gateway without pulling in komo's heavy runtime (toasty/turso, rig,
//! the chat channels). The `komo` binary depends on this crate and re-exports
//! `domain` / `rendezvous` for path stability.

pub mod domain;
pub mod operator_view;
pub mod paths;
pub mod rendezvous;
