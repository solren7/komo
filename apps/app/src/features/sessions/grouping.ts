// Grouping the session list by workspace. Kept out of the component (which
// imports the store, and the store touches the DOM) so it stays unit-testable.

import type { SessionSummary, WorkspaceInfo } from "@/shared/types";

export const DEFAULT_WORKSPACE = "__default__";

export function workspaceLabel(id: string, workspaces: WorkspaceInfo[]): string {
  return (
    workspaces.find((workspace) => workspace.id === id)?.name ??
    (id === DEFAULT_WORKSPACE ? "默认 workspace" : id)
  );
}

export interface WorkspaceGroup {
  workspace: string;
  label: string;
  entries: SessionSummary[];
}

/** Sessions grouped by their (immutable) workspace, newest group first.
 *
 *  Groups are ordered by their most recent session rather than by workspace name:
 *  the sidebar is a recency list, and sorting the *groups* alphabetically would
 *  bury whichever project is being worked on. Within a group the incoming order
 *  is preserved. A session from a pre-workspace gateway counts as the default. */
export function groupByWorkspace(
  sessions: SessionSummary[],
  workspaces: WorkspaceInfo[],
): WorkspaceGroup[] {
  const groups = new Map<string, SessionSummary[]>();
  for (const item of sessions) {
    const workspace = item.workspace ?? DEFAULT_WORKSPACE;
    const entries = groups.get(workspace);
    if (entries) entries.push(item);
    else groups.set(workspace, [item]);
  }
  return Array.from(groups)
    .map(([workspace, entries]) => ({
      workspace,
      label: workspaceLabel(workspace, workspaces),
      entries,
      newest: Math.max(...entries.map((entry) => entry.created_at)),
    }))
    .sort((a, b) => b.newest - a.newest)
    .map(({ newest: _newest, ...group }) => group);
}
