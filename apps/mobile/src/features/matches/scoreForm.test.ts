import { checkRows, rowsToSets, scoreRows } from "./scoreForm";

const bestOfThree = {
  sets_to_win: 2,
  games_per_set: 6,
  tiebreak_at: 6,
  final_set: "match_tiebreak_10" as const,
};

describe("scoreRows", () => {
  it("starts with enough sets to win", () => {
    expect(scoreRows(bestOfThree, []).map((row) => row.kind)).toEqual(["set", "set"]);
  });

  it("adds the deciding match tiebreak when the sets are split", () => {
    const rows = scoreRows(bestOfThree, [
      { a: "6", b: "4" },
      { a: "3", b: "6" },
    ]);
    expect(rows.map((row) => row.kind)).toEqual(["set", "set", "match_tiebreak"]);
  });

  it("drops rows after the match is decided", () => {
    const rows = scoreRows(bestOfThree, [
      { a: "6", b: "4" },
      { a: "6", b: "3" },
      { a: "10", b: "8" },
    ]);
    expect(rows).toHaveLength(2);
  });

  it("does not guess a decider while a set is unfinished", () => {
    expect(
      scoreRows(bestOfThree, [
        { a: "6", b: "" },
        { a: "3", b: "6" },
      ]),
    ).toHaveLength(2);
  });

  it("makes the only set of a one-set match a pro set when the format says so", () => {
    const proSet = { ...bestOfThree, sets_to_win: 1, final_set: "pro_set_8" as const };
    expect(scoreRows(proSet, []).map((row) => row.kind)).toEqual(["pro_set"]);
  });

  it("grows a best of five one set at a time", () => {
    const bestOfFive = { ...bestOfThree, sets_to_win: 3, final_set: "full_set" as const };
    const sets = [
      { a: "6", b: "4" },
      { a: "4", b: "6" },
      { a: "6", b: "4" },
      { a: "4", b: "6" },
    ];
    expect(scoreRows(bestOfFive, sets.slice(0, 3))).toHaveLength(4);
    expect(scoreRows(bestOfFive, sets)).toHaveLength(5);
  });
});

describe("rowsToSets and checkRows", () => {
  it("flags the match tiebreak and validates like the server", () => {
    const rows = scoreRows(bestOfThree, [
      { a: "6", b: "4" },
      { a: "3", b: "6" },
      { a: "7", b: "10" },
    ]);
    expect(rowsToSets(rows)).toEqual([
      { a: 6, b: 4 },
      { a: 3, b: 6 },
      { a: 7, b: 10, match_tiebreak: true },
    ]);
    expect(checkRows(bestOfThree, rows)?.check).toMatchObject({ ok: true, winner: "b" });
  });

  it("waits for every box to be filled", () => {
    expect(checkRows(bestOfThree, scoreRows(bestOfThree, [{ a: "6", b: "4" }]))).toBeNull();
    expect(rowsToSets([{ kind: "set", a: "x", b: "4" }])).toBeNull();
  });

  it("explains an impossible set", () => {
    const rows = scoreRows(bestOfThree, [
      { a: "7", b: "4" },
      { a: "6", b: "0" },
    ]);
    expect(checkRows(bestOfThree, rows)?.check).toMatchObject({
      ok: false,
      kind: "invalid_set",
      set: 1,
      message: "Set 1: 7–4 is not a valid tiebreak set.",
    });
  });
});
