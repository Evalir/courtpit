import type { components } from "@racquetcollective/api-client";

import type { LeagueView } from "@/features/leagues/league";

import { addDays, endOf, lastDayBefore, dayOf, startOf, type Day } from "@/ui/calendar";

type Discipline = components["schemas"]["Discipline"];
type MatchFormat = components["schemas"]["MatchFormat"];
type CreateLeague = components["schemas"]["CreateLeague"];
type PatchLeague = components["schemas"]["PatchLeague"];

/**
 * How the form sets the match format: leave it (editing), the club's default, or its own. The
 * API shows a league's effective format without saying whether it is an override.
 */
export type FormatChoice =
  { kind: "keep" } | { kind: "default" } | { kind: "custom"; format: MatchFormat };

/**
 * A league as the admin edits it. Days are local calendar days: registration opens at the start
 * of `opens` and closes at the end of `closes`; the season runs from the start of `starts` to
 * the end of `ends`.
 */
export interface LeagueForm {
  name: string;
  discipline: Discipline;
  opens: Day;
  closes: Day;
  starts: Day;
  ends: Day;
  boxMin: number;
  boxMax: number;
  format: FormatChoice;
  previousLeagueId: string | null;
}

/** Two weeks of registration from today, the season two days later for twelve weeks. */
export function newLeagueForm(today: Day): LeagueForm {
  const closes = addDays(today, 13);
  const starts = addDays(closes, 2);
  return {
    name: "",
    discipline: "singles",
    opens: today,
    closes,
    starts,
    ends: addDays(starts, 12 * 7 - 1),
    boxMin: 6,
    boxMax: 8,
    format: { kind: "default" },
    previousLeagueId: null,
  };
}

export function formFromLeague(league: LeagueView): LeagueForm {
  return {
    name: league.name,
    discipline: league.discipline,
    opens: dayOf(new Date(league.registration_opens_at)),
    closes: lastDayBefore(new Date(league.registration_closes_at)),
    starts: dayOf(new Date(league.starts_at)),
    ends: lastDayBefore(new Date(league.ends_at)),
    boxMin: league.box_min_size,
    boxMax: league.box_max_size,
    format: { kind: "keep" },
    previousLeagueId: league.previous_league_id ?? null,
  };
}

export type LeagueProblems = Partial<Record<"name" | "dates" | "boxes", string>>;

/** The server's rules, in the form's words. */
export function checkLeague(form: LeagueForm): LeagueProblems {
  const problems: LeagueProblems = {};
  const name = form.name.trim();
  if (name === "" || [...name].length > 80) problems.name = "A name of 1 to 80 characters.";
  if (form.closes < form.opens) {
    problems.dates = "Registration has to close on or after the day it opens.";
  } else if (form.starts <= form.closes) {
    problems.dates = "The season starts after registration closes.";
  } else if (form.ends < form.starts) {
    problems.dates = "The season ends on or after the day it starts.";
  }
  if (!(form.boxMin >= 2 && form.boxMin <= form.boxMax && form.boxMax <= 16)) {
    problems.boxes = "Boxes hold 2 to 16 entries, the smallest size first.";
  }
  return problems;
}

function instants(form: LeagueForm) {
  return {
    registration_opens_at: startOf(form.opens).toISOString(),
    registration_closes_at: endOf(form.closes).toISOString(),
    starts_at: startOf(form.starts).toISOString(),
    ends_at: endOf(form.ends).toISOString(),
  };
}

export function createBody(form: LeagueForm): CreateLeague {
  return {
    name: form.name.trim(),
    discipline: form.discipline,
    ...instants(form),
    box_min_size: form.boxMin,
    box_max_size: form.boxMax,
    match_format: form.format.kind === "custom" ? form.format.format : null,
    previous_league_id: form.previousLeagueId,
  };
}

/** Only what changed, so an untouched draft stays exactly as it was. */
export function patchBody(league: LeagueView, form: LeagueForm): PatchLeague {
  const before = formFromLeague(league);
  const patch: PatchLeague = {};
  if (form.name.trim() !== before.name) patch.name = form.name.trim();
  const dates = instants(form);
  if (form.opens !== before.opens) patch.registration_opens_at = dates.registration_opens_at;
  if (form.closes !== before.closes) patch.registration_closes_at = dates.registration_closes_at;
  if (form.starts !== before.starts) patch.starts_at = dates.starts_at;
  if (form.ends !== before.ends) patch.ends_at = dates.ends_at;
  if (form.boxMin !== before.boxMin) patch.box_min_size = form.boxMin;
  if (form.boxMax !== before.boxMax) patch.box_max_size = form.boxMax;
  if (form.format.kind === "default") patch.match_format = null;
  if (form.format.kind === "custom") patch.match_format = form.format.format;
  if (form.previousLeagueId !== before.previousLeagueId) {
    patch.previous_league_id = form.previousLeagueId;
  }
  return patch;
}

/** A custom format to start from: best of three with a match tiebreak, tiebreaks at 6–6. */
export const STARTER_FORMAT: MatchFormat = {
  sets_to_win: 2,
  games_per_set: 6,
  tiebreak_at: 6,
  final_set: "match_tiebreak_10",
  deuce: "advantage",
};
