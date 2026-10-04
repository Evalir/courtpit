import type { components } from "@courtpit/api-client";

export type PlayerName = components["schemas"]["PlayerName"];

/**
 * Display names for player ids. Matches, standings, entries and match requests embed
 * `{ id, display_name }` for every player they name (`side_a_names`, `player_names`, …), so
 * this needs no requests: pass those lists in.
 *
 * Returns a lookup: the viewer is "You", and a player the viewer may not see (the API sends a
 * null name for someone who left, was banned or is unverified) is "Former member", as is an id
 * the response did not name.
 */
export function playerNames(players: readonly PlayerName[], me?: string): (id: string) => string {
  const names = new Map<string, string>();
  for (const player of players) {
    if (player.display_name) names.set(player.id, player.display_name);
  }
  return (id) => (id === me ? "You" : (names.get(id) ?? "Former member"));
}

/** "You & Ben Ortiz", "Ana Ruiz". */
export function sideName(players: readonly string[], name: (id: string) => string): string {
  return players.map(name).join(" & ");
}
