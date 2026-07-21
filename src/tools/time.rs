use async_trait::async_trait;
use serde_json::Value;
use time::format_description::well_known::Rfc3339;

use crate::domain::{
    context::ToolContext,
    tool::{Tool, ToolError, ToolOutput},
};

pub struct TimeTool;

#[async_trait]
impl Tool for TimeTool {
    fn name(&self) -> &'static str {
        "time"
    }

    fn description(&self) -> &'static str {
        "Returns the current UTC date and time in RFC 3339 format."
    }

    async fn call(&self, _input: Value, _ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let s = time::OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|e| ToolError::Failed(e.into()))?;
        Ok(ToolOutput::text(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::context::SessionContext;
    use std::sync::Arc;

    struct DenyAll;
    #[async_trait]
    impl crate::domain::approval::Approver for DenyAll {
        async fn approve(&self, _r: &crate::domain::approval::ApprovalRequest) -> bool {
            false
        }
    }

    fn ctx() -> ToolContext {
        ToolContext::new(
            SessionContext::detached("cli:test"),
            None,
            Arc::new(DenyAll),
        )
    }

    #[tokio::test]
    async fn time_tool_returns_non_empty_string() {
        let out = TimeTool.call(Value::Null, &ctx()).await.unwrap();
        assert!(!out.text.is_empty());
    }
}
