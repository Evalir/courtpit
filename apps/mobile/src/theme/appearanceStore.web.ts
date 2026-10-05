import { parsePreference, type AppearancePreference } from "./appearance";

const KEY = "courtpit.appearance";

/**
 * Browser: the choice is kept in `localStorage` (readable synchronously, so the first frame is
 * already right), and the page's CSS `color-scheme` follows it so scrollbars and native form
 * controls match. Storage can be unavailable (private windows, blocked site data): the app then
 * follows the system and forgets the choice on reload.
 */
export const appearanceStore = {
  initial(): AppearancePreference | null {
    try {
      return parsePreference(globalThis.localStorage?.getItem(KEY));
    } catch {
      return "system";
    }
  },
  load: async (): Promise<AppearancePreference> => appearanceStore.initial() ?? "system",
  async save(preference: AppearancePreference): Promise<void> {
    try {
      globalThis.localStorage?.setItem(KEY, preference);
    } catch {
      // Not persisted; this session still uses it.
    }
  },
  apply(preference: AppearancePreference): void {
    if (typeof document === "undefined") return;
    document.documentElement.style.colorScheme =
      preference === "system" ? "light dark" : preference;
  },
};
