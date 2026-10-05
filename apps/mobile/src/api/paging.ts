import { useInfiniteQuery } from "@tanstack/react-query";

/** An `openapi-fetch` result: data on success, the error body otherwise. */
export function unwrap<T>(result: { data?: T; error?: unknown }): T {
  if (result.error !== undefined) throw result.error;
  if (result.data === undefined) throw new Error("empty response");
  return result.data;
}

/** A page of a cursor-paginated list (`{ items, next_cursor }`). */
export interface Page<T> {
  items: T[];
  next_cursor?: string | null;
}

/**
 * A cursor-paginated list as one growing array (`?cursor=&limit=`). `openapi-react-query`'s
 * `useInfiniteQuery` sends `cursor=0` for the first page, which the API rejects, so lists page
 * with TanStack Query directly. `key` should mirror openapi-react-query's
 * `["get", path, init]` so invalidating by path catches these too.
 */
export function useCursorList<T>(
  key: readonly unknown[],
  fetchPage: (cursor: string | undefined, signal: AbortSignal) => Promise<Page<T>>,
) {
  const query = useInfiniteQuery({
    queryKey: key,
    queryFn: ({ pageParam, signal }) => fetchPage(pageParam, signal),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
  });
  const items = query.data?.pages.flatMap((page) => page.items) ?? [];
  return { ...query, items };
}
