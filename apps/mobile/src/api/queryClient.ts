import { isApiError } from "@courtpit/api-client";
import { MutationCache, QueryCache, QueryClient, type Query } from "@tanstack/react-query";

import { errorCode } from "./errors";

const expiredListeners = new Set<() => void>();

/** Called whenever any request answers `unauthorized`: the session ended server-side. */
export function onSessionExpired(listener: () => void): () => void {
  expiredListeners.add(listener);
  return () => {
    expiredListeners.delete(listener);
  };
}

function notifyIfExpired(error: unknown) {
  if (errorCode(error) === "unauthorized") {
    for (const listener of expiredListeners) listener();
  }
}

/** The path of an `openapi-react-query` query (keys are `[method, path, init]`). */
export function queryPath(query: Pick<Query, "queryKey">): unknown {
  return query.queryKey[1];
}

/**
 * After a write: refetch everything that depends on the player's data. A confirmed league match
 * moves standings and rankings too, so this is deliberately broad; the community and the
 * session itself are left alone.
 */
export function refreshAfterWrite(queryClient: QueryClient): Promise<void> {
  return queryClient.invalidateQueries({
    predicate: (query) =>
      queryPath(query) !== "/api/v1/tenant" && queryPath(query) !== "/api/v1/auth/session",
  });
}

/**
 * Query defaults: API errors are answers, not glitches, so only network failures are retried;
 * data counts as fresh for 30 s, and screens refetch when the app comes back to the foreground
 * (there is no realtime in v1, spec §15).
 */
export function createQueryClient(): QueryClient {
  return new QueryClient({
    queryCache: new QueryCache({ onError: notifyIfExpired }),
    mutationCache: new MutationCache({ onError: notifyIfExpired }),
    defaultOptions: {
      queries: {
        staleTime: 30_000,
        retry: (failures, error) => failures < 2 && !isApiError(error),
      },
      mutations: { retry: false },
    },
  });
}
