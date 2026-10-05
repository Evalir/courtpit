import type { components } from "@racquetcollective/api-client";

import { checkProfile, formFromProfile, profilePatch } from "./profileForm";

type PlayerProfile = components["schemas"]["PlayerProfile"];

const profile: PlayerProfile = {
  id: "p1",
  community_id: "c1",
  user_id: "u1",
  display_name: "Lily Fernandez",
  utr: 6.4,
  gender: "female",
  play_pref: "any",
  preferred_locations: ["Central Park Club"],
  racket: "Pure Aero",
  strings: null,
  tension_kg: 24,
  phone: null,
  phone_visible: false,
  socials: { instagram: "@lily", strava: "lily-f" },
  socials_visible: true,
  role: "player",
  status: "active",
  created_at: "2026-01-01T00:00:00Z",
} as PlayerProfile;

describe("formFromProfile", () => {
  it("shows numbers as typed text and the offered networks", () => {
    const form = formFromProfile(profile);
    expect(form.utr).toBe("6.40");
    expect(form.tension_kg).toBe("24");
    expect(form.strings).toBe("");
    expect(form.socials).toEqual({ instagram: "@lily", facebook: "", x: "" });
  });
});

describe("checkProfile", () => {
  const form = formFromProfile(profile);

  it("accepts the profile as it is, and empty optional fields", () => {
    expect(checkProfile(form)).toEqual({});
    expect(checkProfile({ ...form, utr: "", tension_kg: "", racket: "" })).toEqual({});
  });

  it("checks the server's limits", () => {
    expect(
      checkProfile({
        ...form,
        display_name: "  ",
        utr: "6.405",
        tension_kg: "40",
        phone: "1".repeat(33),
        preferred_locations: Array.from({ length: 11 }, (_, index) => `Court ${index}`),
      }),
    ).toEqual({
      display_name: "Your name can’t be empty.",
      utr: "A UTR from 1.00 to 16.50, with up to two decimals.",
      tension_kg: "Between 10 and 35 kg.",
      phone: "At most 32 characters.",
      preferred_locations: "Up to 10 places.",
    });
    expect(checkProfile({ ...form, utr: "16,6" }).utr).toBeDefined();
    expect(checkProfile({ ...form, utr: "7,25" }).utr).toBeUndefined();
  });
});

describe("profilePatch", () => {
  const form = formFromProfile(profile);

  it("sends nothing when nothing changed", () => {
    expect(profilePatch(profile, form)).toEqual({});
  });

  it("sends only what changed, clearing emptied fields", () => {
    expect(
      profilePatch(profile, {
        ...form,
        display_name: " Lily F. ",
        utr: "6,5",
        racket: "",
        phone: "+44 7700 900123",
        phone_visible: true,
      }),
    ).toEqual({
      display_name: "Lily F.",
      utr: 6.5,
      racket: null,
      phone: "+44 7700 900123",
      phone_visible: true,
    });
  });

  it("keeps networks the form doesn't offer", () => {
    expect(
      profilePatch(profile, { ...form, socials: { instagram: "", facebook: "lily.f", x: "" } }),
    ).toEqual({ socials: { strava: "lily-f", facebook: "lily.f" } });
  });
});
