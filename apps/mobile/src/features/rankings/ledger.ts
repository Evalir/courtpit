import type { components } from "@racquetcollective/api-client";

export type LedgerEntry = components["schemas"]["LedgerEntry"];

/** Rankings sum the points of the last 52 weeks (spec §12). */
export const WINDOW_DAYS = 52 * 7;

const SOURCE_LABEL: Record<string, string> = {
  league_match: "League match",
  league_season: "Season finish",
  tournament: "Tournament",
};

/** "League match", "Season finish", "Tournament" (or the raw source for a newer kind). */
export function sourceLabel(source: string): string {
  return SOURCE_LABEL[source] ?? source.replace(/_/g, " ");
}

/** Where an entry came from, when the app has a screen for it. */
export function ledgerTarget(
  entry: LedgerEntry,
):
  | { pathname: "/matches/[id]"; params: { id: string } }
  | { pathname: "/leagues/[id]"; params: { id: string } }
  | null {
  if (entry.source === "league_match") {
    return { pathname: "/matches/[id]", params: { id: entry.source_id } };
  }
  if (entry.source === "league_season") {
    return { pathname: "/leagues/[id]", params: { id: entry.source_id } };
  }
  return null;
}

/** Whether an entry still counts towards the ranking at `now`. */
export function stillCounts(entry: LedgerEntry, now: Date): boolean {
  return now.getTime() - new Date(entry.occurred_at).getTime() < WINDOW_DAYS * 86_400_000;
}

/** Entries by calendar month, newest first as the API returns them: "October 2026". */
export function byMonth(
  entries: readonly LedgerEntry[],
  locale?: string,
): { month: string; entries: LedgerEntry[] }[] {
  const format = new Intl.DateTimeFormat(locale, { month: "long", year: "numeric" });
  const groups: { month: string; entries: LedgerEntry[] }[] = [];
  for (const entry of entries) {
    const month = format.format(new Date(entry.occurred_at));
    const last = groups.at(-1);
    if (last?.month === month) last.entries.push(entry);
    else groups.push({ month, entries: [entry] });
  }
  return groups;
}

/** "+12", "0". */
export function pointsLabel(points: number): string {
  return points > 0 ? `+${points}` : String(points);
}
