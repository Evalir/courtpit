import { returnPathOf, returnTo } from "./returnTo";

describe("returnPathOf", () => {
  it("reads the path and query of web, app and Expo Go links", () => {
    expect(returnPathOf("https://riverside.courtpit.app/leagues/1?tab=box#top")).toBe(
      "/leagues/1?tab=box",
    );
    expect(returnPathOf("courtpit://matches/m1")).toBe("/matches/m1");
    expect(returnPathOf("courtpit:///matches/m1/")).toBe("/matches/m1");
    expect(returnPathOf("exp://192.168.1.5:8081/--/players/p1")).toBe("/players/p1");
    expect(returnPathOf("/rankings/me")).toBe("/rankings/me");
  });

  it("ignores links to nowhere in particular", () => {
    expect(returnPathOf("https://riverside.courtpit.app")).toBeNull();
    expect(returnPathOf("https://riverside.courtpit.app/")).toBeNull();
    expect(returnPathOf("http://localhost:8081/sign-in")).toBeNull();
    expect(returnPathOf("/verify?email=a%40b.c")).toBeNull();
    expect(returnPathOf("/profile/delete")).toBeNull();
    expect(returnPathOf("//evil.example/leagues")).toBeNull();
    expect(returnPathOf(null)).toBeNull();
  });
});

describe("returnTo", () => {
  it("keeps the latest destination until taken once", () => {
    returnTo.remember("courtpit://leagues/1");
    returnTo.remember("https://club.example/");
    returnTo.remember("courtpit://matches/m1");
    expect(returnTo.take()).toBe("/matches/m1");
    expect(returnTo.take()).toBeNull();
  });
});
