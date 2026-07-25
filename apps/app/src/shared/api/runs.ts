// Run-ledger reads. Shared: chat re-hydrates a session from them, and the
// settings dashboard lists them.

import { apiField, apiGet } from "./request";
import type { Run, RunDetail } from "../types";

export function fetchRuns(limit: number): Promise<Run[]> {
  return apiField<Run[]>(`/api/runs?limit=${limit}`, "runs");
}

export function fetchRunDetail(id: string): Promise<RunDetail> {
  return apiGet<RunDetail>(`/api/runs/${encodeURIComponent(id)}`);
}
