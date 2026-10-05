import { formatDate, formatDateTime, formatUtr } from "./format";

describe("format", () => {
  it("formats dates in the given locale", () => {
    // Local time zone varies between machines; use a midday UTC instant and date-only checks.
    expect(formatDate("2026-10-11T12:00:00Z", "en-GB")).toBe("11 Oct 2026");
    expect(formatDateTime("2026-10-11T12:00:00Z", "en-GB")).toMatch(/^Sun 11 Oct/);
  });

  it("formats UTR with two decimals and a dash when unknown", () => {
    expect(formatUtr(6.5)).toBe("6.50");
    expect(formatUtr(null)).toBe("—");
    expect(formatUtr(undefined)).toBe("—");
  });
});
