/**
 * Color math for runtime theming. Communities supply CSS color strings in their branding; the
 * app derives the rest of the palette from them and keeps text readable whatever they chose.
 */

/** An sRGB color with 0–255 channels and 0–1 alpha. */
export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

const HEX = /^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const RGB_FN = /^rgba?\(\s*([^)]*)\)$/i;

/** Parses `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(..)` and `rgba(..)`; `null` otherwise. */
export function parseColor(input: string): Rgba | null {
  const value = input.trim();
  const hex = HEX.exec(value)?.[1];
  if (hex) {
    const digits =
      hex.length <= 4 ? [...hex].map((digit) => digit + digit) : (hex.match(/../g) ?? []);
    const [r = 0, g = 0, b = 0, a = 255] = digits.map((pair) => parseInt(pair, 16));
    return { r, g, b, a: a / 255 };
  }
  const args = RGB_FN.exec(value)?.[1];
  if (args) {
    const parts = args.split(/[\s,/]+/).filter(Boolean);
    if (parts.length < 3 || parts.length > 4) return null;
    const channels = parts
      .slice(0, 3)
      .map((part) => (part.endsWith("%") ? (parseFloat(part) / 100) * 255 : parseFloat(part)));
    const alphaPart = parts[3];
    const alpha =
      alphaPart === undefined
        ? 1
        : alphaPart.endsWith("%")
          ? parseFloat(alphaPart) / 100
          : parseFloat(alphaPart);
    if ([...channels, alpha].some(Number.isNaN)) return null;
    const [r = 0, g = 0, b = 0] = channels.map((channel) => clamp(channel, 0, 255));
    return { r, g, b, a: clamp(alpha, 0, 1) };
  }
  return null;
}

/** `#rrggbb`, or `#rrggbbaa` when not opaque. */
export function toHex({ r, g, b, a }: Rgba): string {
  const pair = (channel: number) => Math.round(channel).toString(16).padStart(2, "0");
  return `#${pair(r)}${pair(g)}${pair(b)}${a < 1 ? pair(a * 255) : ""}`;
}

/** Linear blend: `amount` 0 gives `from`, 1 gives `to`. */
export function mix(from: Rgba, to: Rgba, amount: number): Rgba {
  const lerp = (start: number, end: number) => start + (end - start) * amount;
  return {
    r: lerp(from.r, to.r),
    g: lerp(from.g, to.g),
    b: lerp(from.b, to.b),
    a: lerp(from.a, to.a),
  };
}

/** WCAG relative luminance of an opaque color. */
export function luminance({ r, g, b }: Rgba): number {
  const linear = (channel: number) => {
    const c = channel / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
}

/** WCAG contrast ratio, 1–21. */
export function contrast(one: Rgba, other: Rgba): number {
  const [light, dark] = [luminance(one), luminance(other)].sort((x, y) => y - x) as [
    number,
    number,
  ];
  return (light + 0.05) / (dark + 0.05);
}

/** Whichever of the candidates reads best on `background`. */
export function bestOn(background: Rgba, candidates: readonly Rgba[]): Rgba {
  return candidates.reduce((best, candidate) =>
    contrast(candidate, background) > contrast(best, background) ? candidate : best,
  );
}

/**
 * `color` itself if it reaches `ratio` against `background`, else the least step towards black
 * or white (whichever direction gets there) that does. Keeps a brand color recognisable while
 * making it legible as text.
 */
export function withContrast(color: Rgba, background: Rgba, ratio: number): Rgba {
  if (contrast(color, background) >= ratio) return color;
  const target = luminance(background) > 0.5 ? BLACK : WHITE;
  for (let step = 1; step <= 20; step += 1) {
    const candidate = mix(color, target, step / 20);
    if (contrast(candidate, background) >= ratio) return candidate;
  }
  return target;
}

export const BLACK: Rgba = { r: 0, g: 0, b: 0, a: 1 };
export const WHITE: Rgba = { r: 255, g: 255, b: 255, a: 1 };

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}
