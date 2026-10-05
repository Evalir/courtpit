import type { components } from "@racquetcollective/api-client";

type PlayerRef = components["schemas"]["PlayerRef"];

/**
 * A name for every player id a view mentions. Matches, standings lines, entries and match
 * requests carry `names` (decision 85), so no screen fetches players one by one. The viewer
 * reads "You"; an id the view did not name (a player removed from the community) reads
 * "Former member".
 */
export function nameLookup(refs: Iterable<PlayerRef>, me?: string): (id: string) => string {
  const names = new Map<string, string>();
  for (const ref of refs) names.set(ref.id, ref.display_name);
  return (id) => (id === me ? "You" : (names.get(id) ?? "Former member"));
}

/** "You & Ben Ortiz", "Ana Ruiz". */
export function sideName(players: readonly string[], name: (id: string) => string): string {
  return players.map(name).join(" & ");
}
