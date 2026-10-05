import type { ConfigContext, ExpoConfig } from "expo/config";

/**
 * One codebase, one app per community (spec §16). A native build bakes in the community's
 * identity from the environment its EAS build profile sets; colors are not baked in, they come
 * from `GET /api/v1/tenant` at runtime. Locally everything falls back to development values.
 *
 * - `RACQUETCOLLECTIVE_APP_NAME`   store / home-screen name
 * - `RACQUETCOLLECTIVE_BUNDLE_ID`  iOS bundle identifier and Android package
 * - `RACQUETCOLLECTIVE_SCHEME`     deep-link scheme
 * - `RACQUETCOLLECTIVE_EAS_PROJECT_ID` the community's EAS project: push tokens are issued for it and
 *                         EAS Update serves its JS updates (none without it)
 * - `RACQUETCOLLECTIVE_ICON`       the community's app icon (a path under `apps/mobile`), if it has one
 * - `RACQUETCOLLECTIVE_GOOGLE_IOS_URL_SCHEME` the reversed iOS OAuth client id
 *                         (`com.googleusercontent.apps.…`); Google sign-in is built in only with it.
 *                         The client ids themselves are `EXPO_PUBLIC_GOOGLE_*_CLIENT_ID` (decision 100).
 * - `RACQUETCOLLECTIVE_WEB_HOST`   the community's web host (`riverside.racquetcollective.app`): its https links
 *                         open in the app (universal links / app links, decision 99)
 *
 * The community slug and API origin are `EXPO_PUBLIC_*` variables read by the bundle itself
 * (`src/api/config.ts`), so they also apply to the web build.
 */
const webHost = process.env.RACQUETCOLLECTIVE_WEB_HOST;
const projectId = process.env.RACQUETCOLLECTIVE_EAS_PROJECT_ID;
const googleUrlScheme = process.env.RACQUETCOLLECTIVE_GOOGLE_IOS_URL_SCHEME;

export default ({ config }: ConfigContext): ExpoConfig => ({
  ...config,
  name: process.env.RACQUETCOLLECTIVE_APP_NAME ?? "Racquet Collective (dev)",
  slug: "racquetcollective",
  version: "0.1.0",
  scheme: process.env.RACQUETCOLLECTIVE_SCHEME ?? "racquetcollective",
  ...(process.env.RACQUETCOLLECTIVE_ICON ? { icon: process.env.RACQUETCOLLECTIVE_ICON } : {}),
  orientation: "portrait",
  // JS updates reach builds of the same app version (EAS Update, spec §16).
  runtimeVersion: { policy: "appVersion" },
  ...(projectId ? { updates: { url: `https://u.expo.dev/${projectId}` } } : {}),
  // Follows the device; a player can pin light or dark in Profile (docs/frontend.md, Theming).
  // Android needs expo-system-ui installed for this to apply.
  userInterfaceStyle: "automatic",
  ios: {
    bundleIdentifier: process.env.RACQUETCOLLECTIVE_BUNDLE_ID ?? "app.racquetcollective.dev",
    supportsTablet: true,
    usesAppleSignIn: true,
    // Only HTTPS and the OS's own crypto, which are exempt: App Store Connect then skips the
    // export-compliance question on every upload (docs/release.md).
    infoPlist: { ITSAppUsesNonExemptEncryption: false },
    ...(webHost ? { associatedDomains: [`applinks:${webHost}`, `webcredentials:${webHost}`] } : {}),
  },
  android: {
    package: process.env.RACQUETCOLLECTIVE_BUNDLE_ID ?? "app.racquetcollective.dev",
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
    // The dark splash matches the base a derived dark theme starts from (src/theme/theme.ts).
    ["expo-splash-screen", { backgroundColor: "#ffffff", dark: { backgroundColor: "#0f1216" } }],
    ["expo-notifications", { defaultChannel: "default" }],
    "expo-apple-authentication",
    ...(googleUrlScheme
      ? [
          ["@react-native-google-signin/google-signin", { iosUrlScheme: googleUrlScheme }] as [
            string,
            object,
          ],
        ]
      : []),
  ],
  extra: {
    eas: { projectId },
  },
  experiments: {
    typedRoutes: true,
    reactCompiler: true,
  },
});
