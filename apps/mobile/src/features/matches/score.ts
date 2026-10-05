import type { components } from "@courtpit/api-client";

type MatchFormat = components["schemas"]["MatchFormat"];
type SetScore = components["schemas"]["SetScore"];
type Side = components["schemas"]["Side"];

/**
 * A TypeScript port of the domain crate's `validate_score` (crates/domain/src/score.rs), so the
 * score form can say what is wrong while the player types. The server stays the authority; both
 * implementations run the shared cases in `crates/domain/testdata/score_vectors.json`.
 */

/** Why a score is not legal under a format (the domain's `ScoreError` variants). */
export type ScoreErrorKind =
  | "format"
  | "empty"
  | "invalid_set"
  | "match_tiebreak_expected"
  | "match_tiebreak_not_allowed"
  | "after_match_decided"
  | "incomplete";

export type ScoreCheck =
  | { ok: true; winner: Side; setsA: number; setsB: number }
  | { ok: false; kind: ScoreErrorKind; set?: number; message: string };

/** What the next set is, given how many sets each side has won so far. */
export type SetKind = "set" | "pro_set" | "match_tiebreak";

export function formatProblem(format: MatchFormat): string | null {
  if (format.sets_to_win < 1 || format.sets_to_win > 3) return "sets_to_win must be 1, 2 or 3";
  if (format.games_per_set < 1 || format.games_per_set > 9) {
    return "games_per_set must be between 1 and 9";
  }
  if (format.tiebreak_at != null && format.tiebreak_at !== format.games_per_set) {
    return "tiebreak_at must equal games_per_set when set";
  }
  return null;
}

/** The kind of the next set once side A has won `setsA` sets and side B `setsB`. */
export function nextSetKind(format: MatchFormat, setsA: number, setsB: number): SetKind {
  const deciding = setsA === format.sets_to_win - 1 && setsB === format.sets_to_win - 1;
  if (deciding && format.final_set === "match_tiebreak_10") return "match_tiebreak";
  if (deciding && format.final_set === "pro_set_8") return "pro_set";
  return "set";
}

/** A normal set to `games`, with or without a tiebreak at `games–games`. */
function validSet(games: number, tiebreak: boolean, won: number, lost: number): boolean {
  if (tiebreak) {
    return (
      (won === games && lost + 2 <= games) ||
      (won === games + 1 && (lost === games - 1 || lost === games))
    );
  }
  return won >= games && won - lost >= 2 && (won === games || won - lost === 2);
}

/** First to 10 points, win by two. */
function validMatchTiebreak(won: number, lost: number): boolean {
  return won >= 10 && won - lost >= 2 && (won === 10 || won - lost === 2);
}

const kindName: Record<SetKind, (format: MatchFormat) => string> = {
  set: (format) => (format.tiebreak_at != null ? "tiebreak set" : "advantage set"),
  pro_set: () => "pro set to 8",
  match_tiebreak: () => "match tiebreak (first to 10, win by 2)",
};

/** Validates `sets` against `format` and derives the winner, exactly as the server does. */
export function checkScore(format: MatchFormat, sets: readonly SetScore[]): ScoreCheck {
  const problem = formatProblem(format);
  if (problem) return { ok: false, kind: "format", message: `Invalid match format: ${problem}.` };
  if (sets.length === 0) return { ok: false, kind: "empty", message: "Enter at least one set." };
  const toWin = format.sets_to_win;
  const tiebreak = format.tiebreak_at != null;
  let setsA = 0;
  let setsB = 0;
  for (const [index, set] of sets.entries()) {
    const number = index + 1;
    if (setsA === toWin || setsB === toWin) {
      return {
        ok: false,
        kind: "after_match_decided",
        set: number,
        message: `The match was already decided before set ${number}.`,
      };
    }
    const kind = nextSetKind(format, setsA, setsB);
    const [won, lost] = set.a >= set.b ? [set.a, set.b] : [set.b, set.a];
    const invalid = (): ScoreCheck => ({
      ok: false,
      kind: "invalid_set",
      set: number,
      message: `Set ${number}: ${set.a}–${set.b} is not a valid ${kindName[kind](format)}.`,
    });
    if (kind === "match_tiebreak") {
      if (!set.match_tiebreak) {
        return {
          ok: false,
          kind: "match_tiebreak_expected",
          set: number,
          message: `Set ${number} is a match tiebreak.`,
        };
      }
      if (!validMatchTiebreak(won, lost)) return invalid();
    } else if (set.match_tiebreak) {
      return {
        ok: false,
        kind: "match_tiebreak_not_allowed",
        set: number,
        message: `Set ${number} cannot be a match tiebreak.`,
      };
    } else if (kind === "pro_set") {
      if (!validSet(8, tiebreak, won, lost)) return invalid();
    } else if (!validSet(format.games_per_set, tiebreak, won, lost)) {
      return invalid();
    }
    if (set.a >= set.b) setsA += 1;
    else setsB += 1;
  }
  if (setsA === toWin) return { ok: true, winner: "a", setsA, setsB };
  if (setsB === toWin) return { ok: true, winner: "b", setsA, setsB };
  return {
    ok: false,
    kind: "incomplete",
    message: `Not finished yet: a side needs ${toWin} ${toWin === 1 ? "set" : "sets"} to win.`,
  };
}
