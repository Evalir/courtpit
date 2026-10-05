import type { components } from "@racquetcollective/api-client";

import type { BadgeTone } from "@/ui/Badge";

import type { LeagueView } from "./league";

export type EntryView = components["schemas"]["EntryView"];
export type MyEntry = components["schemas"]["MyEntry"];
type Discipline = components["schemas"]["Discipline"];
type Gender = components["schemas"]["Gender"];

/** Whether the league takes entries right now (the server's `registration_open`). */
export function registrationOpen(league: LeagueView, now: Date): boolean {
  return (
    league.status === "registration" &&
    now >= new Date(league.registration_opens_at) &&
    now < new Date(league.registration_closes_at)
  );
}

/** Where the viewer stands with an entry they play in or are invited to. */
export type EntryState =
  /** Complete: counted in the league. */
  | { kind: "entered"; partner: string | null }
  /** Their invitation is out to `partner`. */
  | { kind: "waiting"; partner: string }
  /** Listed for others looking for a partner. */
  | { kind: "looking" }
  /** Solo with nobody asked: the invitation was declined or lapsed. */
  | { kind: "no_partner" }
  /** `by` invites the viewer to complete their entry. */
  | { kind: "invited"; by: string };

/** The viewer's relation to `entry`, or null when it is not theirs (or is withdrawn). */
export function entryState(entry: EntryView, me: string): EntryState | null {
  if (entry.status === "withdrawn") return null;
  if (!entry.player_ids.includes(me)) {
    return entry.invited_partner_id === me ? { kind: "invited", by: entry.created_by } : null;
  }
  if (entry.status !== "pending_partner") {
    return { kind: "entered", partner: entry.player_ids.find((id) => id !== me) ?? null };
  }
  if (entry.invited_partner_id != null) {
    return { kind: "waiting", partner: entry.invited_partner_id };
  }
  return entry.looking_for_partner ? { kind: "looking" } : { kind: "no_partner" };
}

/** A league's entries from the viewer's side. */
export interface LeagueEntries {
  /** The viewer's own entry (a player has at most one live entry per league). */
  own: EntryView | null;
  /** Other players' entries inviting the viewer. */
  invitations: EntryView[];
  /** Solo entries of other players looking for a partner. */
  looking: EntryView[];
  /** Complete entries, the viewer's included. */
  confirmed: number;
}

export function leagueEntries(entries: readonly EntryView[], me: string): LeagueEntries {
  const live = entries.filter((entry) => entry.status !== "withdrawn");
  return {
    own: live.find((entry) => entry.player_ids.includes(me)) ?? null,
    invitations: live.filter((entry) => entryState(entry, me)?.kind === "invited"),
    looking: live.filter(
      (entry) =>
        entry.status === "pending_partner" &&
        entry.looking_for_partner &&
        entry.player_ids.length === 1 &&
        !entry.player_ids.includes(me),
    ),
    confirmed: live.filter(
      (entry) => entry.status === "confirmed" || entry.status === "pending_payment",
    ).length,
  };
}

/**
 * The entries on Home: invitations to answer and entries that lost their partner, while
 * registration is still open.
 */
export function entriesNeedingYou(mine: readonly MyEntry[], me: string, now: Date): MyEntry[] {
  return mine.filter((item) => {
    const kind = entryState(item.entry, me)?.kind;
    return (kind === "invited" || kind === "no_partner") && registrationOpen(item.league, now);
  });
}

/**
 * Why the viewer cannot enter a mixed league, when the profile already shows it. An
 * undisclosed gender never qualifies; whether `other` does is the community's rule, which
 * only the server knows, so its answer is shown instead.
 */
export function mixedBlocker(discipline: Discipline, gender: Gender | undefined): string | null {
  if (discipline !== "mixed" || gender !== "undisclosed") return null;
  return "Mixed doubles pairs players by gender, so your profile needs one before you can enter.";
}

/** Badges for the Leagues list: the viewer's own entry outranks invitations to them. */
export function entryBadges(
  mine: readonly MyEntry[],
  me: string,
): Map<string, { label: string; tone: BadgeTone }> {
  const badges = new Map<string, { label: string; tone: BadgeTone }>();
  for (const { entry, league } of mine) {
    const kind = entryState(entry, me)?.kind;
    if (kind === "entered") badges.set(league.id, { label: "You’re in", tone: "success" });
    else if (kind === "invited") {
      if (!badges.has(league.id)) badges.set(league.id, { label: "Invited", tone: "accent" });
    } else if (kind) badges.set(league.id, { label: "Entry pending", tone: "warning" });
  }
  return badges;
}
