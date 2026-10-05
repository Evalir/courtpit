import {
  entriesNeedingYou,
  entryBadges,
  entryState,
  leagueEntries,
  mixedBlocker,
  registrationOpen,
  type EntryView,
  type MyEntry,
} from "./entries";
import type { LeagueView } from "./league";

const ME = "me";
const ANA = "ana";
const BO = "bo";
const NOW = new Date("2026-10-04T12:00:00Z");

let counter = 0;
function entry(overrides: Partial<EntryView>): EntryView {
  counter += 1;
  return {
    id: `e${counter}`,
    league_id: "l1",
    player_ids: [ME],
    created_by: ME,
    status: "pending_partner",
    looking_for_partner: false,
    created_at: "2026-10-01T00:00:00Z",
    names: [],
    ...overrides,
  };
}

function league(overrides: Partial<LeagueView> = {}): LeagueView {
  return {
    id: "l1",
    name: "Autumn doubles",
    discipline: "doubles",
    registration_opens_at: "2026-10-01T00:00:00Z",
    registration_closes_at: "2026-10-10T00:00:00Z",
    starts_at: "2026-10-12T00:00:00Z",
    ends_at: "2026-12-12T00:00:00Z",
    status: "registration",
    match_format: { sets_to_win: 2, games_per_set: 6, tiebreak_at: 6, final_set: "full_set" },
    box_min_size: 6,
    box_max_size: 8,
    created_at: "2026-09-01T00:00:00Z",
    ...overrides,
  };
}

describe("registrationOpen", () => {
  it("follows the window, not just the status", () => {
    expect(registrationOpen(league(), NOW)).toBe(true);
    expect(registrationOpen(league({ registration_opens_at: "2026-10-05T00:00:00Z" }), NOW)).toBe(
      false,
    );
    expect(registrationOpen(league({ registration_closes_at: NOW.toISOString() }), NOW)).toBe(
      false,
    );
    expect(registrationOpen(league({ status: "active" }), NOW)).toBe(false);
  });
});

describe("entryState", () => {
  it("reads the viewer's own entry", () => {
    expect(entryState(entry({ status: "confirmed", player_ids: [ANA, ME] }), ME)).toEqual({
      kind: "entered",
      partner: ANA,
    });
    expect(entryState(entry({ status: "confirmed" }), ME)).toEqual({
      kind: "entered",
      partner: null,
    });
    expect(entryState(entry({ invited_partner_id: BO }), ME)).toEqual({
      kind: "waiting",
      partner: BO,
    });
    expect(entryState(entry({ looking_for_partner: true }), ME)).toEqual({ kind: "looking" });
    expect(entryState(entry({}), ME)).toEqual({ kind: "no_partner" });
  });

  it("sees invitations to the viewer, and nothing in other entries", () => {
    const invite = entry({ player_ids: [ANA], created_by: ANA, invited_partner_id: ME });
    expect(entryState(invite, ME)).toEqual({ kind: "invited", by: ANA });
    expect(entryState(entry({ player_ids: [ANA], created_by: ANA }), ME)).toBeNull();
    expect(entryState(entry({ status: "withdrawn" }), ME)).toBeNull();
  });
});

describe("leagueEntries", () => {
  it("splits a league's entries for the viewer", () => {
    const own = entry({ looking_for_partner: true });
    const invite = entry({ player_ids: [ANA], created_by: ANA, invited_partner_id: ME });
    const looking = entry({ player_ids: [BO], created_by: BO, looking_for_partner: true });
    const pair = entry({ player_ids: [ANA, BO], created_by: ANA, status: "confirmed" });
    const gone = entry({ player_ids: [BO], created_by: BO, status: "withdrawn" });
    expect(leagueEntries([own, invite, looking, pair, gone], ME)).toEqual({
      own,
      invitations: [invite],
      looking: [looking],
      confirmed: 1,
    });
  });
});

describe("entriesNeedingYou", () => {
  it("keeps invitations and partnerless entries while registration is open", () => {
    const invite: MyEntry = {
      league: league(),
      entry: entry({ player_ids: [ANA], created_by: ANA, invited_partner_id: ME }),
    };
    const lapsed: MyEntry = { league: league(), entry: entry({}) };
    const waiting: MyEntry = { league: league(), entry: entry({ invited_partner_id: BO }) };
    const closed: MyEntry = { league: league({ status: "active" }), entry: entry({}) };
    expect(entriesNeedingYou([invite, lapsed, waiting, closed], ME, NOW)).toEqual([invite, lapsed]);
  });
});

describe("mixedBlocker", () => {
  it("stops an undisclosed gender from entering mixed, and leaves the rest to the server", () => {
    expect(mixedBlocker("mixed", "undisclosed")).toMatch(/needs one/);
    expect(mixedBlocker("mixed", "other")).toBeNull();
    expect(mixedBlocker("doubles", "undisclosed")).toBeNull();
  });
});

describe("entryBadges", () => {
  it("marks each league once, the viewer's own entry first", () => {
    const invite = (by: string, leagueId: string): MyEntry => ({
      league: league({ id: leagueId }),
      entry: entry({ player_ids: [by], created_by: by, invited_partner_id: ME }),
    });
    const badges = entryBadges(
      [
        invite(ANA, "l1"),
        { league: league({ id: "l1" }), entry: entry({ status: "confirmed" }) },
        invite(BO, "l2"),
        { league: league({ id: "l3" }), entry: entry({ looking_for_partner: true }) },
      ],
      ME,
    );
    expect(Object.fromEntries([...badges].map(([id, badge]) => [id, badge.label]))).toEqual({
      l1: "You’re in",
      l2: "Invited",
      l3: "Entry pending",
    });
  });
});
