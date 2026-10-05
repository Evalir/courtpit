import { parsePreference, resolveScheme } from "./appearance";

describe("parsePreference", () => {
  it("keeps a stored light or dark choice", () => {
    expect(parsePreference("light")).toBe("light");
    expect(parsePreference("dark")).toBe("dark");
  });

  it.each([null, undefined, "", "system", "sepia", "DARK"])(
    "follows the system for %p",
    (stored) => {
      expect(parsePreference(stored)).toBe("system");
    },
  );
});

describe("resolveScheme", () => {
  it("uses the player's choice over the system's", () => {
    expect(resolveScheme("light", "dark")).toBe("light");
    expect(resolveScheme("dark", "light")).toBe("dark");
    expect(resolveScheme("dark", null)).toBe("dark");
  });

  it("follows the system, and draws light when the system says nothing", () => {
    expect(resolveScheme("system", "dark")).toBe("dark");
    expect(resolveScheme("system", "light")).toBe("light");
    expect(resolveScheme("system", null)).toBe("light");
    expect(resolveScheme("system", "unspecified")).toBe("light");
  });
});
