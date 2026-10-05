import type { components } from "@courtpit/api-client";

import vectors from "../../../../../crates/domain/testdata/score_vectors.json";

import { checkScore, nextSetKind } from "./score";

type MatchFormat = components["schemas"]["MatchFormat"];
type SetScore = components["schemas"]["SetScore"];

interface Vector {
  name: string;
  format: MatchFormat;
  sets: SetScore[];
  winner?: "a" | "b";
  error?: string;
  set?: number;
}

describe("checkScore agrees with the domain crate", () => {
  it.each((vectors as Vector[]).map((vector) => [vector.name, vector] as const))(
    "%s",
    (_name, vector) => {
      const result = checkScore(vector.format, vector.sets);
      if (vector.winner) {
        expect(result).toMatchObject({ ok: true, winner: vector.winner });
      } else {
        expect(result).toMatchObject({ ok: false, kind: vector.error });
        if (!result.ok) expect(result.set).toBe(vector.set);
      }
    },
  );
});

describe("nextSetKind", () => {
  const format: MatchFormat = {
    sets_to_win: 2,
    games_per_set: 6,
    tiebreak_at: 6,
    final_set: "match_tiebreak_10",
  };

  it("plays the deciding set as the format says", () => {
    expect(nextSetKind(format, 0, 0)).toBe("set");
    expect(nextSetKind(format, 1, 0)).toBe("set");
    expect(nextSetKind(format, 1, 1)).toBe("match_tiebreak");
    expect(nextSetKind({ ...format, final_set: "pro_set_8" }, 1, 1)).toBe("pro_set");
    expect(nextSetKind({ ...format, final_set: "full_set" }, 1, 1)).toBe("set");
    expect(nextSetKind({ ...format, sets_to_win: 1, final_set: "pro_set_8" }, 0, 0)).toBe(
      "pro_set",
    );
  });
});
