import { useQuery, type QueryKey } from "@tanstack/react-query";

import { POLL } from "@/shared/config";

/** Every dashboard panel polls on the same cadence. */
export function usePanelQuery<T>(key: QueryKey, queryFn: () => Promise<T>) {
  return useQuery({ queryKey: key, queryFn, refetchInterval: POLL.dashboard });
}
