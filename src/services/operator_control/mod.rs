//! Operator control: one module owns how host-operator actions (list/inspect
//! reads, governance and maintenance writes) reach shion's state.
//!
//! Turso's exclusive cross-process lock means a running gateway is the sole
//! owner of the dbs — so every operator action has two transports: routed to
//! the gateway over its loopback api channel, or executed in-process against
//! directly-opened stores. This module hides that choice; CLI callers never
//! probe the gateway or pick a db themselves.

pub mod actions;
pub mod request;

pub use request::*;
