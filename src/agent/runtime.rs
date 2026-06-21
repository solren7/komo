use std::sync::Arc;

use tracing::{Instrument, info, info_span, warn};

use crate::{
    domain::{
        llm::LlmClient,
        message::Message,
        repository::{MessageRepository, SessionRepository},
        reviewer::Reviewer,
        run::{RUN_FIELD_CAP, Run, RunRepository, RunStatus, truncate},
        session::Session,
    },
    services::tool_registry::{
        RunContext, SessionContext, current_session, with_run, with_session,
    },
};

pub struct AgentRuntime {
    pub llm: Arc<dyn LlmClient>,
    pub sessions: Arc<dyn SessionRepository>,
    pub messages: Arc<dyn MessageRepository>,
    /// Run ledger: every turn is recorded here, with one step per tool call
    /// (captured at `execute_isolated`). See `domain/run.rs`, roadmap §7.
    pub runs: Arc<dyn RunRepository>,
    pub reviewer: Option<Arc<dyn Reviewer>>,
    pub review_interval: usize,
}

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

impl AgentRuntime {
    pub async fn handle_input(
        &self,
        session_id: &str,
        user_input: String,
    ) -> anyhow::Result<String> {
        // Session-scoped tools (e.g. `todo`) read the turn's session from the
        // ambient context. The gateway dispatcher sets it (with a real reply
        // sink); the REPL calls us directly, so establish a detached context
        // here when none exists. Don't override an existing one — that would
        // drop the gateway's sink and break mid-turn approval.
        if current_session().is_none() {
            let ctx = SessionContext::detached(session_id);
            return with_session(ctx, self.run_turn(session_id, user_input)).await;
        }
        self.run_turn(session_id, user_input).await
    }

    /// One turn = one [`Run`]. Opens a ledger entry, runs the turn body under a
    /// `RunContext` (so tool calls record steps) and a `run` tracing span, then
    /// finalizes the entry with the outcome. All ledger writes are best-effort:
    /// a ledger failure is logged but never changes the turn's result.
    async fn run_turn(&self, session_id: &str, user_input: String) -> anyhow::Result<String> {
        let mut run = Run::start(session_id, &user_input);
        if let Err(error) = self.runs.start(&run).await {
            warn!(%error, "failed to open run ledger entry (non-fatal)");
        }

        let span = info_span!("run", run_id = %run.id, session = %session_id);
        let ctx = RunContext::new(run.id.clone(), self.runs.clone());
        // Keep a handle to read the tool-step count after the turn (the seq
        // counter is shared via `Arc`, so this clone sees the final value).
        let probe = ctx.clone();

        let outcome = with_run(ctx, self.turn_body(session_id, user_input))
            .instrument(span)
            .await;

        run.plan = match probe.steps_count() {
            0 => "respond".to_string(),
            n => format!("{n} tool call(s)"),
        };
        run.ended_at = Some(now());
        match &outcome {
            Ok(reply) => {
                run.status = RunStatus::Done;
                run.final_output = truncate(reply, RUN_FIELD_CAP);
                info!(run_id = %run.id, "run done");
            }
            Err(error) => {
                run.status = RunStatus::Failed;
                run.error = truncate(&format!("{error:#}"), RUN_FIELD_CAP);
                warn!(run_id = %run.id, %error, "run failed");
            }
        }
        if let Err(error) = self.runs.finish(&run).await {
            warn!(%error, "failed to finalize run ledger entry (non-fatal)");
        }

        outcome
    }

    /// The turn's actual work: persist the user message, let the LLM answer
    /// (it drives any tool calls itself via rig), persist the reply, and kick
    /// off the periodic reviewer.
    async fn turn_body(&self, session_id: &str, user_input: String) -> anyhow::Result<String> {
        // Load or create session.
        let mut session = match self.sessions.find(session_id).await? {
            Some(s) => s,
            None => {
                let s = Session::new(session_id);
                self.sessions.save(&s).await?;
                s
            }
        };

        let user_msg = Message::user(&user_input);
        self.messages.save(session_id, &user_msg).await?;
        session.messages.push(user_msg);

        let reply = self.llm.complete(&session).await?;

        let assistant_msg = Message::assistant(&reply);
        self.messages.save(session_id, &assistant_msg).await?;
        session.messages.push(assistant_msg);

        if let Some(reviewer) = &self.reviewer {
            let interval = self.review_interval.max(1);
            if session.user_turns() % interval == 0 {
                let reviewer = reviewer.clone();
                let snapshot = session.clone();
                tokio::spawn(async move {
                    match reviewer.review(&snapshot).await {
                        Ok(outcome) if !outcome.is_empty() => {
                            info!(?outcome, "self-improvement review")
                        }
                        Ok(_) => {}
                        Err(error) => warn!(%error, "review failed (non-fatal)"),
                    }
                });
            }
        }

        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{llm::LlmClient, run::RunStatus, session::Session, tool::Tool},
        infra::db::Db,
        services::tool_registry::execute_isolated,
        tools::time::TimeTool,
    };
    use async_trait::async_trait;

    /// Stub LLM: returns a fixed reply, and (to mimic rig's tool-calling loop)
    /// optionally invokes one tool through `execute_isolated` — the same path a
    /// real tool call takes — so we can assert the run context set by
    /// `run_turn` reaches the tool and a step is recorded.
    struct StubLlm {
        reply: &'static str,
        tool: Option<Arc<dyn Tool>>,
    }

    #[async_trait]
    impl LlmClient for StubLlm {
        async fn complete(&self, _session: &Session) -> anyhow::Result<String> {
            if let Some(tool) = &self.tool {
                let _ = execute_isolated(tool.clone(), "{}".into()).await;
            }
            Ok(self.reply.to_string())
        }
    }

    fn sqlite_url(name: &str) -> String {
        let path = std::env::temp_dir().join(name);
        let _ = std::fs::remove_file(&path);
        format!("sqlite:{}", path.display())
    }

    fn runtime_with(db: Arc<Db>, tool: Option<Arc<dyn Tool>>) -> AgentRuntime {
        AgentRuntime {
            llm: Arc::new(StubLlm {
                reply: "hello there",
                tool,
            }),
            sessions: db.clone(),
            messages: db.clone(),
            runs: db.clone(),
            reviewer: None,
            review_interval: 10,
        }
    }

    #[tokio::test]
    async fn turn_with_a_tool_call_records_a_run_with_a_step() {
        let db = Arc::new(
            Db::connect(&sqlite_url("shion_rt_tool_run.db"))
                .await
                .unwrap(),
        );
        let rt = runtime_with(db.clone(), Some(Arc::new(TimeTool)));

        rt.handle_input("cli:s1", "hi".into()).await.unwrap();

        let runs = RunRepository::list(&*db, 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Done);
        assert_eq!(runs[0].plan, "1 tool call(s)");

        let steps = RunRepository::steps(&*db, &runs[0].id).await.unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].tool_name, "time");
        assert!(steps[0].ok);
    }

    #[tokio::test]
    async fn turn_without_tools_records_a_run_without_steps() {
        let db = Arc::new(
            Db::connect(&sqlite_url("shion_rt_direct_run.db"))
                .await
                .unwrap(),
        );
        let rt = runtime_with(db.clone(), None);

        let reply = rt.handle_input("cli:s2", "hi".into()).await.unwrap();
        assert_eq!(reply, "hello there");

        let runs = RunRepository::list(&*db, 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Done);
        assert_eq!(runs[0].plan, "respond");
        assert_eq!(runs[0].final_output, "hello there");

        let steps = RunRepository::steps(&*db, &runs[0].id).await.unwrap();
        assert!(steps.is_empty());
    }
}
