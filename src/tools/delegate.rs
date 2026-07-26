use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::domain::{
    context::ToolContext,
    llm::LlmClient,
    message::Message,
    session::Session,
    tool::{Tool, ToolError, ToolOutput, parse_args},
};

#[derive(Deserialize)]
struct DelegateArgs {
    task: String,
}

/// Delegates a self-contained subtask to a fresh sub-agent (its own LLM, with
/// no tools) and returns the sub-agent's answer. Useful for focused side
/// questions without polluting the main conversation.
pub struct DelegateTool {
    llm: Arc<dyn LlmClient>,
}

impl DelegateTool {
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self { llm }
    }
}

#[async_trait]
impl Tool for DelegateTool {
    fn name(&self) -> &'static str {
        "delegate"
    }

    fn description(&self) -> &'static str {
        "Delegate a focused, self-contained subtask to a sub-agent and return \
         its result. Provide all needed context in `task`; the sub-agent does \
         not see the main conversation."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "Fully self-contained instruction for the sub-agent."
                }
            },
            "required": ["task"]
        })
    }

    async fn call(&self, input: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let args: DelegateArgs = parse_args(&input)?;

        let mut session = Session::new("delegate");
        session.messages.push(Message::user(&args.task));
        Ok(ToolOutput::text(self.llm.complete(&session).await?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoLlm;
    #[async_trait]
    impl LlmClient for EchoLlm {
        async fn complete(&self, session: &Session) -> anyhow::Result<String> {
            let last = session.messages.last().unwrap();
            Ok(format!("sub-agent handled: {}", last.content))
        }
    }

    #[tokio::test]
    async fn delegates_task_to_sub_agent() {
        let tool = DelegateTool::new(Arc::new(EchoLlm));
        let out = tool
            .call(
                json!({ "task": "summarize X" }),
                &crate::tools::test_support::detached_ctx("cli:test"),
            )
            .await
            .unwrap();
        assert_eq!(out.text, "sub-agent handled: summarize X");
    }
}
