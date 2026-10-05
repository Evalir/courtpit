/** Layout tokens shared by every community; only colors and the typeface are branded. */

export const space = {
  xxs: 2,
  xs: 4,
  sm: 8,
  md: 12,
  lg: 16,
  xl: 24,
  xxl: 32,
  xxxl: 48,
} as const;

export const radius = {
  sm: 6,
  md: 10,
  lg: 16,
  pill: 999,
} as const;

export type FontWeight = "regular" | "medium" | "semibold" | "bold";

/** Text styles: size and line height in points, and weight. */
export const typeScale = {
  display: { fontSize: 32, lineHeight: 38, weight: "bold" },
  title: { fontSize: 24, lineHeight: 30, weight: "bold" },
  heading: { fontSize: 19, lineHeight: 25, weight: "semibold" },
  subheading: { fontSize: 16, lineHeight: 22, weight: "semibold" },
  body: { fontSize: 16, lineHeight: 22, weight: "regular" },
  label: { fontSize: 14, lineHeight: 20, weight: "medium" },
  caption: { fontSize: 12, lineHeight: 16, weight: "regular" },
  overline: { fontSize: 12, lineHeight: 16, weight: "semibold" },
} as const satisfies Record<string, { fontSize: number; lineHeight: number; weight: FontWeight }>;

export type TextVariant = keyof typeof typeScale;

/** Content column width on wide screens (web on a desktop, tablets). */
export const maxContentWidth = 720;
