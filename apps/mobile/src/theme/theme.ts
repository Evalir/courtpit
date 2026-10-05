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

/** Light or dark: what the device (or the player's override) asks for. */
export type ColorScheme = "light" | "dark";

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

/** The neutral a derived dark theme starts from: a near-black, tinted with the brand. */
const darkBase = parseColor("#0f1216") as Rgba;

type BrandKey = keyof typeof defaultBrandColors;

/**
 * Builds the theme for a community; missing, unknown or malformed values fall back to the
 * defaults (the server fills absent branding keys with defaults too, but stays lenient).
 *
 * `dark` derives a dark variant from a light brand (decision 102): a near-black background
 * tinted with the primary, the brand's page color as ink, and the brand colors kept as fills
 * but lifted where they would vanish. A brand that is already dark is used as is in both
 * schemes, since its community chose it.
 */
export function themeFromBranding(
  branding: Partial<TenantInfo["branding"]> | undefined,
  scheme: ColorScheme = "light",
): Theme {
  const brand = (key: BrandKey): Rgba => {
    const supplied = branding?.colors?.[key];
    return (supplied && parseColor(supplied)) || (parseColor(defaultBrandColors[key]) as Rgba);
  };
  const primary = brand("primary");
  const brandIsDark = luminance(brand("surface")) < 0.4;
  const colors =
    scheme === "dark" && !brandIsDark
      ? derivedDarkPalette(primary, brand("secondary"), brand("background"))
      : brandPalette(
          primary,
          brand("secondary"),
          brand("background"),
          brand("surface"),
          brand("text"),
        );
  return {
    colors,
    typography: branding?.typography === "inter" ? "inter" : "system",
    dark: luminance(parseColor(colors.surface) as Rgba) < 0.4,
  };
}

/** The community's own palette, with text pushed to legibility. */
function brandPalette(
  primary: Rgba,
  accent: Rgba,
  background: Rgba,
  surface: Rgba,
  brandText: Rgba,
): Palette {
  const text = withContrast(brandText, surface, 7);
  const tint = (color: Rgba, amount: number) => toHex(mix(surface, color, amount));
  const onSurface = (color: Rgba) => toHex(withContrast(color, surface, 4.5));
  const solid = (hex: string) => parseColor(hex) as Rgba;
  return {
    primary: toHex(primary),
    onPrimary: toHex(bestOn(primary, [WHITE, BLACK])),
    primaryText: toHex(withContrast(primary, background, 4.5)),
    primarySoft: tint(primary, 0.12),
    accent: toHex(accent),
    // The background is a candidate for a brand that is dark already (its text is light).
    onAccent: toHex(bestOn(accent, [WHITE, text, background])),
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
  };
}

/**
 * A dark palette derived from a light brand. Surfaces sit a step above the background (dark
 * UIs show elevation by lightness, not shadow), so tone colors are checked against their soft
 * fills, the lightest thing they sit on; soft fills are stronger than in light mode so they
 * still read as color. The primary fill is lifted to 3:1 against the surface (WCAG non-text
 * contrast) because most brand primaries are mid or dark tones.
 */
function derivedDarkPalette(primary: Rgba, accent: Rgba, brandBackground: Rgba): Palette {
  const background = mix(darkBase, primary, 0.08);
  const surface = mix(background, WHITE, 0.06);
  const text = withContrast(brandBackground, surface, 7);
  const fill = withContrast(primary, surface, 3);
  const soft = (color: Rgba, amount: number) => mix(surface, color, amount);
  // A tone as text on its own soft fill, and so on the darker surface and background too.
  const tone = (color: Rgba, amount: number) => {
    const fillColor = soft(color, amount);
    const legible = withContrast(withContrast(color, fillColor, 4.5), surface, 4.5);
    return [toHex(legible), toHex(fillColor)] as const;
  };
  const solid = (hex: string) => parseColor(hex) as Rgba;
  const [primaryText, primarySoft] = tone(primary, 0.24);
  const [success, successSoft] = tone(solid(semantic.success), 0.22);
  const [warning, warningSoft] = tone(solid(semantic.warning), 0.22);
  const [danger, dangerSoft] = tone(solid(semantic.danger), 0.22);
  const surfaceMuted = soft(text, 0.08);
  return {
    primary: toHex(fill),
    onPrimary: toHex(bestOn(fill, [WHITE, BLACK])),
    primaryText,
    primarySoft,
    accent: toHex(accent),
    onAccent: toHex(bestOn(accent, [WHITE, BLACK])),
    background: toHex(background),
    surface: toHex(surface),
    surfaceMuted: toHex(surfaceMuted),
    text: toHex(text),
    textMuted: toHex(withContrast(mix(text, surface, 0.4), surfaceMuted, 4.5)),
    border: toHex(mix(background, text, 0.16)),
    success,
    successSoft,
    warning,
    warningSoft,
    danger,
    dangerSoft,
  };
}
