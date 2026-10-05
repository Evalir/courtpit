import { isAdmin, moderation } from "./roles";

const admin = { id: "a", role: "admin" as const };
const owner = { id: "o", role: "owner" as const };
const member = (status: "active" | "banned" | "deleted" = "active") => ({
  id: "p",
  role: "player" as const,
  status,
});

describe("moderation", () => {
  it("lets admins ban active members and lift bans", () => {
    expect(moderation(admin, member())).toBe("ban");
    expect(moderation(admin, member("banned"))).toBe("unban");
    expect(moderation(admin, member("deleted"))).toBeNull();
  });

  it("follows rank, and never applies to oneself or to non-admins", () => {
    const otherAdmin = { id: "b", role: "admin" as const, status: "active" as const };
    expect(moderation(admin, otherAdmin)).toBeNull();
    expect(moderation(owner, otherAdmin)).toBe("ban");
    expect(moderation(owner, { ...owner, status: "active" })).toBeNull();
    expect(moderation({ id: "x", role: "player" }, member())).toBeNull();
    expect(isAdmin("owner")).toBe(true);
  });
});
