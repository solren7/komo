import { apiField } from "@/shared/api/request";
import type { WorkspaceInfo } from "@/shared/types";

export function fetchWorkspaces(): Promise<WorkspaceInfo[]> {
  return apiField<WorkspaceInfo[]>("/api/workspaces", "workspaces");
}
