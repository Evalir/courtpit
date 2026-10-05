import type { EntryView } from "@/features/leagues/entries";
import type { LeagueView } from "@/features/leagues/league";

/** Solo doubles/mixed entries an admin can pair before the draw. */
export function soloEntries(entries: readonly EntryView[]): EntryView[] {
  return entries.filter(
    (entry) => entry.status === "pending_partner" && entry.player_ids.length === 1,
  );
}

/** Adds or removes an entry from a pairing selection of at most two (the newest pick wins). */
export function togglePick(picked: readonly string[], id: string): string[] {
  if (picked.includes(id)) return picked.filter((other) => other !== id);
  return [...picked, id].slice(-2);
}

/** Whether an active league can be finished now: its season has to be over (`ends_at`). */
export function canFinish(league: LeagueView, now: Date): boolean {
  return league.status === "active" && now >= new Date(league.ends_at);
}
