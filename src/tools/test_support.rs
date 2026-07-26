//! Shared test fixtures for tool tests.
//!
//! Every tool now takes an explicit [`ToolContext`] (tool trait v2), so each
//! test module used to hand-roll the same deny-all / allow-all approver plus a
//! detached session. These are those, once.
//!
//! Tools with interesting approval behavior (`shell`, `file`, `homeassistant`)
//! keep their own recording doubles — asserting *what* was asked is part of what
//! those tests are for.

use std::sync::Arc;

use crate::domain::{
    approval::{ApprovalRequest, Approver, Decision, Risk},
    context::{SessionContext, ToolContext},
};

/// Mirrors what every real approver does when nobody can answer: `Risk::Safe`
/// passes (reads never prompt), anything side-effecting is refused.
///
/// Deliberately not a blanket deny — that would refuse the `read`/`grep` family
/// too, which no production path does, so tests would be asserting against a
/// policy that doesn't exist.
pub struct SafeOnly;

#[async_trait::async_trait]
impl Approver for SafeOnly {
    async fn decide(&self, request: &ApprovalRequest) -> Decision {
        if request.risk == Risk::Safe {
            Decision::Allow
        } else {
            Decision::deny()
        }
    }
}

/// Approves everything.
pub struct AllowAll;

#[async_trait::async_trait]
impl Approver for AllowAll {
    async fn decide(&self, _request: &ApprovalRequest) -> Decision {
        Decision::Allow
    }
}

/// A detached context for `session` with the [`SafeOnly`] approver: a test using
/// it fails loudly if the tool asks approval for something it shouldn't, while
/// read-only work behaves as it does in production.
pub fn detached_ctx(session: &str) -> ToolContext {
    ToolContext::new(SessionContext::detached(session), None, Arc::new(SafeOnly))
}

/// A detached context whose approver allows everything.
pub fn approving_ctx(session: &str) -> ToolContext {
    ToolContext::new(SessionContext::detached(session), None, Arc::new(AllowAll))
}
