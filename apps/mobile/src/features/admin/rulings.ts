import type { components } from "@courtpit/api-client";

import type { MatchView } from "@/features/matches/match";

import { isAdmin } from "./roles";

type PlayerRole = components["schemas"]["PlayerRole"];

/** What a club admin may decide on a match (all false for members). */
export interface AdminActions {
  /** Decide a disputed match: set the score, order a replay, or void it. */
  resolve: boolean;
  /** Award an unplayed league match to one side. */
  walkover: boolean;
  /** Call off a match not yet played (players cancel their own friendlies themselves). */
  cancel: boolean;
  /** Why an admin can't referee here, if that is the reason nothing is offered. */
  blocked: string | null;
}

const NONE: AdminActions = { resolve: false, walkover: false, cancel: false, blocked: null };

/**
 * Mirrors the server: admins referee only matches they don't play in (an owner may, so a club
 * with one admin is never stuck); walkovers are for league matches.
 */
export function adminActions(
  match: MatchView,
  viewer: { id: string; role: PlayerRole },
): AdminActions {
  if (!isAdmin(viewer.role)) return NONE;
  const plays = [...match.side_a, ...match.side_b].includes(viewer.id);
  const open = match.status === "proposed" || match.status === "scheduled";
  const referee = !plays || viewer.role === "owner";
  const actions: AdminActions = {
    resolve: referee && match.status === "disputed",
    walkover: referee && open && match.league_id != null,
    cancel: !plays && open,
    blocked: null,
  };
  if (plays && !referee && (match.status === "disputed" || (open && match.league_id != null))) {
    actions.blocked = "You play in this match, so another admin or the owner decides it.";
  }
  return actions;
}
