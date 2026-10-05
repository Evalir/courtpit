import type { ConfigContext, ExpoConfig } from "expo/config";

/**
 * One codebase, one app per community (spec §16). A native build bakes in the community's
 * identity from the environment its EAS build profile sets; colors are not baked in, they come
 * from `GET /api/v1/tenant` at runtime. Locally everything falls back to development values.
 *
 * - `COURTPIT_APP_NAME`   store / home-screen name
 * - `COURTPIT_BUNDLE_ID`  iOS bundle identifier and Android package
 * - `COURTPIT_SCHEME`     deep-link scheme
 * - `COURTPIT_EAS_PROJECT_ID` the EAS project, which push tokens are issued for
 * - `COURTPIT_WEB_HOST`   the community's web host (`riverside.courtpit.app`): its https links
 *                         open in the app (universal links / app links, decision 99)
 *
 * The community slug and API origin are `EXPO_PUBLIC_*` variables read by the bundle itself
 * (`src/api/config.ts`), so they also apply to the web build.
 */
const webHost = process.env.COURTPIT_WEB_HOST;

export default ({ config }: ConfigContext): ExpoConfig => ({
  ...config,
  name: process.env.COURTPIT_APP_NAME ?? "Courtpit (dev)",
  slug: "courtpit",
  version: "0.1.0",
  scheme: process.env.COURTPIT_SCHEME ?? "courtpit",
  orientation: "portrait",
  // Branding defines a light palette only; see docs/frontend.md (Theming).
  userInterfaceStyle: "light",
  ios: {
    bundleIdentifier: process.env.COURTPIT_BUNDLE_ID ?? "app.courtpit.dev",
    supportsTablet: true,
    ...(webHost ? { associatedDomains: [`applinks:${webHost}`, `webcredentials:${webHost}`] } : {}),
  },
  android: {
    package: process.env.COURTPIT_BUNDLE_ID ?? "app.courtpit.dev",
    ...(webHost
      ? {
          intentFilters: [
            {
              action: "VIEW",
              autoVerify: true,
              data: [{ scheme: "https", host: webHost }],
              category: ["BROWSABLE", "DEFAULT"],
            },
          ],
        }
      : {}),
  },
  web: {
    // A single-page app: the server hands the same index.html to every community host.
    output: "single",
    bundler: "metro",
  },
  plugins: [
    "expo-router",
    "expo-secure-store",
    "expo-font",
    ["expo-splash-screen", { backgroundColor: "#ffffff" }],
    ["expo-notifications", { defaultChannel: "default" }],
  ],
  extra: {
    eas: { projectId: process.env.COURTPIT_EAS_PROJECT_ID },
  },
  experiments: {
    typedRoutes: true,
    reactCompiler: true,
  },
});
