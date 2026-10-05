import { createContext, use, useEffect, useMemo, useState, type ReactNode } from "react";
import { useColorScheme } from "react-native";

import { resolveScheme, type AppearancePreference } from "./appearance";
import { appearanceStore } from "./appearanceStore";
import type { ColorScheme } from "./theme";

interface AppearanceValue {
  /** What the player chose on this device. */
  preference: AppearancePreference;
  /** What to draw in now. */
  scheme: ColorScheme;
  /** Changes the choice, applies it at once and remembers it on this device. */
  setPreference: (preference: AppearancePreference) => void;
}

const AppearanceContext = createContext<AppearanceValue | null>(null);

/**
 * Light or dark, per device: the system's scheme unless the player picked one (decision 102).
 * Native reads the stored choice asynchronously and renders nothing until it has it; the splash
 * screen is still up then, so the first drawn frame is already in the right scheme.
 */
export function AppearanceProvider({ children }: { children: ReactNode }) {
  const system = useColorScheme();
  const [preference, setStored] = useState(appearanceStore.initial);

  useEffect(() => {
    let live = true;
    void appearanceStore.load().then((loaded) => {
      if (!live) return;
      appearanceStore.apply(loaded);
      setStored(loaded);
    });
    return () => {
      live = false;
    };
  }, []);

  const value = useMemo<AppearanceValue | null>(() => {
    if (!preference) return null;
    return {
      preference,
      scheme: resolveScheme(preference, system),
      setPreference: (next) => {
        // Applied before the state change, so an OS override from an earlier choice never
        // answers `useColorScheme()` for a render that already follows the system.
        appearanceStore.apply(next);
        setStored(next);
        void appearanceStore.save(next);
      },
    };
  }, [preference, system]);

  if (!value) return null;
  return <AppearanceContext value={value}>{children}</AppearanceContext>;
}

/** The device's appearance and a way to change it. */
export function useAppearance(): AppearanceValue {
  const appearance = use(AppearanceContext);
  if (!appearance) throw new Error("useAppearance outside AppearanceProvider");
  return appearance;
}
