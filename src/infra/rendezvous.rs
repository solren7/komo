//! The gateway rendezvous reader moved to `komo-core` so the GUI client can
//! discover a running gateway (`~/.komo/gateway.json`) without komo's heavy
//! runtime. This facade re-exports it so `crate::infra::rendezvous::…` paths are
//! unchanged.
pub use komo_core::rendezvous::*;
