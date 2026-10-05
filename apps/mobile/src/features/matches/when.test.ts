import { dayLabel, daySlots, pickerWindow, sameDay, upcomingDays } from "./when";

// Local times throughout, so the tests hold in any time zone.
const now = new Date(2026, 9, 4, 18, 10);

describe("upcomingDays", () => {
  it("starts today and spans the window", () => {
    const days = upcomingDays(now);
    expect(days).toHaveLength(pickerWindow.days);
    expect(days[0]).toEqual(new Date(2026, 9, 4));
    expect(days[27]).toEqual(new Date(2026, 9, 31));
  });

  it("crosses month ends", () => {
    expect(upcomingDays(new Date(2026, 9, 30, 9), 3)).toEqual([
      new Date(2026, 9, 30),
      new Date(2026, 9, 31),
      new Date(2026, 10, 1),
    ]);
  });
});

describe("daySlots", () => {
  it("offers half-hour slots and marks those already past", () => {
    const slots = daySlots(new Date(2026, 9, 4), now);
    expect(slots[0]?.time).toEqual(new Date(2026, 9, 4, 7, 0));
    expect(slots.at(-1)?.time).toEqual(new Date(2026, 9, 4, 21, 30));
    expect(slots.filter((slot) => !slot.past)[0]?.time).toEqual(new Date(2026, 9, 4, 18, 30));
  });

  it("leaves tomorrow open", () => {
    expect(daySlots(new Date(2026, 9, 5), now).every((slot) => !slot.past)).toBe(true);
  });
});

describe("labels", () => {
  it("names today and tomorrow", () => {
    expect(dayLabel(new Date(2026, 9, 4), now)).toBe("Today");
    expect(dayLabel(new Date(2026, 9, 5), now)).toBe("Tomorrow");
    expect(dayLabel(new Date(2026, 9, 10), now, "en-GB")).toBe("Sat 10");
  });

  it("compares local days", () => {
    expect(sameDay(new Date(2026, 9, 4, 23, 59), new Date(2026, 9, 4, 0, 1))).toBe(true);
    expect(sameDay(new Date(2026, 9, 4), new Date(2026, 9, 5))).toBe(false);
  });
});
