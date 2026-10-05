import type { MatchView } from "@/features/matches/match";

import { adminActions } from "./rulings";

const match = (overrides: Partial<MatchView>): MatchView => ({
  id: "m1",
  discipline: "singles",
  side_a: ["ana"],
  side_b: ["bo"],
  status: "disputed",
  match_format: { sets_to_win: 2, games_per_set: 6, tiebreak_at: 6, final_set: "full_set" },
  created_at: "2026-09-01T00:00:00Z",
  names: [],
  ...overrides,
});
const admin = { id: "admin", role: "admin" as const };

describe("adminActions", () => {
  it("lets admins decide disputes, walk over and cancel league matches", () => {
    expect(adminActions(match({}), admin)).toMatchObject({ resolve: true, cancel: false });
    expect(adminActions(match({ status: "scheduled", league_id: "l1" }), admin)).toMatchObject({
      resolve: false,
      walkover: true,
      cancel: true,
    });
    expect(adminActions(match({ status: "proposed" }), admin)).toMatchObject({
      walkover: false,
      cancel: true,
    });
    expect(adminActions(match({ status: "confirmed" }), admin)).toMatchObject({
      resolve: false,
      walkover: false,
      cancel: false,
    });
  });

  it("keeps admins from refereeing their own matches, except the owner", () => {
    const own = match({ side_a: ["admin"] });
    expect(adminActions(own, admin)).toEqual({
      resolve: false,
      walkover: false,
      cancel: false,
      blocked: "You play in this match, so another admin or the owner decides it.",
    });
    expect(adminActions(own, { id: "admin", role: "owner" })).toMatchObject({ resolve: true });
  });

  it("offers members nothing", () => {
    expect(adminActions(match({}), { id: "x", role: "player" })).toEqual({
      resolve: false,
      walkover: false,
      cancel: false,
      blocked: null,
    });
  });
});
