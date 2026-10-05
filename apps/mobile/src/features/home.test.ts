import { needsYouLine } from "./home";

describe("needsYouLine", () => {
  it("counts matches and league entries that need the player", () => {
    expect(needsYouLine(0, 0)).toBe("You’re all caught up.");
    expect(needsYouLine(1, 0)).toBe("1 match needs you.");
    expect(needsYouLine(3, 0)).toBe("3 matches need you.");
    expect(needsYouLine(0, 1)).toBe("1 league entry needs you.");
    expect(needsYouLine(1, 2)).toBe("1 match and 2 league entries need you.");
  });
});
