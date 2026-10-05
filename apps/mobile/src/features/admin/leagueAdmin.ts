import type { LeagueView } from "@/features/leagues/league";

/** What an admin can do with a league in its current state. */
export interface LeagueAdmin {
  edit: boolean;
  publish: boolean;
  cancel: boolean;
}

/**
 * Mirrors the server: drafts can be edited (published or not, until registration opens) and
 * published once; anything not finished or cancelled can be cancelled.
 */
export function leagueAdmin(league: LeagueView): LeagueAdmin {
  const draft = league.status === "draft";
  return {
    edit: draft,
    publish: draft && league.published_at == null,
    cancel: league.status !== "finished" && league.status !== "cancelled",
  };
}
