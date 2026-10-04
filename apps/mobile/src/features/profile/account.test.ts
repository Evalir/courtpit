import { deletionConfirmed, exportFileName, passwordProblem } from "./account";

describe("passwordProblem", () => {
  it("checks length, then the repetition", () => {
    expect(passwordProblem("short", "short")).toEqual({ password: "At least 10 characters." });
    expect(passwordProblem("x".repeat(257), "")).toEqual({ password: "At most 256 characters." });
    expect(passwordProblem("long enough!", "long enough")).toEqual({
      again: "The two passwords differ.",
    });
    expect(passwordProblem("long enough!", "long enough!")).toBeNull();
  });

  it("counts characters, not UTF-16 units", () => {
    expect(passwordProblem("🎾".repeat(10), "🎾".repeat(10))).toBeNull();
  });
});

describe("deletionConfirmed", () => {
  it("takes the email in any case, ignoring outer spaces", () => {
    expect(deletionConfirmed(" Lily@Example.com ", "lily@example.com")).toBe(true);
    expect(deletionConfirmed("lily@example", "lily@example.com")).toBe(false);
  });
});

describe("exportFileName", () => {
  it("dates the file", () => {
    expect(exportFileName(new Date("2026-10-04T23:00:00Z"))).toBe(
      "courtpit-export-2026-10-04.json",
    );
  });
});
