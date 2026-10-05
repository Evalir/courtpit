import * as SecureStore from "expo-secure-store";
import { Appearance } from "react-native";

import { parsePreference, type AppearancePreference } from "./appearance";

const KEY = "racquetcollective.appearance";

/**
 * Native: the choice is kept in SecureStore (already a dependency; the value is not secret)
 * and also handed to the OS, so system UI the app shows (alerts, the keyboard, share sheets)
 * matches. `appearanceStore.web.ts` is the browser twin.
 */
export const appearanceStore = {
  /** Native storage is asynchronous: nothing is known before `load`. */
  initial: (): AppearancePreference | null => null,
  async load(): Promise<AppearancePreference> {
    try {
      return parsePreference(await SecureStore.getItemAsync(KEY));
    } catch {
      return "system";
    }
  },
  async save(preference: AppearancePreference): Promise<void> {
    try {
      await SecureStore.setItemAsync(KEY, preference);
    } catch (error) {
      console.warn("could not save the appearance", error);
    }
  },
  apply(preference: AppearancePreference): void {
    Appearance.setColorScheme(preference === "system" ? "unspecified" : preference);
  },
};
