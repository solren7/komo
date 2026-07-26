//! Shared plumbing for the filesystem tools (`read`, `write`, and — next —
//! `edit` / `apply_patch`).
//!
//! Three things every one of them needs, in the same order every time:
//! resolve the model's path against the workspace, ask the approver with the
//! right [`ActionRef`] (so `[policy]` rules keep matching on category/access),
//! and turn a refusal into text the model can act on.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::{
    approval::{ActionRef, ApprovalRequest, Decision},
    context::ToolContext,
    tool::ToolError,
    workspace::Workspace,
};

/// Resolve a model-supplied path inside `workspace`. Relative paths anchor to
/// the workspace root; anything that lands outside it is refused as
/// [`ToolError::Denied`] — the workspace whitelist is a floor, not a prompt (no
/// approval unlocks it, matching `shell`'s hardline patterns).
pub fn resolve(workspace: &Arc<Workspace>, path: &str) -> Result<PathBuf, ToolError> {
    workspace.resolve_contained(Path::new(path)).ok_or_else(|| {
        ToolError::Denied(format!(
            "path `{path}` is outside the workspace and was blocked. \
                 Only paths under {} are available.",
            workspace
                .roots()
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

/// Consult the approver for a **read**. Reads are `Risk::Safe`, so an
/// interactive approver never prompts — but a `category = "file", access =
/// "read"` deny rule still blackholes the path (the exfiltration guard). Returns
/// the refusal text when blocked.
pub async fn allow_read(ctx: &ToolContext, path: &Path) -> Option<String> {
    let request =
        ApprovalRequest::safe(format!("read {}", path.display())).with_action(ActionRef::File {
            path: path.to_path_buf(),
            write: false,
        });
    match ctx.decide(&request).await {
        Decision::Allow => None,
        Decision::Deny { feedback } => Some(match feedback {
            Some(reason) => format!(
                "Read of {} blocked: {reason}. Nothing was read.",
                path.display()
            ),
            None => format!(
                "Read of {} blocked by the permission policy; nothing was read.",
                path.display()
            ),
        }),
    }
}

/// Consult the approver for a **write** (`Risk::Normal` — it prompts).
/// `summary` describes the mutation for the human. Returns the refusal text,
/// carrying the user's reason when they gave one, when denied.
pub async fn allow_write(ctx: &ToolContext, path: &Path, summary: String) -> Option<String> {
    let request = ApprovalRequest::normal(summary)
        .with_scope_key("file:write")
        .with_action(ActionRef::File {
            path: path.to_path_buf(),
            write: true,
        });
    match ctx.decide(&request).await {
        Decision::Allow => None,
        Decision::Deny { feedback } => Some(match feedback {
            Some(reason) => format!(
                "Rejected by the user; {} was not changed. They said: {reason}",
                path.display()
            ),
            None => format!("Rejected by user; {} was not changed.", path.display()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(root: &str) -> Arc<Workspace> {
        Arc::new(Workspace::new(vec![PathBuf::from(root)]))
    }

    #[test]
    fn relative_paths_anchor_to_the_workspace_root() {
        let resolved = resolve(&ws("/home/u/p"), "src/main.rs").unwrap();
        assert_eq!(resolved, PathBuf::from("/home/u/p/src/main.rs"));
    }

    #[test]
    fn escapes_are_denied_not_merely_reported() {
        let err = resolve(&ws("/home/u/p"), "../secret").unwrap_err();
        assert!(matches!(err, ToolError::Denied(_)));
        // The message names the allowed root so the model can retry sensibly.
        assert!(err.to_string().contains("/home/u/p"));
    }
}
