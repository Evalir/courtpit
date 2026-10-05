import { actionFor, formatScore, groupMatches, sideOf, type MatchView } from "./match";

const ME = "00000000-0000-7000-8000-000000000001";
const PARTNER = "00000000-0000-7000-8000-000000000002";
const OPP = "00000000-0000-7000-8000-000000000003";
const OPP2 = "00000000-0000-7000-8000-000000000004";
const NOW = new Date("2026-10-04T12:00:00Z");

let counter = 0;
function match(overrides: Partial<MatchView>): MatchView {
  counter += 1;
  return {
    id: `m${counter}`,
    discipline: "singles",
    side_a: [ME],
    side_b: [OPP],
    status: "proposed",
    match_format: {
      sets_to_win: 2,
      games_per_set: 6,
      tiebreak_at: 6,
      final_set: "match_tiebreak_10",
    },
    created_at: "2026-09-01T00:00:00Z",
    names: [],
    ...overrides,
  };
}

describe("sideOf", () => {
  it("finds the player's side in doubles", () => {
    const doubles = match({ side_a: [OPP, OPP2], side_b: [PARTNER, ME] });
    expect(sideOf(doubles, ME)).toBe("b");
    expect(sideOf(doubles, OPP2)).toBe("a");
    expect(sideOf(doubles, "someone-else")).toBeNull();
  });
});

describe("formatScore", () => {
  const score = {
    sets: [
      { a: 6, b: 4 },
      { a: 3, b: 6 },
      { a: 10, b: 7, match_tiebreak: true },
    ],
  };

  it("reads from side A by default and brackets the match tiebreak", () => {
    expect(formatScore(score)).toBe("6–4 3–6 [10–7]");
  });

  it("puts the given side's games first", () => {
    expect(formatScore(score, "b")).toBe("4–6 6–3 [7–10]");
  });
});

describe("actionFor", () => {
  it("asks the other side to confirm a report", () => {
    expect(actionFor(match({ status: "reported", reported_by: OPP }), ME, NOW)).toBe("confirm");
    expect(actionFor(match({ status: "reported", reported_by: ME }), ME, NOW)).toBeNull();
  });

  it("treats a partner's report as the viewer's own side", () => {
    const doubles = match({
      side_a: [ME, PARTNER],
      side_b: [OPP, OPP2],
      status: "reported",
      reported_by: PARTNER,
    });
    expect(actionFor(doubles, ME, NOW)).toBeNull();
    expect(actionFor(doubles, OPP2, NOW)).toBe("confirm");
  });

  it("asks for a score once a scheduled match's time has passed", () => {
    expect(
      actionFor(match({ status: "scheduled", scheduled_at: "2026-10-03T18:00:00Z" }), ME, NOW),
    ).toBe("report");
    expect(
      actionFor(match({ status: "scheduled", scheduled_at: "2026-10-05T18:00:00Z" }), ME, NOW),
    ).toBeNull();
  });

  it("asks to arrange proposed matches, and nothing of bystanders", () => {
    expect(actionFor(match({ status: "proposed" }), ME, NOW)).toBe("arrange");
    expect(actionFor(match({ status: "proposed" }), "admin", NOW)).toBeNull();
  });
});

describe("groupMatches", () => {
  it("sorts every status into one section and drops cancelled matches", () => {
    const later = match({ status: "scheduled", scheduled_at: "2026-10-09T09:00:00Z" });
    const sooner = match({ status: "scheduled", scheduled_at: "2026-10-06T09:00:00Z" });
    const toConfirm = match({ status: "reported", reported_by: OPP });
    const toReport = match({ status: "scheduled", scheduled_at: "2026-10-01T09:00:00Z" });
    const proposed = match({ status: "proposed" });
    const mineReported = match({ status: "reported", reported_by: ME });
    const disputed = match({ status: "disputed" });
    const oldResult = match({ status: "confirmed", reported_at: "2026-09-10T00:00:00Z" });
    const newResult = match({ status: "resolved", reported_at: "2026-09-20T00:00:00Z" });
    const walkover = match({ status: "walkover" });
    const cancelled = match({ status: "cancelled" });

    const groups = groupMatches(
      [
        later,
        sooner,
        toConfirm,
        toReport,
        proposed,
        mineReported,
        disputed,
        oldResult,
        newResult,
        walkover,
        cancelled,
      ],
      ME,
      NOW,
    );

    expect(groups.needsYou).toEqual([toConfirm, toReport]);
    expect(groups.upcoming).toEqual([sooner, later]);
    expect(groups.toArrange).toEqual([proposed]);
    expect(groups.waiting).toEqual([mineReported, disputed]);
    expect(groups.results).toEqual([newResult, oldResult, walkover]);
  });
});
