import type { components } from "@courtpit/api-client";

import { formatDate } from "@/features/format";
import type { BadgeTone } from "@/ui/Badge";

export type LeagueView = components["schemas"]["LeagueView"];
type LeagueStatus = components["schemas"]["LeagueStatus"];
type MatchFormat = components["schemas"]["MatchFormat"];

export const leagueBadge: Record<LeagueStatus, { label: string; tone: BadgeTone }> = {
  draft: { label: "Draft", tone: "neutral" },
  registration: { label: "Registration", tone: "primary" },
  active: { label: "In season", tone: "success" },
  finished: { label: "Finished", tone: "neutral" },
  cancelled: { label: "Cancelled", tone: "danger" },
};

/** Order of the leagues list: what a player can act on first. */
export const leagueOrder: readonly LeagueStatus[] = [
  "registration",
  "active",
  "draft",
  "finished",
  "cancelled",
];

/** The one date that matters for a league right now. */
export function leagueTiming(league: LeagueView, now: Date, locale?: string): string {
  const date = (iso: string) => formatDate(iso, locale);
  switch (league.status) {
    case "draft":
      return "Not published yet";
    case "registration":
      return new Date(league.registration_opens_at) > now
        ? `Registration opens ${date(league.registration_opens_at)}`
        : `Registration closes ${date(league.registration_closes_at)}`;
    case "active":
      return `${date(league.starts_at)} – ${date(league.ends_at)}`;
    case "finished":
      return `Finished ${date(league.ends_at)}`;
    case "cancelled":
      return league.cancel_reason ?? "Cancelled";
  }
}

/** "Best of 3 sets · tiebreak at 6–6 · match tiebreak to 10 instead of a final set". */
export function describeFormat(format: MatchFormat): string {
  const parts: string[] = [];
  parts.push(format.sets_to_win === 1 ? "One set" : `Best of ${format.sets_to_win * 2 - 1} sets`);
  if (format.games_per_set !== 6) parts.push(`sets to ${format.games_per_set} games`);
  parts.push(
    format.tiebreak_at
      ? `tiebreak at ${format.tiebreak_at}–${format.tiebreak_at}`
      : "advantage sets",
  );
  if (format.sets_to_win > 1) {
    if (format.final_set === "match_tiebreak_10")
      parts.push("match tiebreak to 10 instead of a final set");
    if (format.final_set === "pro_set_8") parts.push("pro set to 8 as the final set");
  }
  if (format.deuce === "golden_point") parts.push("golden point at deuce");
  return parts.join(" · ");
}

/** The boxes with the viewer's own box first (a player plays in at most one), and which it is. */
export function ownBoxFirst<Box extends { table: readonly { player_ids: readonly string[] }[] }>(
  boxes: readonly Box[],
  me: string,
): { boxes: Box[]; mine: Box | null } {
  const mine = boxes.find((box) => box.table.some((line) => line.player_ids.includes(me))) ?? null;
  return { boxes: mine ? [mine, ...boxes.filter((box) => box !== mine)] : [...boxes], mine };
}
