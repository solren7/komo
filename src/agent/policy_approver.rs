//! [`PolicyApprover`] — the configurable permission layer (roadmap §3).
//!
//! A decorator over the interactive approver (`CliApprover` / `ChatApprover`):
//! it consults the resolved [`Policy`] first, and only escalates to the wrapped
//! approver when the policy returns [`Verdict::Ask`]. This keeps the per-action
//! decision logic in one configurable place instead of scattered `if/else` in
//! each tool, while leaving each tool's own hardline floor untouched below it.
//!
//! Same composition shape as `agent::daemon::WorkdayGated` decorating a
//! `Maintenance`.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::info;

use crate::domain::{
    approval::{ApprovalRequest, Approver, Decision, Risk},
    policy::{Policy, Verdict},
};
use crate::services::tool_execution::current_session;

/// Wraps an [`Approver`], applying a [`Policy`] before falling back to it.
pub struct PolicyApprover {
    policy: Policy,
    inner: Arc<dyn Approver>,
}

impl PolicyApprover {
    /// Wrap `inner` with `policy`. Returns the trait object the tools depend on.
    pub fn wrap(policy: Policy, inner: Arc<dyn Approver>) -> Arc<dyn Approver> {
        Arc::new(Self { policy, inner })
    }
}

/// The channel a session id belongs to: the part before `:` (`feishu:oc_x` →
/// `feishu`), or `cli` for the REPL's bare uuid session ids.
fn channel_of(session_id: &str) -> String {
    match session_id.split_once(':') {
        Some((platform, _)) => platform.to_string(),
        None => "cli".to_string(),
    }
}

#[async_trait]
impl Approver for PolicyApprover {
    async fn decide(&self, request: &ApprovalRequest) -> Decision {
        let channel = current_session().map(|c| channel_of(&c.session_id));

        // Read-only actions get deny-only evaluation: a deny rule can block a
        // network fetch / file read, but nothing escalates one to a prompt — an
        // unmatched safe action stays allowed without consulting the inner
        // approver (which would auto-pass it anyway).
        if request.risk == Risk::Safe {
            let decision = self.policy.decide(request, channel.as_deref());
            if decision.verdict == Verdict::Deny {
                info!(summary = %request.summary, channel = ?channel, rule = ?decision.rule,
                      "policy: denied (safe action)");
                return policy_denial(decision);
            }
            return Decision::Allow;
        }

        let decision = self.policy.decide(request, channel.as_deref());
        match decision.verdict {
            Verdict::Deny => {
                info!(summary = %request.summary, channel = ?channel, rule = ?decision.rule,
                      "policy: denied");
                policy_denial(decision)
            }
            // The engine already gates no-session grants: with `channel = None`
            // only an explicitly `unattended` allow rule (never a default)
            // produces `Allow`, so an Allow here is safe to honor as-is.
            Verdict::Allow => {
                info!(summary = %request.summary, channel = ?channel, rule = ?decision.rule,
                      "policy: auto-allowed");
                Decision::Allow
            }
            Verdict::Ask => self.inner.decide(request).await,
        }
    }
}

/// A policy denial, explained to the model: naming the rule that blocked it is
/// what stops the model from retrying the same call in a loop, and tells it
/// whether to look for another route or give up and report the block.
fn policy_denial(decision: crate::domain::policy::Decision) -> Decision {
    match decision.rule {
        // The index is the one `komo policy list` prints, so the operator can
        // find the exact line if the user asks why.
        Some(i) => Decision::deny_because(format!(
            "被权限策略拒绝（命中规则 #{i}，见 `komo policy list`）。\
             这是 operator 在 config.toml 里设定的，重试同样的调用不会成功。"
        )),
        None => Decision::deny_because(
            "被权限策略的默认规则拒绝。重试同样的调用不会成功；\
             需要 operator 在 config.toml 的 [policy] 里放行。",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::approval::ActionRef;
    use crate::domain::policy::{Category, Effect, Matcher, Rule};
    use crate::services::tool_execution::{SessionContext, with_session};
    use std::sync::Mutex;

    struct Recording {
        asked: Mutex<bool>,
        answer: bool,
    }
    #[async_trait]
    impl Approver for Recording {
        async fn decide(&self, _request: &ApprovalRequest) -> Decision {
            *self.asked.lock().unwrap() = true;
            self.answer.into()
        }
    }

    fn allow_rule(value: &str) -> Rule {
        Rule {
            channels: None,
            category: Category::Shell,
            matcher: Matcher::Prefix,
            value: value.to_string(),
            access: None,
            effect: Effect::Allow,
            include_dangerous: false,
            unattended: false,
        }
    }

    fn shell_req() -> ApprovalRequest {
        ApprovalRequest::normal("run: cargo build").with_action(ActionRef::Shell {
            command: "cargo build".to_string(),
        })
    }

    #[tokio::test]
    async fn auto_allow_skips_inner_within_a_session() {
        let inner = Arc::new(Recording {
            asked: Mutex::new(false),
            answer: false,
        });
        let approver = PolicyApprover::wrap(
            Policy::new(vec![allow_rule("cargo ")], Verdict::Ask),
            inner.clone(),
        );
        let ctx = SessionContext::detached("cli-session");
        let allowed = with_session(ctx, approver.approve(&shell_req())).await;
        assert!(allowed);
        assert!(!*inner.asked.lock().unwrap(), "inner must not be consulted");
    }

    #[tokio::test]
    async fn allow_without_session_falls_through_to_inner() {
        let inner = Arc::new(Recording {
            asked: Mutex::new(false),
            answer: false,
        });
        let approver = PolicyApprover::wrap(
            Policy::new(vec![allow_rule("cargo ")], Verdict::Ask),
            inner.clone(),
        );
        // No `with_session`: a sweep-like context. Allow must not auto-grant.
        let allowed = approver.approve(&shell_req()).await;
        assert!(!allowed);
        assert!(*inner.asked.lock().unwrap(), "inner should be consulted");
    }

    #[tokio::test]
    async fn unattended_rule_auto_allows_without_a_session() {
        let inner = Arc::new(Recording {
            asked: Mutex::new(false),
            answer: false,
        });
        let mut rule = allow_rule("cargo ");
        rule.unattended = true;
        let approver = PolicyApprover::wrap(Policy::new(vec![rule], Verdict::Ask), inner.clone());
        // No `with_session`: the sweep context. The explicit opt-in grants.
        let allowed = approver.approve(&shell_req()).await;
        assert!(allowed);
        assert!(!*inner.asked.lock().unwrap(), "inner must not be consulted");
    }

    #[tokio::test]
    async fn safe_action_is_blocked_by_a_deny_rule_without_asking() {
        let inner = Arc::new(Recording {
            asked: Mutex::new(false),
            answer: true,
        });
        let mut deny = allow_rule("");
        deny.category = Category::Network;
        deny.matcher = Matcher::Suffix;
        deny.value = "internal.corp".to_string();
        deny.effect = Effect::Deny;
        let approver = PolicyApprover::wrap(Policy::new(vec![deny], Verdict::Ask), inner.clone());

        let req = ApprovalRequest::safe("fetch").with_action(ActionRef::Network {
            url: "https://api.internal.corp/secrets".to_string(),
        });
        let ctx = SessionContext::detached("cli-session");
        assert!(!with_session(ctx, approver.approve(&req)).await);
        assert!(!*inner.asked.lock().unwrap(), "safe deny must not prompt");
    }

    #[tokio::test]
    async fn unmatched_safe_action_passes_without_consulting_inner() {
        let inner = Arc::new(Recording {
            asked: Mutex::new(false),
            answer: false,
        });
        let approver = PolicyApprover::wrap(Policy::default(), inner.clone());
        let req = ApprovalRequest::safe("fetch").with_action(ActionRef::Network {
            url: "https://example.com".to_string(),
        });
        // Even with no session in scope (sweep/aux), safe stays allowed.
        assert!(approver.approve(&req).await);
        assert!(!*inner.asked.lock().unwrap());
    }

    #[tokio::test]
    async fn ask_delegates_to_inner() {
        let inner = Arc::new(Recording {
            asked: Mutex::new(false),
            answer: true,
        });
        let approver = PolicyApprover::wrap(Policy::default(), inner.clone());
        let ctx = SessionContext::detached("cli-session");
        let allowed = with_session(ctx, approver.approve(&shell_req())).await;
        assert!(allowed);
        assert!(*inner.asked.lock().unwrap());
    }
}
