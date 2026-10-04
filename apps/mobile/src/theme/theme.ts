import type { components } from "@courtpit/api-client";

import {
  BLACK,
  WHITE,
  bestOn,
  luminance,
  mix,
  parseColor,
  toHex,
  withContrast,
  type Rgba,
} from "./color";

type TenantInfo = components["schemas"]["TenantInfo"];

/**
 * Every color the UI uses. Communities brand five of them (`primary`, `secondary`, `background`,
 * `surface`, `text`); the rest are derived so that any branding stays legible.
 */
export interface Palette {
  /** Brand fill: primary buttons, the active tab, selected controls. */
  primary: string;
  /** Text and icons on `primary`. */
  onPrimary: string;
  /** `primary` as text or an icon on the background, darkened or lightened until it reads. */
  primaryText: string;
  /** A light tint of `primary` for selected rows, chips and the user's own line in a table. */
  primarySoft: string;
  /** The community's second color: highlights and badges. */
  accent: string;
  /** Text on `accent`. */
  onAccent: string;
  /** Screen background. */
  background: string;
  /** Cards, sheets and the tab bar. */
  surface: string;
  /** Inputs and control tracks: a shade off `surface`. */
  surfaceMuted: string;
  /** Body text. */
  text: string;
  /** Secondary text (still WCAG AA on `surface`). */
  textMuted: string;
  /** Hairlines and input outlines. */
  border: string;
  success: string;
  successSoft: string;
  warning: string;
  warningSoft: string;
  danger: string;
  dangerSoft: string;
}

/** Typefaces the app ships; `branding.typography` picks one by key. */
export type Typography = "system" | "inter";

/** What the client applies from `GET /api/v1/tenant`. */
export interface Theme {
  colors: Palette;
  typography: Typography;
  /** The surface is dark: light status-bar content. */
  dark: boolean;
}

/** Courtpit's own look, used for anything a community's branding leaves out. */
export const defaultBrandColors = {
  primary: "#2456c9",
  secondary: "#d9f24a",
  background: "#f4f6f9",
  surface: "#ffffff",
  text: "#131a26",
} as const;

const semantic = { success: "#1b7f4c", warning: "#a95b0c", danger: "#c3352e" } as const;

type BrandKey = keyof typeof defaultBrandColors;

/**
 * Builds the theme for a community; missing, unknown or malformed values fall back to the
 * defaults (the server fills absent branding keys with defaults too, but stays lenient).
 */
export function themeFromBranding(branding: Partial<TenantInfo["branding"]> | undefined): Theme {
  const brand = (key: BrandKey): Rgba => {
    const supplied = branding?.colors?.[key];
    return (supplied && parseColor(supplied)) || (parseColor(defaultBrandColors[key]) as Rgba);
  };
  const background = brand("background");
  const surface = brand("surface");
  const text = withContrast(brand("text"), surface, 7);
  const primary = brand("primary");
  const accent = brand("secondary");
  const tint = (color: Rgba, amount: number) => toHex(mix(surface, color, amount));
  const onSurface = (color: Rgba) => toHex(withContrast(color, surface, 4.5));
  const solid = (hex: string) => parseColor(hex) as Rgba;

  return {
    colors: {
      primary: toHex(primary),
      onPrimary: toHex(bestOn(primary, [WHITE, BLACK])),
      primaryText: toHex(withContrast(primary, background, 4.5)),
      primarySoft: tint(primary, 0.12),
      accent: toHex(accent),
      onAccent: toHex(bestOn(accent, [WHITE, text])),
      background: toHex(background),
      surface: toHex(surface),
      surfaceMuted: tint(text, 0.05),
      text: toHex(text),
      textMuted: onSurface(mix(text, surface, 0.4)),
      border: toHex(mix(background, text, 0.13)),
      success: onSurface(solid(semantic.success)),
      successSoft: tint(solid(semantic.success), 0.12),
      warning: onSurface(solid(semantic.warning)),
      warningSoft: tint(solid(semantic.warning), 0.14),
      danger: onSurface(solid(semantic.danger)),
      dangerSoft: tint(solid(semantic.danger), 0.1),
    },
    typography: branding?.typography === "inter" ? "inter" : "system",
    dark: luminance(surface) < 0.4,
  };
}
