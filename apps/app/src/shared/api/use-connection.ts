// Gateway liveness as a query, not a hand-rolled interval: react-query owns the
// timer, dedupes across subscribers, and gives every component the same answer.

import { useQuery } from "@tanstack/react-query";

import { POLL } from "../config";
import { qk } from "./query-keys";
import { getClient } from "./runtime";
import type { KomoConnectResponse } from "./types";

const OFFLINE: KomoConnectResponse = { connected: false };

export function useConnection(): KomoConnectResponse {
  const query = useQuery({
    queryKey: qk.connection,
    queryFn: () => getClient().connect(),
    refetchInterval: POLL.connection,
    // The probe never rejects (it reports `connected: false` instead), so a
    // stale value is never left on screen.
    staleTime: 0,
  });
  return query.data ?? OFFLINE;
}
