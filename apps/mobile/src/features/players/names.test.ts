import { playerNames, sideName } from "./names";

const ME = "00000000-0000-7000-8000-000000000001";
const ANA = "00000000-0000-7000-8000-000000000002";
const GONE = "00000000-0000-7000-8000-000000000003";
const UNNAMED = "00000000-0000-7000-8000-000000000004";

describe("playerNames", () => {
  const name = playerNames(
    [
      { id: ME, display_name: "Lily Fernandez" },
      { id: ANA, display_name: "Ana Ruiz" },
      { id: GONE, display_name: null },
    ],
    ME,
  );

  it("names the viewer “You” and others by their display name", () => {
    expect(name(ME)).toBe("You");
    expect(name(ANA)).toBe("Ana Ruiz");
  });

  it("reads a hidden or unnamed player as a former member", () => {
    expect(name(GONE)).toBe("Former member");
    expect(name(UNNAMED)).toBe("Former member");
    expect(playerNames([{ id: GONE }])(GONE)).toBe("Former member");
  });

  it("joins a side’s players", () => {
    expect(sideName([ME, ANA], name)).toBe("You & Ana Ruiz");
    expect(sideName([ANA], name)).toBe("Ana Ruiz");
  });
});
