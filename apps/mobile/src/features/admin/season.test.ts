import type { EntryView } from "@/features/leagues/entries";
import type { LeagueView } from "@/features/leagues/league";

import { canFinish, soloEntries, togglePick } from "./season";

const entry = (overrides: Partial<EntryView>) =>
  ({ id: "e", player_ids: ["p"], status: "pending_partner", ...overrides }) as EntryView;

describe("soloEntries", () => {
  it("keeps only solo entries still waiting for a partner", () => {
    const solo = entry({ id: "solo" });
    expect(
      soloEntries([
        solo,
        entry({ id: "pair", player_ids: ["p", "q"], status: "confirmed" }),
        entry({ id: "gone", status: "withdrawn" }),
      ]),
    ).toEqual([solo]);
  });
});

describe("togglePick", () => {
  it("keeps at most two, dropping the oldest", () => {
    expect(togglePick([], "a")).toEqual(["a"]);
    expect(togglePick(["a"], "b")).toEqual(["a", "b"]);
    expect(togglePick(["a", "b"], "c")).toEqual(["b", "c"]);
    expect(togglePick(["a", "b"], "a")).toEqual(["b"]);
  });
});

describe("canFinish", () => {
  const league = { status: "active", ends_at: "2026-11-30T00:00:00Z" } as LeagueView;
  it("waits for the end of the season", () => {
    expect(canFinish(league, new Date("2026-11-29T23:00:00Z"))).toBe(false);
    expect(canFinish(league, new Date("2026-11-30T00:00:00Z"))).toBe(true);
    expect(canFinish({ ...league, status: "finished" }, new Date("2027-01-01"))).toBe(false);
  });
});
