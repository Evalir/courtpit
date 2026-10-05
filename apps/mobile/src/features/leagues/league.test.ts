import { formatDate } from "@/features/format";

import { describeFormat, leagueTiming, ownBoxFirst, type LeagueView } from "./league";

const league: LeagueView = {
  id: "l1",
  name: "Autumn Singles",
  discipline: "singles",
  registration_opens_at: "2026-09-01T12:00:00Z",
  registration_closes_at: "2026-09-20T12:00:00Z",
  starts_at: "2026-09-22T12:00:00Z",
  ends_at: "2026-11-30T12:00:00Z",
  status: "active",
  match_format: {
    sets_to_win: 2,
    games_per_set: 6,
    tiebreak_at: 6,
    final_set: "match_tiebreak_10",
  },
  box_min_size: 6,
  box_max_size: 8,
  created_at: "2026-08-01T00:00:00Z",
};
const NOW = new Date("2026-10-04T12:00:00Z");

describe("leagueTiming", () => {
  it("shows the season for an active league", () => {
    // Month abbreviations differ between ICU versions ("Sep"/"Sept"), so compose the expectation.
    const start = formatDate(league.starts_at, "en-GB");
    expect(leagueTiming(league, NOW, "en-GB")).toBe(`${start} – 30 Nov 2026`);
  });

  it("shows when registration opens or closes", () => {
    const upcoming = {
      ...league,
      status: "registration" as const,
      registration_opens_at: "2026-10-10T12:00:00Z",
      registration_closes_at: "2026-10-20T12:00:00Z",
    };
    expect(leagueTiming(upcoming, NOW, "en-GB")).toBe("Registration opens 10 Oct 2026");
    expect(
      leagueTiming({ ...upcoming, registration_opens_at: "2026-10-01T12:00:00Z" }, NOW, "en-GB"),
    ).toBe("Registration closes 20 Oct 2026");
  });

  it("reads a deadline at midnight as the end of the day before", () => {
    const closes = new Date(2026, 10, 21).toISOString(); // local midnight starting 21 Nov
    const open = {
      ...league,
      status: "registration" as const,
      registration_opens_at: "2026-10-01T12:00:00Z",
      registration_closes_at: closes,
    };
    expect(leagueTiming(open, NOW, "en-GB")).toBe("Registration closes 20 Nov 2026");
  });

  it("explains a cancellation", () => {
    expect(
      leagueTiming({ ...league, status: "cancelled", cancel_reason: "too few entries" }, NOW),
    ).toBe("too few entries");
  });
});

describe("describeFormat", () => {
  it("describes the common club format", () => {
    expect(describeFormat(league.match_format)).toBe(
      "Best of 3 sets · tiebreak at 6–6 · match tiebreak to 10 instead of a final set",
    );
  });

  it("covers single sets, short sets, advantage sets and golden point", () => {
    expect(
      describeFormat({
        sets_to_win: 1,
        games_per_set: 4,
        tiebreak_at: 4,
        final_set: "full_set",
        deuce: "golden_point",
      }),
    ).toBe("One set · sets to 4 games · tiebreak at 4–4 · golden point at deuce");
    expect(describeFormat({ sets_to_win: 3, games_per_set: 6, final_set: "pro_set_8" })).toBe(
      "Best of 5 sets · advantage sets · pro set to 8 as the final set",
    );
  });
});

describe("ownBoxFirst", () => {
  const box = (name: string, players: string[][]) => ({
    name,
    table: players.map((player_ids) => ({ player_ids })),
  });

  it("moves the viewer's box to the top and keeps the rest in order", () => {
    const one = box("Box 1", [["a"], ["b"]]);
    const two = box("Box 2", [["c"], ["me", "d"]]);
    const three = box("Box 3", [["e"]]);
    expect(ownBoxFirst([one, two, three], "me")).toEqual({ boxes: [two, one, three], mine: two });
    expect(ownBoxFirst([one, three], "me")).toEqual({ boxes: [one, three], mine: null });
  });
});
