import type { components } from "@courtpit/api-client";

import { checkScore, nextSetKind, type ScoreCheck, type SetKind } from "./score";

type MatchFormat = components["schemas"]["MatchFormat"];
type SetScore = components["schemas"]["SetScore"];

/** What the player typed for one set: games (or points) for side A and side B. */
export interface RowInput {
  a: string;
  b: string;
}

/** A row the score form shows: the set's kind and what was typed. */
export interface ScoreRow extends RowInput {
  kind: SetKind;
}

function parse(value: string): number | null {
  const trimmed = value.trim();
  return /^\d{1,2}$/.test(trimmed) ? Number(trimmed) : null;
}

/**
 * The rows the form shows for `format`: enough sets to win the match, then one more each time
 * the sets typed so far leave it undecided, and none after a side has won. A row's kind (set,
 * pro set or match tiebreak) follows the sets won before it.
 */
export function scoreRows(format: MatchFormat, inputs: readonly RowInput[]): ScoreRow[] {
  const toWin = format.sets_to_win;
  const rows: ScoreRow[] = [];
  let setsA = 0;
  let setsB = 0;
  let known = true;
  for (let index = 0; index < toWin * 2 - 1; index += 1) {
    if (setsA === toWin || setsB === toWin) break;
    if (index >= toWin && !known) break;
    const input = inputs[index] ?? { a: "", b: "" };
    rows.push({ kind: known ? nextSetKind(format, setsA, setsB) : "set", ...input });
    const games = [parse(input.a), parse(input.b)] as const;
    if (games[0] === null || games[1] === null || games[0] === games[1]) {
      known = false;
    } else if (games[0] > games[1]) {
      setsA += 1;
    } else {
      setsB += 1;
    }
  }
  return rows;
}

/** The rows as the API's sets, or `null` while any row has a missing or non-numeric value. */
export function rowsToSets(rows: readonly ScoreRow[]): SetScore[] | null {
  const sets: SetScore[] = [];
  for (const row of rows) {
    const a = parse(row.a);
    const b = parse(row.b);
    if (a === null || b === null) return null;
    sets.push(row.kind === "match_tiebreak" ? { a, b, match_tiebreak: true } : { a, b });
  }
  return sets;
}

/** The form's verdict: incomplete input, or the domain's check of the typed score. */
export function checkRows(
  format: MatchFormat,
  rows: readonly ScoreRow[],
): { sets: SetScore[]; check: ScoreCheck } | null {
  const sets = rowsToSets(rows);
  return sets ? { sets, check: checkScore(format, sets) } : null;
}
