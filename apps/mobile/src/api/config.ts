import Constants from "expo-constants";
import { Platform } from "react-native";

/** Where the API is and which community this app talks to. */
export interface ApiConfig {
  /** API origin, no trailing slash. */
  baseUrl: string;
  /** Community slug for `X-RacquetCollective-Community`; absent on web on a community's own host. */
  community?: string;
  /** Browser build: the session is an httpOnly cookie, not a stored token. */
  web: boolean;
}

/**
 * Resolves the API configuration from the build's `EXPO_PUBLIC_*` variables:
 *
 * - **Native** bakes in `EXPO_PUBLIC_COMMUNITY` (each community's build profile sets it) and
 *   `EXPO_PUBLIC_API_URL`. In development the URL may be empty: the app then calls the dev
 *   server it was loaded from, which proxies `/api` to a local API (`metro.config.js`).
 * - **Web** is served by the API itself, so it calls its own origin. In production it sends no
 *   community header and the server picks the community from the host; in development
 *   `.env.development` names the community because `localhost` is not a community host.
 *
 * Throws when a native build was made without a community, which is a packaging mistake.
 */
export function resolveApiConfig(): ApiConfig {
  // `process.env.EXPO_PUBLIC_*` must be read literally: Expo inlines these at build time.
  const community = process.env.EXPO_PUBLIC_COMMUNITY || undefined;
  const apiUrl = process.env.EXPO_PUBLIC_API_URL || undefined;
  const web = Platform.OS === "web";
  if (web) {
    return { baseUrl: apiUrl ?? globalThis.location.origin, community, web };
  }
  if (!community) {
    throw new Error("This build has no community: set EXPO_PUBLIC_COMMUNITY in its build profile.");
  }
  const devServer = Constants.expoConfig?.hostUri;
  const baseUrl = apiUrl ?? (devServer ? `http://${devServer}` : undefined);
  if (!baseUrl) {
    throw new Error("This build has no API address: set EXPO_PUBLIC_API_URL in its build profile.");
  }
  return { baseUrl: baseUrl.replace(/\/+$/, ""), community, web };
}
