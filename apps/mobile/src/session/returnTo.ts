import * as Linking from "expo-linking";
import { Platform } from "react-native";

/** Paths that are no destination after signing in. */
const NOT_DESTINATIONS = new Set(["/", "/sign-in", "/verify", "/profile/delete"]);

/**
 * The in-app path (and query) a link points at, if signing in should end there. Takes
 * `https://club.example/leagues/1?x=1`, `racquetcollective://leagues/1`, Expo Go's
 * `exp://host:8081/--/leagues/1` or a bare path. React Native's `URL` lacks `pathname`, hence
 * plain string work.
 */
export function returnPathOf(url: string | null | undefined): string | null {
  if (!url) return null;
  let path = url.replace(/#.*$/, "");
  const expoGo = path.indexOf("/--/");
  if (expoGo >= 0) {
    path = path.slice(expoGo + 3);
  } else {
    const parts = /^([a-z][a-z0-9+.-]*):\/\/([^/?]*)(.*)$/i.exec(path);
    if (parts) {
      const [, scheme = "", host = "", rest = ""] = parts;
      // A custom scheme's "host" is the first path segment: racquetcollective://leagues/1.
      path = /^https?$/i.test(scheme) ? rest : `/${`${host}${rest}`.replace(/^\/+/, "")}`;
    }
  }
  const [pathname = "", query] = path.split("?", 2);
  const clean = pathname.replace(/\/+$/, "") || "/";
  if (!clean.startsWith("/") || clean.startsWith("//") || NOT_DESTINATIONS.has(clean)) {
    return null;
  }
  return query ? `${clean}?${query}` : clean;
}

let pending: string | null = null;
let initialTaken = false;

// On web the router replaces a guarded address with /sign-in, so read it before it renders.
const webInitialUrl =
  Platform.OS === "web" && typeof window !== "undefined" ? window.location.href : null;

/** Where to go once signed in; the latest link wins. */
export const returnTo = {
  remember(url: string | null | undefined): void {
    const path = returnPathOf(url);
    if (path) pending = path;
  },
  take(): string | null {
    const path = pending;
    pending = null;
    return path;
  },
  /** The link that opened the app, the first time it is asked for. */
  async initialUrl(): Promise<string | null> {
    if (initialTaken) return null;
    initialTaken = true;
    return webInitialUrl ?? (await Linking.getInitialURL());
  },
};
