use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;

use crate::domain::{
    context::ToolContext,
    repository::SessionRepository,
    tool::{Tool, ToolError, ToolOutput, parse_args},
};

#[derive(Deserialize)]
struct SessionArgs {
    action: String,
}

/// Introspection over Komo's own stored conversation sessions. Lets the
/// model answer "how many sessions do you have" from the database instead of
/// reaching for shell commands like `tmux ls` or `who`.
pub struct SessionTool {
    sessions: Arc<dyn SessionRepository>,
}

impl SessionTool {
    pub fn new(sessions: Arc<dyn SessionRepository>) -> Self {
        Self { sessions }
    }
}

#[async_trait]
impl Tool for SessionTool {
    fn name(&self) -> &'static str {
        "session"
    }

    fn description(&self) -> &'static str {
        "Inspect Komo's own stored conversation sessions (this agent's chat \
         history database, NOT system/tmux/login sessions). action=\"count\" \
         returns how many sessions exist; action=\"list\" returns each \
         session's id, creation time, and message count."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["count", "list"],
                    "description": "count = total number of stored sessions; list = one line per session."
                }
            },
            "required": ["action"]
        })
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let args: SessionArgs = parse_args(&input)?;
        let sessions = self.sessions.list().await?;

        match args.action.as_str() {
            "count" => Ok(
                ToolOutput::text(format!("{} stored sessions", sessions.len()))
                    .with_structured(json!({ "count": sessions.len() })),
            ),
            "list" => {
                if sessions.is_empty() {
                    return Ok(ToolOutput::text("no stored sessions"));
                }
                let lines: Vec<String> = sessions
                    .iter()
                    .map(|s| {
                        let created = time::OffsetDateTime::from_unix_timestamp(s.created_at)
                            .ok()
                            .and_then(|t| t.format(&Rfc3339).ok())
                            .unwrap_or_else(|| s.created_at.to_string());
                        format!(
                            "{} | created {} | {} messages ({} user turns)",
                            s.id,
                            created,
                            s.messages.len(),
                            s.user_turns()
                        )
                    })
                    .collect();
                Ok(ToolOutput::text(format!(
                    "{} sessions:\n{}",
                    sessions.len(),
                    lines.join("\n")
                ))
                .with_title(format!("{} sessions", sessions.len())))
            }
            other => Err(ToolError::InvalidInput(format!(
                "unknown session action `{other}` (expected: count | list)"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::session::Session;

    struct FakeRepo(Vec<Session>);

    #[async_trait]
    impl SessionRepository for FakeRepo {
        async fn find(&self, _id: &str) -> anyhow::Result<Option<Session>> {
            Ok(None)
        }
        async fn find_windowed(&self, _id: &str, _limit: usize) -> anyhow::Result<Option<Session>> {
            Ok(None)
        }
        async fn list(&self) -> anyhow::Result<Vec<Session>> {
            Ok(self.0.clone())
        }
        async fn save(&self, _session: &Session) -> anyhow::Result<()> {
            Ok(())
        }
        async fn delete_empty_sessions(&self) -> anyhow::Result<usize> {
            Ok(0)
        }
        async fn rotate(&self, _session_id: &str) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
    }

    fn ctx() -> ToolContext {
        crate::tools::test_support::detached_ctx("cli:test")
    }

    #[tokio::test]
    async fn count_reports_number_of_sessions() {
        let repo = Arc::new(FakeRepo(vec![Session::new("a"), Session::new("b")]));
        let out = SessionTool::new(repo)
            .call(json!({"action":"count"}), &ctx())
            .await
            .unwrap();
        assert_eq!(out.text, "2 stored sessions");
    }

    #[tokio::test]
    async fn list_includes_session_ids() {
        let repo = Arc::new(FakeRepo(vec![Session::new("abc-123")]));
        let out = SessionTool::new(repo)
            .call(json!({"action":"list"}), &ctx())
            .await
            .unwrap();
        assert!(out.text.contains("abc-123"));
        assert!(out.text.contains("0 user turns"));
    }

    #[tokio::test]
    async fn unknown_action_is_invalid_input() {
        let repo = Arc::new(FakeRepo(Vec::new()));
        let err = SessionTool::new(repo)
            .call(json!({"action":"drop"}), &ctx())
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidInput(_)));
        assert!(err.to_string().contains("unknown session action"));
    }
}
