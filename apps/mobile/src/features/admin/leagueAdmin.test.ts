import type { LeagueView } from "@/features/leagues/league";

import { leagueAdmin } from "./leagueAdmin";

const league = (overrides: Partial<LeagueView>) =>
  ({ status: "draft", ...overrides }) as LeagueView;

describe("leagueAdmin", () => {
  it("follows the league's lifecycle", () => {
    expect(leagueAdmin(league({}))).toEqual({ edit: true, publish: true, cancel: true });
    expect(leagueAdmin(league({ published_at: "2026-10-01T00:00:00Z" }))).toEqual({
      edit: true,
      publish: false,
      cancel: true,
    });
    expect(leagueAdmin(league({ status: "registration" }))).toEqual({
      edit: false,
      publish: false,
      cancel: true,
    });
    expect(leagueAdmin(league({ status: "finished" }))).toEqual({
      edit: false,
      publish: false,
      cancel: false,
    });
  });
});
