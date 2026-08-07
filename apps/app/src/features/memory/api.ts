import { apiField, apiGet, apiPost } from "@/shared/api/request";
import type { Memory } from "@/shared/types";

/** What last night's consolidation would do, without doing it. */
export interface DreamPreview {
  promote: Memory[];
  archive: Memory[];
  candidate_count: number;
}

/** `status` filters by memory status; "" means every status. */
export function fetchMemories(status: string): Promise<Memory[]> {
  const path = status ? `/api/memories?status=${encodeURIComponent(status)}` : "/api/memories";
  return apiField<Memory[]>(path, "memories");
}

export function actOnMemory(id: string, action: string): Promise<unknown> {
  return apiPost(`/api/memories/${encodeURIComponent(id)}/${action}`, {});
}

export function fetchDream(): Promise<DreamPreview> {
  return apiGet<DreamPreview>("/api/dream");
}

export function applyDream(): Promise<unknown> {
  return apiPost("/api/dream/apply", {});
}
