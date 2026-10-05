import { addDays, dayOf, endOf, formatDay, lastDayBefore, monthGrid, startOf } from "./calendar";

describe("calendar days", () => {
  it("round-trips through local midnights", () => {
    expect(dayOf(startOf("2026-10-04"))).toBe("2026-10-04");
    expect(dayOf(endOf("2026-10-31"))).toBe("2026-11-01");
    expect(lastDayBefore(endOf("2026-10-31"))).toBe("2026-10-31");
    expect(addDays("2026-12-30", 3)).toBe("2027-01-02");
    expect(addDays("2026-03-01", -1)).toBe("2026-02-28");
  });

  it("lays a month out in Monday-first weeks", () => {
    // October 2026 starts on a Thursday and has 31 days.
    const weeks = monthGrid(2026, 9);
    expect(weeks[0]).toEqual([
      null,
      null,
      null,
      "2026-10-01",
      "2026-10-02",
      "2026-10-03",
      "2026-10-04",
    ]);
    expect(weeks.at(-1)).toEqual([
      "2026-10-26",
      "2026-10-27",
      "2026-10-28",
      "2026-10-29",
      "2026-10-30",
      "2026-10-31",
      null,
    ]);
    expect(weeks.every((week) => week.length === 7)).toBe(true);
  });

  it("formats a day", () => {
    expect(formatDay("2026-10-10", "en-GB")).toBe("Sat, 10 Oct 2026");
  });
});
