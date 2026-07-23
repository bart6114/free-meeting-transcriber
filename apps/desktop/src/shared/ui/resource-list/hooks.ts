import { useQuery } from "@tanstack/react-query";

// This used to fetch a hosted "web templates" gallery from the upstream
// product's own servers. That backend was removed along with the rest of
// the hosted infrastructure (this fork is local-only), so there is nothing
// left to call — return an empty result without making a network request
// to a domain this fork does not control.
export function useWebResources<T>(endpoint: string) {
  return useQuery({
    queryKey: ["settings", endpoint, "suggestions"],
    queryFn: async (): Promise<T[]> => [],
  });
}
