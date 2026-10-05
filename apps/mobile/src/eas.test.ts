// The build profiles that become store apps (spec §16): one per community, complete enough that
// a build can't ship talking to the wrong community or claiming someone else's links.
// Jest runs this file as CommonJS; the app's tsconfig has no Node types.
declare const __dirname: string;
declare const require: (path: string) => unknown;

type Profile = {
  extends?: string;
  channel?: string;
  distribution?: string;
  developmentClient?: boolean;
  env?: Record<string, string>;
};
type Eas = { build: Record<string, Profile> };

const eas = require(`${__dirname}/../eas.json`) as Eas;

/** A profile with everything it inherits. */
function resolved(name: string): Profile {
  const profile = eas.build[name];
  if (!profile) throw new Error(`no profile ${name}`);
  const parent = profile.extends ? resolved(profile.extends) : {};
  return { ...parent, ...profile, env: { ...parent.env, ...profile.env } };
}

const IDENTITY = [
  "EXPO_PUBLIC_COMMUNITY",
  "EXPO_PUBLIC_API_URL",
  "COURTPIT_APP_NAME",
  "COURTPIT_BUNDLE_ID",
  "COURTPIT_SCHEME",
  "COURTPIT_WEB_HOST",
];

const communityProfiles = Object.keys(eas.build).filter(
  (name) => resolved(name).distribution === "store" && name !== "store",
);

describe("eas.json", () => {
  it("has a store profile per community", () => {
    expect(communityProfiles).toContain("demo");
  });

  it.each(communityProfiles)("%s names its community completely and consistently", (name) => {
    const { env = {}, channel } = resolved(name);
    for (const key of IDENTITY) expect(env[key]).toBeTruthy();
    expect(channel).toBe(name);
    expect(env.EXPO_PUBLIC_API_URL).toBe(`https://${env.COURTPIT_WEB_HOST}`);
    expect(env.COURTPIT_BUNDLE_ID).toMatch(/^[a-z][a-z0-9]*(\.[a-z][a-z0-9]*)+$/);
    expect(env.COURTPIT_SCHEME).toMatch(/^[a-z][a-z0-9+.-]*$/);
  });

  it("gives every community its own app, scheme and update channel", () => {
    for (const key of ["COURTPIT_BUNDLE_ID", "COURTPIT_SCHEME", "COURTPIT_WEB_HOST"]) {
      const values = communityProfiles.map((name) => resolved(name).env?.[key]);
      expect(new Set(values).size).toBe(values.length);
    }
  });
});
