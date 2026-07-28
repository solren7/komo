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
    policy::{Policy, Rule, Verdict, channel_of},
};
use crate::infra::permissions_store::PermissionsStore;
use crate::services::tool_execution::current_session;

/// Wraps an [`Approver`], applying a [`Policy`] before falling back to it.
pub struct PolicyApprover {
    policy: Policy,
    inner: Arc<dyn Approver>,
    /// Where an "always allow" answer is persisted. `None` for the unattended
    /// approvers (cron / briefing), which can never receive that answer anyway —
    /// there is nobody at the prompt.
    saved: Option<Arc<PermissionsStore>>,
}

impl PolicyApprover {
    /// Wrap `inner` with `policy`. Returns the trait object the tools depend on.
    pub fn wrap(policy: Policy, inner: Arc<dyn Approver>) -> Arc<dyn Approver> {
        Arc::new(Self {
            policy,
            inner,
            saved: None,
        })
    }

    /// [`wrap`](Self::wrap), plus the store that makes an `a` answer durable.
    /// This is the **only** place a grant is written: the interactive approvers
    /// just report which answer came back, so the three of them (CLI, chat, TUI)
    /// can't drift on what "always" means.
    pub fn wrap_with_store(
        policy: Policy,
        inner: Arc<dyn Approver>,
        saved: Arc<PermissionsStore>,
    ) -> Arc<dyn Approver> {
        Arc::new(Self {
            policy,
            inner,
            saved: Some(saved),
        })
    }

    /// Persist the narrowest rule covering `request`, scoped to the channel the
    /// answer came from. Best-effort and never fails the call: the user said yes,
    /// and the only thing at stake is whether they get asked again.
    fn remember(&self, request: &ApprovalRequest, channel: Option<&str>) {
        let (Some(store), Some(action), Some(channel)) =
            (self.saved.as_ref(), request.action.as_ref(), channel)
        else {
            // No store, no structured action to generalize, or no session to
            // scope to — nothing safe to write, so the grant stays session-local.
            info!(summary = %request.summary, "always-allow could not be saved; granted once");
            return;
        };
        let Some(rule) = Rule::narrowest_for(action, channel) else {
            return;
        };
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let described = rule.describe();
        if store.remember(rule, &now) {
            info!(rule = %described, "saved an always-allow grant");
        }
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
            Verdict::Ask => match self.inner.decide(request).await {
                // "Allow, and remember it": persist here, then hand the caller a
                // plain Allow — no tool needs to know the difference.
                Decision::AllowAlways => {
                    self.remember(request, channel.as_deref());
                    Decision::Allow
                }
                other => other,
            },
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

    /// The `a` answer's whole point: the grant outlives the process. The
    /// approver is the only writer, so this is where that is pinned.
    #[tokio::test]
    async fn always_persists_a_narrow_grant_and_stops_asking() {
        struct Always;
        #[async_trait]
        impl Approver for Always {
            async fn decide(&self, _request: &ApprovalRequest) -> Decision {
                Decision::AllowAlways
            }
        }

        let home = std::env::temp_dir().join("komo_policy_always");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let store = Arc::new(PermissionsStore::load(&home));
        let approver = PolicyApprover::wrap_with_store(
            Policy::default().with_saved(store.rules()),
            Arc::new(Always),
            store.clone(),
        );

        let ctx = SessionContext::detached("cli-session");
        assert!(with_session(ctx.clone(), approver.approve(&shell_req())).await);

        // Saved, narrowed to the command's first token, scoped to the channel.
        let saved = store.list();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].value, "cargo ");
        assert_eq!(saved[0].channels, Some(vec!["cli".to_string()]));

        // …and a fresh process would honor it without asking: build a *deny-all*
        // inner over the reloaded store and confirm the policy short-circuits.
        let reloaded = Arc::new(PermissionsStore::load(&home));
        let inner = Arc::new(Recording {
            asked: Mutex::new(false),
            answer: false,
        });
        let next = PolicyApprover::wrap_with_store(
            Policy::default().with_saved(reloaded.rules()),
            inner.clone(),
            reloaded,
        );
        assert!(with_session(ctx, next.approve(&shell_req())).await);
        assert!(
            !*inner.asked.lock().unwrap(),
            "a saved grant must not reach the interactive approver"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    /// No session in scope ⇒ nothing to scope a rule to, so the grant stays
    /// session-local rather than being written channel-less (which would apply
    /// everywhere — the opposite of narrow).
    #[tokio::test]
    async fn always_without_a_session_grants_once_and_saves_nothing() {
        struct Always;
        #[async_trait]
        impl Approver for Always {
            async fn decide(&self, _request: &ApprovalRequest) -> Decision {
                Decision::AllowAlways
            }
        }

        let home = std::env::temp_dir().join("komo_policy_always_nosession");
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let store = Arc::new(PermissionsStore::load(&home));
        let approver = PolicyApprover::wrap_with_store(
            Policy::default().with_saved(store.rules()),
            Arc::new(Always),
            store.clone(),
        );

        assert!(approver.approve(&shell_req()).await);
        assert!(store.is_empty());

        let _ = std::fs::remove_dir_all(&home);
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
