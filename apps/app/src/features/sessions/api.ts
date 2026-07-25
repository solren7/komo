import { apiField, apiPost } from "@/shared/api/request";
import type { SessionSummary } from "@/shared/types";

export function fetchSessions(): Promise<SessionSummary[]> {
  return apiField<SessionSummary[]>("/api/sessions", "sessions");
}

export function renameSession(id: string, title: string): Promise<unknown> {
  return apiPost(`/api/sessions/${encodeURIComponent(id)}/title`, { title });
}

/** "active" | "archive" | "deleted" (deleted is a soft delete server-side). */
export function setSessionStatus(id: string, status: string): Promise<unknown> {
  return apiPost(`/api/sessions/${encodeURIComponent(id)}/status`, { status });
}
