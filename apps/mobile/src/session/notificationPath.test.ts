import { notificationPath } from "./notificationPath";

describe("notificationPath", () => {
  it("reads the server's url and ignores anything else", () => {
    expect(notificationPath({ url: "/matches/m1" })).toBe("/matches/m1");
    expect(notificationPath({ url: "/sign-in" })).toBeNull();
    expect(notificationPath({ url: 3 })).toBeNull();
    expect(notificationPath({})).toBeNull();
    expect(notificationPath(null)).toBeNull();
  });
});
