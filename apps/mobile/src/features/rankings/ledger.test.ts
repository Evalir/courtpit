import {
  byMonth,
  ledgerTarget,
  pointsLabel,
  sourceLabel,
  stillCounts,
  type LedgerEntry,
} from "./ledger";

const entry = (overrides: Partial<LedgerEntry>): LedgerEntry => ({
  id: "r1",
  discipline: "singles",
  source: "league_match",
  source_id: "m1",
  points: 3,
  occurred_at: "2026-10-01T18:00:00Z",
  ...overrides,
});

describe("sourceLabel and ledgerTarget", () => {
  it("names each source and links matches and seasons", () => {
    expect(sourceLabel("league_season")).toBe("Season finish");
    expect(sourceLabel("club_night")).toBe("club night");
    expect(ledgerTarget(entry({}))).toEqual({ pathname: "/matches/[id]", params: { id: "m1" } });
    expect(ledgerTarget(entry({ source: "league_season", source_id: "l1" }))).toEqual({
      pathname: "/leagues/[id]",
      params: { id: "l1" },
    });
    expect(ledgerTarget(entry({ source: "tournament" }))).toBeNull();
  });
});

describe("stillCounts", () => {
  it("drops points older than 52 weeks", () => {
    const now = new Date("2026-10-04T00:00:00Z");
    expect(stillCounts(entry({ occurred_at: "2025-10-06T00:00:00Z" }), now)).toBe(true);
    expect(stillCounts(entry({ occurred_at: "2025-10-04T00:00:00Z" }), now)).toBe(false);
  });
});

describe("byMonth", () => {
  it("groups consecutive entries by month", () => {
    const one = entry({ id: "1", occurred_at: "2026-10-03T10:00:00Z" });
    const two = entry({ id: "2", occurred_at: "2026-10-01T10:00:00Z" });
    const three = entry({ id: "3", occurred_at: "2026-09-20T10:00:00Z" });
    expect(byMonth([one, two, three], "en-GB")).toEqual([
      { month: "October 2026", entries: [one, two] },
      { month: "September 2026", entries: [three] },
    ]);
  });
});

describe("pointsLabel", () => {
  it("signs gains", () => {
    expect(pointsLabel(12)).toBe("+12");
    expect(pointsLabel(0)).toBe("0");
  });
});
