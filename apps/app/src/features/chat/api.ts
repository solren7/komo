// Chat's data plane: session transcript + run ledger reads, and the two
// out-of-band interaction writes. Every `/api` path this feature touches
// appears here exactly once.

import { apiField, apiPost } from "@/shared/api/request";
import { fetchRuns as fetchRunsShared, fetchRunDetail } from "@/shared/api/runs";
import type { Interactions, Run, SessionMessage } from "@/shared/types";

/** How far back the run ledger is scanned when re-hydrating a session. */
export const HISTORY_RUN_LIMIT = 500;

export const interactionsPath = (session: string) =>
  `/api/interactions/${encodeURIComponent(session)}`;

export function fetchSessionMessages(session: string): Promise<SessionMessage[]> {
  return apiField<SessionMessage[]>(
    `/api/sessions/${encodeURIComponent(session)}/messages`,
    "messages",
  );
}

export function fetchRuns(limit = HISTORY_RUN_LIMIT): Promise<Run[]> {
  return fetchRunsShared(limit);
}

export { fetchRunDetail };

export function decideApproval(
  session: string,
  decision: "once" | "session" | "deny",
): Promise<unknown> {
  return apiPost(`${interactionsPath(session)}/approval`, { decision });
}

export function answerQuestion(session: string, text: string): Promise<unknown> {
  return apiPost(`${interactionsPath(session)}/answer`, { text });
}

export type { Interactions };
