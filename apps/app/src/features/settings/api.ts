import { apiField, apiGet } from "@/shared/api/request";
import { fetchRunDetail, fetchRuns } from "@/shared/api/runs";
import type { StatusSnapshot, Task } from "@/shared/types";

/** How many runs the dashboard lists. */
export const RUNS_LIMIT = 50;

export function fetchStatus(): Promise<StatusSnapshot> {
  return apiGet<StatusSnapshot>("/api/status");
}

export function fetchTasks(): Promise<Task[]> {
  return apiField<Task[]>("/api/tasks", "tasks");
}

export { fetchRunDetail, fetchRuns };
