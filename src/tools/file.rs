use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::domain::{
    approval::{ActionRef, ApprovalRequest},
    context::ToolContext,
    tool::{Tool, ToolError, ToolOutput, parse_args},
    workspace::Workspace,
};

/// Maximum number of bytes returned by a single read, to keep tool output bounded.
const MAX_READ_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
struct FileArgs {
    action: String,
    path: String,
    #[serde(default)]
    content: Option<String>,
}

/// Reads and writes local files, confined to a [`Workspace`]. Writes require
/// user approval, requested through the call's [`ToolContext`].
pub struct FileTool {
    workspace: Arc<Workspace>,
}

impl FileTool {
    pub fn new(workspace: Arc<Workspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for FileTool {
    fn name(&self) -> &'static str {
        "file"
    }

    fn description(&self) -> &'static str {
        "Read or write a local file within the workspace. action=\"read\" returns \
         the file's contents; action=\"write\" creates or overwrites the file with \
         the given content (requires user approval)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "write"],
                    "description": "Whether to read or write the file."
                },
                "path": {
                    "type": "string",
                    "description": "Path to the file, inside the workspace."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write. Required when action=\"write\"."
                }
            },
            "required": ["action", "path"]
        })
    }

    /// Drop the write `content` body before it reaches the run ledger — the file
    /// being written can be arbitrarily large and may contain secrets. Keep the
    /// action, path, and a byte count so the step still reads sensibly.
    fn redact_args(&self, args: &str) -> String {
        match serde_json::from_str::<serde_json::Value>(args) {
            Ok(mut v) => {
                if let Some(content) = v.get("content").and_then(|c| c.as_str()) {
                    let len = content.len();
                    v["content"] = serde_json::json!(format!("<{len} bytes redacted>"));
                }
                v.to_string()
            }
            // Unparseable args: keep nothing rather than risk leaking a body.
            Err(_) => "<file args redacted>".to_string(),
        }
    }

    async fn call(&self, input: Value, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let args: FileArgs = parse_args(&input)?;

        // Workspace whitelist: reject any path outside the allowed roots.
        if !self.workspace.contains(Path::new(&args.path)) {
            return Err(ToolError::Failed(anyhow::anyhow!(
                "path `{}` is outside the workspace and was blocked",
                args.path
            )));
        }

        match args.action.as_str() {
            "read" => {
                // Reads are `Risk::Safe` (never prompt), but the policy layer's
                // deny rules still apply — `category = "file", access = "read"`
                // can fence off sensitive paths inside the workspace.
                let request = ApprovalRequest::safe(format!("read {}", args.path)).with_action(
                    ActionRef::File {
                        path: Path::new(&args.path).to_path_buf(),
                        write: false,
                    },
                );
                if !ctx.approve(&request).await {
                    return Ok(ToolOutput::text(format!(
                        "Read blocked by the permission policy (a `file` deny rule matches {}); \
                         nothing was read.",
                        args.path
                    )));
                }

                let mut text = tokio::fs::read_to_string(&args.path)
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", args.path))?;
                if text.len() > MAX_READ_BYTES {
                    text.truncate(MAX_READ_BYTES);
                    text.push_str("\n…[truncated]");
                }
                Ok(ToolOutput::text(text).with_title(format!("read {}", args.path)))
            }
            "write" => {
                let content = args
                    .content
                    .ok_or_else(|| anyhow::anyhow!("`content` is required for action=write"))?;

                // Approval gate: writing mutates the filesystem. Answering
                // "session" at the prompt allows further writes this session.
                let request = ApprovalRequest::normal(format!(
                    "write {} bytes to {}",
                    content.len(),
                    args.path
                ))
                .with_scope_key("file:write")
                .with_action(ActionRef::File {
                    path: Path::new(&args.path).to_path_buf(),
                    write: true,
                });
                if !ctx.approve(&request).await {
                    return Ok(ToolOutput::text(
                        "Write rejected by user; nothing was changed.",
                    ));
                }

                tokio::fs::write(&args.path, &content)
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to write {}: {e}", args.path))?;
                Ok(
                    ToolOutput::text(format!("Wrote {} bytes to {}", content.len(), args.path))
                        .with_title(format!("write {}", args.path)),
                )
            }
            other => Err(ToolError::InvalidInput(format!(
                "unknown action `{other}` (expected \"read\" or \"write\")"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::approval::{ApprovalRequest, Approver};
    use crate::domain::context::SessionContext;
    use std::path::PathBuf;

    struct AlwaysApprove;
    #[async_trait::async_trait]
    impl Approver for AlwaysApprove {
        async fn approve(&self, _request: &ApprovalRequest) -> bool {
            true
        }
    }

    fn ctx_with(approver: Arc<dyn Approver>) -> ToolContext {
        ToolContext::new(SessionContext::detached("cli:test"), None, approver)
    }

    fn tool_rooted_at(dir: PathBuf) -> FileTool {
        FileTool::new(Arc::new(Workspace::new(vec![dir])))
    }

    #[tokio::test]
    async fn write_then_read_roundtrips() {
        let dir = std::env::temp_dir();
        let path = dir.join("komo_file_tool_test.txt");
        let path_str = path.to_string_lossy().to_string();
        let tool = tool_rooted_at(dir);
        let ctx = ctx_with(Arc::new(AlwaysApprove));

        let write_args = json!({ "action": "write", "path": path_str, "content": "hello" });
        let out = tool.call(write_args, &ctx).await.unwrap();
        assert!(out.text.contains("Wrote 5 bytes"));

        let read_args = json!({ "action": "read", "path": path_str });
        let content = tool.call(read_args, &ctx).await.unwrap();
        assert_eq!(content.text, "hello");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn write_without_content_errors() {
        let dir = std::env::temp_dir();
        let path = dir
            .join("komo_no_content.txt")
            .to_string_lossy()
            .to_string();
        let tool = tool_rooted_at(dir);
        let ctx = ctx_with(Arc::new(AlwaysApprove));
        let args = json!({ "action": "write", "path": path });
        assert!(tool.call(args, &ctx).await.is_err());
    }

    #[tokio::test]
    async fn redact_args_drops_write_content_body() {
        let tool = tool_rooted_at(std::env::temp_dir());
        let args = json!({ "action": "write", "path": "/x/y.txt", "content": "secret-body" });
        let redacted = tool.redact_args(&args.to_string());
        assert!(!redacted.contains("secret-body"));
        assert!(redacted.contains("redacted"));
        assert!(redacted.contains("/x/y.txt")); // path preserved
    }

    #[tokio::test]
    async fn denied_read_is_blocked_by_the_approver() {
        struct DenyAll;
        #[async_trait::async_trait]
        impl Approver for DenyAll {
            async fn approve(&self, _request: &ApprovalRequest) -> bool {
                false
            }
        }
        let dir = std::env::temp_dir();
        let path = dir.join("komo_denied_read.txt");
        std::fs::write(&path, "secret").unwrap();
        let tool = FileTool::new(Arc::new(Workspace::new(vec![dir])));
        let ctx = ctx_with(Arc::new(DenyAll));
        let args = json!({ "action": "read", "path": path.to_string_lossy() });
        let out = tool.call(args, &ctx).await.unwrap();
        assert!(out.text.contains("blocked by the permission policy"));
        assert!(!out.text.contains("secret"));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn path_outside_workspace_is_blocked() {
        let tool = tool_rooted_at(PathBuf::from("/home/user/project"));
        let ctx = ctx_with(Arc::new(AlwaysApprove));
        let args = json!({ "action": "read", "path": "/etc/passwd" });
        let err = tool.call(args, &ctx).await.unwrap_err();
        assert!(err.to_string().contains("outside the workspace"));
    }
}
