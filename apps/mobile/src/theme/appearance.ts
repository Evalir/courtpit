import type { ColorScheme } from "./theme";

/** The player's choice on this device: follow the system, or always light or always dark. */
export type AppearancePreference = "system" | ColorScheme;

/** The choices, in the order the picker shows them. */
export const appearanceOptions = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
] as const satisfies readonly { value: AppearancePreference; label: string }[];

/** A stored choice; anything unknown (nothing stored, a newer version's value) follows the system. */
export function parsePreference(stored: string | null | undefined): AppearancePreference {
  return stored === "light" || stored === "dark" ? stored : "system";
}

/**
 * The scheme to draw in: the player's choice, or the system's when following it. A system that
 * reports nothing (`useColorScheme()` is `null` on some platforms) gets light.
 */
export function resolveScheme(
  preference: AppearancePreference,
  system: string | null | undefined,
): ColorScheme {
  if (preference !== "system") return preference;
  return system === "dark" ? "dark" : "light";
}
