import {
  actionFor,
  formatScore,
  groupMatches,
  matchActions,
  outcomeNote,
  proposalLabel,
  sideOf,
  type MatchView,
} from "./match";

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

describe("matchActions", () => {
  const proposal = (by: string, status: "open" | "accepted" = "open") => ({
    id: `p-${by}`,
    proposed_by: by,
    proposed_time: "2026-10-10T17:00:00Z",
    status,
    created_at: "2026-10-01T00:00:00Z",
  });

  it("lets players of a friendly arrange, report and cancel it", () => {
    expect(matchActions(match({ status: "proposed", proposals: [] }), ME, NOW)).toEqual({
      confirm: false,
      answer: null,
      waiting: null,
      propose: true,
      report: true,
      cancel: true,
    });
  });

  it("leaves cancelling a league match to admins", () => {
    const league = match({ status: "scheduled", league_id: "l1" });
    expect(matchActions(league, ME, NOW)).toMatchObject({ report: true, cancel: false });
  });

  it("answers the other side's proposal and waits on our own", () => {
    const theirs = match({ proposals: [proposal(OPP)] });
    expect(matchActions(theirs, ME, NOW)).toMatchObject({
      answer: { proposed_by: OPP },
      waiting: null,
    });
    const doubles = match({
      side_a: [ME, PARTNER],
      side_b: [OPP, OPP2],
      proposals: [proposal(PARTNER), proposal(OPP, "accepted")],
    });
    expect(matchActions(doubles, ME, NOW)).toMatchObject({
      answer: null,
      waiting: { proposed_by: PARTNER },
    });
  });

  it("offers nothing on a finished match or to a bystander", () => {
    const done = matchActions(match({ status: "confirmed" }), ME, NOW);
    expect(Object.values(done).every((value) => value === false || value === null)).toBe(true);
    const bystander = matchActions(match({ status: "proposed" }), "someone", NOW);
    expect(bystander).toMatchObject({ propose: false, report: false, cancel: false });
  });
});

describe("outcomeNote", () => {
  const name = (id: string) => (id === ME ? "You" : "Olivia");

  it("says which player called a friendly off, with their reason", () => {
    const cancelled = match({ status: "cancelled", resolved_by: OPP, resolution_note: "Rain" });
    expect(outcomeNote(cancelled, name)).toBe("Cancelled by Olivia: “Rain”");
    expect(outcomeNote(match({ status: "cancelled", resolved_by: ME }), name)).toBe(
      "Cancelled by You.",
    );
  });

  it("credits anyone outside the match to the club's admins", () => {
    const byAdmin = match({ status: "cancelled", resolved_by: "admin" });
    expect(outcomeNote(byAdmin, name)).toBe("Cancelled by a club admin.");
    const ruled = match({ status: "resolved", resolved_by: "admin", resolution_note: "6–4 6–4" });
    expect(outcomeNote(ruled, name)).toBe("Admin ruling: 6–4 6–4");
    expect(outcomeNote(match({ status: "confirmed" }), name)).toBeNull();
  });
});

describe("proposalLabel", () => {
  const superseded = {
    id: "p1",
    proposed_by: OPP,
    proposed_time: "2026-10-10T17:00:00Z",
    status: "superseded" as const,
    created_at: "2026-10-01T00:00:00Z",
  };

  it("tells a replaced proposal from one the match's end closed", () => {
    expect(proposalLabel(superseded, match({ status: "scheduled" }))).toBe(
      "Replaced by a newer proposal",
    );
    expect(proposalLabel(superseded, match({ status: "cancelled" }))).toBe("Closed");
  });
});
