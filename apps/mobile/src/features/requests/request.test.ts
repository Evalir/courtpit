import {
  bandAround,
  fitsLevel,
  levelLabel,
  requestRole,
  spotsLabel,
  windowLabel,
  type MatchRequestView,
} from "./request";

const request = (overrides: Partial<MatchRequestView>): MatchRequestView => ({
  id: "r1",
  created_by: "me",
  discipline: "doubles",
  players: ["me"],
  slots_open: 3,
  time_window_start: "2026-10-10T08:00:00Z",
  time_window_end: "2026-10-10T11:00:00Z",
  status: "open",
  created_at: "2026-10-01T00:00:00Z",
  names: [],
  ...overrides,
});

describe("requestRole", () => {
  it("tells creator, joiner and outsider apart", () => {
    expect(requestRole(request({}), "me")).toBe("creator");
    expect(requestRole(request({ created_by: "ana", players: ["ana", "me"] }), "me")).toBe(
      "joined",
    );
    expect(requestRole(request({ created_by: "ana", players: ["ana"] }), "me")).toBe("open");
  });
});

describe("levels", () => {
  it("labels every kind of band", () => {
    expect(levelLabel(null, null)).toBe("Any level");
    expect(levelLabel(4, 6)).toBe("UTR 4.00–6.00");
    expect(levelLabel(4, null)).toBe("UTR 4.00 and up");
    expect(levelLabel(undefined, 6)).toBe("Up to UTR 6.00");
  });

  it("lets players without a UTR join only unbanded requests", () => {
    expect(fitsLevel(null, null, null)).toBe(true);
    expect(fitsLevel(null, 4, 6)).toBe(false);
    expect(fitsLevel(5, 4, 6)).toBe(true);
    expect(fitsLevel(6.5, 4, 6)).toBe(false);
    expect(fitsLevel(3.5, 4, null)).toBe(false);
    expect(fitsLevel(3.5, null, 4)).toBe(true);
  });

  it("builds a band around a level within the API's range", () => {
    expect(bandAround(5.25)).toEqual({ min: 4.25, max: 6.25 });
    expect(bandAround(1.5)).toEqual({ min: 1, max: 2.5 });
    expect(bandAround(16)).toEqual({ min: 15, max: 16.5 });
  });
});

describe("labels", () => {
  it("counts spots", () => {
    expect(spotsLabel(1)).toBe("1 spot left");
    expect(spotsLabel(2)).toBe("2 spots left");
  });

  it("shows a same-day window compactly", () => {
    const start = new Date(2026, 9, 10, 9, 0).toISOString();
    const end = new Date(2026, 9, 10, 12, 0).toISOString();
    expect(windowLabel(start, end, "en-GB")).toBe("Sat 10 Oct, 09:00–12:00");
    const nextDay = new Date(2026, 9, 11, 10, 0).toISOString();
    expect(windowLabel(start, nextDay, "en-GB")).toBe("Sat 10 Oct 09:00 – Sun 11 Oct 10:00");
  });
});
