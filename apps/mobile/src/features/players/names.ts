import { useQueries } from "@tanstack/react-query";

import { useApi } from "@/api/client";
import { errorCode } from "@/api/errors";

/**
 * Display names for player ids. Matches, entries and standings carry ids only, so each id is
 * fetched once with `GET /players/{id}` and cached for ten minutes; TanStack Query dedupes the
 * requests across every card on screen.
 *
 * Returns a lookup: the viewer is "You", a player who left or is hidden is "Former member",
 * and a name still loading is an ellipsis.
 */
export function usePlayerNames(ids: readonly string[], me?: string): (id: string) => string {
  const { $api } = useApi();
  const unique = [...new Set(ids)].filter((id) => id !== me);
  const results = useQueries({
    queries: unique.map((id) =>
      $api.queryOptions(
        "get",
        "/api/v1/players/{id}",
        { params: { path: { id } } },
        // A missing player stays missing; don't retry or report it as an outage.
        { staleTime: 10 * 60_000, retry: false },
      ),
    ),
  });
  const names = new Map<string, string>();
  unique.forEach((id, index) => {
    const result = results[index];
    if (result?.data) names.set(id, result.data.display_name);
    else if (result?.error && errorCode(result.error) === "not_found")
      names.set(id, "Former member");
  });
  return (id) => (id === me ? "You" : (names.get(id) ?? "…"));
}

/** "You & Ben Ortiz", "Ana Ruiz". */
export function sideName(players: readonly string[], name: (id: string) => string): string {
  return players.map(name).join(" & ");
}
