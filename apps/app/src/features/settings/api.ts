import { apiField, apiGet, apiPost } from "@/shared/api/request";
import { fetchRunDetail, fetchRuns } from "@/shared/api/runs";
import type { Memory, StatusSnapshot, Task } from "@/shared/types";

/** How many runs the dashboard lists. */
export const RUNS_LIMIT = 50;

export function fetchStatus(): Promise<StatusSnapshot> {
  return apiGet<StatusSnapshot>("/api/status");
}

export function fetchTasks(): Promise<Task[]> {
  return apiField<Task[]>("/api/tasks", "tasks");
}

/** `status` filters by memory status; "" means every status. */
export function fetchMemories(status: string): Promise<Memory[]> {
  const path = status ? `/api/memories?status=${encodeURIComponent(status)}` : "/api/memories";
  return apiField<Memory[]>(path, "memories");
}

export function actOnMemory(id: string, action: string): Promise<unknown> {
  return apiPost(`/api/memories/${encodeURIComponent(id)}/${action}`, {});
}

export { fetchRunDetail, fetchRuns };
