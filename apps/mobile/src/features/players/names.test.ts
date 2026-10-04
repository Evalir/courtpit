import { nameLookup, sideName } from "./names";

describe("nameLookup", () => {
  const refs = [
    { id: "a", display_name: "Ana Ruiz" },
    { id: "b", display_name: "Ben Ortiz" },
  ];

  it("names known ids, the viewer as You, and anyone else as a former member", () => {
    const name = nameLookup(refs, "b");
    expect(name("a")).toBe("Ana Ruiz");
    expect(name("b")).toBe("You");
    expect(name("zzz")).toBe("Former member");
  });

  it("joins a doubles side", () => {
    expect(sideName(["b", "a"], nameLookup(refs, "b"))).toBe("You & Ana Ruiz");
  });
});
