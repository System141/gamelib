import { QueryClient } from "@tanstack/react-query";

// Data only changes when a sync or a link edit says so, so never refetch on focus or staleness.
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: Number.POSITIVE_INFINITY,
      refetchOnWindowFocus: false,
      retry: 1,
    },
  },
});
