import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { ErrorBoundary } from "./error-boundary";

/** One client per window. Retries are off: every read polls on an interval, so
 *  a failed fetch resolves itself on the next tick rather than after a backoff
 *  the user can't see. */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: { retry: false, refetchOnWindowFocus: false, staleTime: 2_000 },
  },
});

export function AppProviders({ children }: { children: React.ReactNode }) {
  return (
    <ErrorBoundary>
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    </ErrorBoundary>
  );
}
