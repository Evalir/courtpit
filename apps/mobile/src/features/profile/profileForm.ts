import type { components } from "@racquetcollective/api-client";

type PlayerProfile = components["schemas"]["PlayerProfile"];
type ProfilePatch = components["schemas"]["ProfilePatch"];
type Gender = components["schemas"]["Gender"];
type PlayPref = components["schemas"]["PlayPref"];

/** The social networks the form offers; other keys a profile holds are kept as they are. */
export const NETWORKS = [
  { key: "instagram", label: "Instagram" },
  { key: "facebook", label: "Facebook" },
  { key: "x", label: "X" },
] as const;
type Network = (typeof NETWORKS)[number]["key"];

/** The edit form's state: text inputs hold what was typed. */
export interface ProfileForm {
  display_name: string;
  utr: string;
  gender: Gender;
  play_pref: PlayPref;
  preferred_locations: string[];
  racket: string;
  strings: string;
  tension_kg: string;
  phone: string;
  phone_visible: boolean;
  socials: Record<Network, string>;
  socials_visible: boolean;
}

const text = (value: string | number | null | undefined) => (value == null ? "" : String(value));

export function formFromProfile(profile: PlayerProfile): ProfileForm {
  const socials: Record<string, string | undefined> = profile.socials;
  return {
    display_name: profile.display_name,
    utr: profile.utr == null ? "" : profile.utr.toFixed(2),
    gender: profile.gender,
    play_pref: profile.play_pref,
    preferred_locations: profile.preferred_locations,
    racket: text(profile.racket),
    strings: text(profile.strings),
    tension_kg: text(profile.tension_kg),
    phone: text(profile.phone),
    phone_visible: profile.phone_visible,
    socials: {
      instagram: socials.instagram ?? "",
      facebook: socials.facebook ?? "",
      x: socials.x ?? "",
    },
    socials_visible: profile.socials_visible,
  };
}

/** A field's problem, worded for the player (the server checks the same limits). */
export type Problems = Partial<Record<keyof ProfileForm, string>>;

/** A decimal typed with a dot or a comma, or null when it is not a plain number. */
function parseDecimal(input: string): number | null {
  const value = input.trim().replace(",", ".");
  return /^\d+(\.\d+)?$/.test(value) ? Number(value) : null;
}

const tooLong = (value: string, max: number) =>
  value.trim().length > max ? `At most ${max} characters.` : undefined;

/** Checks the form; empty optional fields clear the value. */
export function checkProfile(form: ProfileForm): Problems {
  const problems: Problems = {};
  const name = form.display_name.trim();
  if (name === "") problems.display_name = "Your name can’t be empty.";
  else if (name.length > 60) problems.display_name = "At most 60 characters.";
  if (form.utr.trim() !== "") {
    const utr = /^\d+([.,]\d{1,2})?$/.test(form.utr.trim()) ? parseDecimal(form.utr) : null;
    if (utr === null || utr < 1 || utr > 16.5) {
      problems.utr = "A UTR from 1.00 to 16.50, with up to two decimals.";
    }
  }
  if (form.tension_kg.trim() !== "") {
    const kg = parseDecimal(form.tension_kg);
    if (kg === null || kg < 10 || kg > 35) problems.tension_kg = "Between 10 and 35 kg.";
  }
  const racket = tooLong(form.racket, 80);
  if (racket) problems.racket = racket;
  const strings = tooLong(form.strings, 80);
  if (strings) problems.strings = strings;
  const phone = tooLong(form.phone, 32);
  if (phone) problems.phone = phone;
  if (NETWORKS.some(({ key }) => form.socials[key].trim().length > 200)) {
    problems.socials = "Handles are at most 200 characters.";
  }
  if (form.preferred_locations.length > 10) {
    problems.preferred_locations = "Up to 10 places.";
  }
  return problems;
}

const optional = (value: string) => (value.trim() === "" ? null : value.trim());

/** The changes between `profile` and a checked `form`, as a `PATCH /me` body. */
export function profilePatch(profile: PlayerProfile, form: ProfileForm): ProfilePatch {
  const before = formFromProfile(profile);
  const patch: ProfilePatch = {};
  if (form.display_name.trim() !== before.display_name) {
    patch.display_name = form.display_name.trim();
  }
  if (form.utr.trim() !== before.utr) {
    patch.utr = form.utr.trim() === "" ? null : parseDecimal(form.utr);
  }
  if (form.gender !== before.gender) patch.gender = form.gender;
  if (form.play_pref !== before.play_pref) patch.play_pref = form.play_pref;
  if (form.preferred_locations.join("\n") !== before.preferred_locations.join("\n")) {
    patch.preferred_locations = form.preferred_locations;
  }
  if (form.racket.trim() !== before.racket) patch.racket = optional(form.racket);
  if (form.strings.trim() !== before.strings) patch.strings = optional(form.strings);
  if (form.tension_kg.trim() !== before.tension_kg) {
    patch.tension_kg = form.tension_kg.trim() === "" ? null : parseDecimal(form.tension_kg);
  }
  if (form.phone.trim() !== before.phone) patch.phone = optional(form.phone);
  if (form.phone_visible !== before.phone_visible) patch.phone_visible = form.phone_visible;
  if (NETWORKS.some(({ key }) => form.socials[key].trim() !== before.socials[key])) {
    // Networks the form doesn't offer are sent back unchanged.
    const socials: Record<string, string> = { ...profile.socials };
    for (const { key } of NETWORKS) {
      const handle = form.socials[key].trim();
      if (handle === "") delete socials[key];
      else socials[key] = handle;
    }
    patch.socials = socials;
  }
  if (form.socials_visible !== before.socials_visible) {
    patch.socials_visible = form.socials_visible;
  }
  return patch;
}
