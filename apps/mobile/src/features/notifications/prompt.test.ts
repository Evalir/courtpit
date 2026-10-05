import { promptDue } from "./prompt";

describe("promptDue", () => {
  const now = new Date("2026-10-05T12:00:00Z");
  it("asks until snoozed, then again after 30 days", () => {
    expect(promptDue(null, now)).toBe(true);
    expect(promptDue("2026-09-20T12:00:00Z", now)).toBe(false);
    expect(promptDue("2026-09-05T12:00:00Z", now)).toBe(true);
    expect(promptDue("garbage", now)).toBe(true);
  });
});
