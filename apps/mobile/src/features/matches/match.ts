import type { components } from "@courtpit/api-client";

import type { BadgeTone } from "@/ui/Badge";

export type MatchView = components["schemas"]["MatchView"];
export type MatchStatus = components["schemas"]["MatchStatus"];
type Score = components["schemas"]["Score"];
type Side = components["schemas"]["Side"];

/** How each status reads in a badge. */
export const statusBadge: Record<MatchStatus, { label: string; tone: BadgeTone }> = {
  proposed: { label: "To arrange", tone: "neutral" },
  scheduled: { label: "Scheduled", tone: "primary" },
  reported: { label: "Awaiting confirmation", tone: "warning" },
  confirmed: { label: "Confirmed", tone: "success" },
  disputed: { label: "Disputed", tone: "danger" },
  resolved: { label: "Resolved", tone: "success" },
  walkover: { label: "Walkover", tone: "neutral" },
  cancelled: { label: "Cancelled", tone: "neutral" },
};

/** The side `player` plays on, if any. */
export function sideOf(match: MatchView, player: string): Side | null {
  if (match.side_a.includes(player)) return "a";
  if (match.side_b.includes(player)) return "b";
  return null;
}

/**
 * A score as people say it, from one side's point of view: `6–4 3–6 [10–7]` (a match tiebreak
 * in brackets). `side` defaults to A; pass the viewer's side to put their games first.
 */
export function formatScore(score: Score, side: Side = "a"): string {
  return score.sets
    .map((set) => {
      const [mine, theirs] = side === "a" ? [set.a, set.b] : [set.b, set.a];
      const text = `${mine}–${theirs}`;
      return set.match_tiebreak ? `[${text}]` : text;
    })
    .join(" ");
}

/** What the signed-in player should do about a match, if anything. */
export type MatchAction = "confirm" | "report" | "arrange" | null;

/**
 * - `confirm`: the other side reported a score; confirm or dispute it before the deadline.
 * - `report`: a scheduled match whose time has passed has no score yet.
 * - `arrange`: nobody has agreed a time yet.
 */
export function actionFor(match: MatchView, me: string, now: Date): MatchAction {
  const mine = sideOf(match, me);
  if (!mine) return null;
  switch (match.status) {
    case "reported": {
      const reporterSide = match.reported_by ? sideOf(match, match.reported_by) : null;
      return reporterSide !== mine ? "confirm" : null;
    }
    case "scheduled":
      return match.scheduled_at && new Date(match.scheduled_at) <= now ? "report" : null;
    case "proposed":
      return "arrange";
    default:
      return null;
  }
}

/** The home screen's sections, each in display order. */
export interface MatchGroups {
  /** Waiting on the viewer: confirm a score or report a played match. */
  needsYou: MatchView[];
  /** Scheduled and still ahead, soonest first. */
  upcoming: MatchView[];
  /** Agreed by nobody yet. */
  toArrange: MatchView[];
  /** Waiting on someone else: the opponent's confirmation or an admin's ruling. */
  waiting: MatchView[];
  /** Results, newest first. */
  results: MatchView[];
}

const RESULT: ReadonlySet<MatchStatus> = new Set(["confirmed", "resolved", "walkover"]);

/** Sorts the viewer's matches into the home screen's sections; cancelled ones are dropped. */
export function groupMatches(matches: readonly MatchView[], me: string, now: Date): MatchGroups {
  const groups: MatchGroups = {
    needsYou: [],
    upcoming: [],
    toArrange: [],
    waiting: [],
    results: [],
  };
  for (const match of matches) {
    const action = actionFor(match, me, now);
    if (action === "confirm" || action === "report") groups.needsYou.push(match);
    else if (action === "arrange") groups.toArrange.push(match);
    else if (match.status === "scheduled") groups.upcoming.push(match);
    else if (match.status === "reported" || match.status === "disputed") groups.waiting.push(match);
    else if (RESULT.has(match.status)) groups.results.push(match);
  }
  const time = (value: string | null | undefined) => (value ? Date.parse(value) : 0);
  groups.upcoming.sort((x, y) => time(x.scheduled_at) - time(y.scheduled_at));
  groups.results.sort(
    (x, y) => time(y.reported_at ?? y.scheduled_at) - time(x.reported_at ?? x.scheduled_at),
  );
  return groups;
}
