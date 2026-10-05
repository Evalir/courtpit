import { contrast, parseColor } from "./color";
import { defaultBrandColors, themeFromBranding } from "./theme";

const rgb = (value: string) => {
  const parsed = parseColor(value);
  if (!parsed) throw new Error(`unparsable ${value}`);
  return parsed;
};

// The demo community's branding, as `racquetcollective-server seed` writes it.
const riverside = {
  display_name: "Riverside Tennis Club",
  logo_url: "https://example.com/demo/riverside-logo.png",
  colors: {
    primary: "#0b6e4f",
    secondary: "#f4b942",
    background: "#f7f9f4",
    surface: "#ffffff",
    text: "#1b2a22",
  },
  typography: "inter",
  feature_flags: { doubles: true, mixed_doubles: true, match_requests: true },
};

describe("themeFromBranding", () => {
  it("uses the community's colors as given when they are legible", () => {
    const { colors, typography } = themeFromBranding(riverside);
    expect(colors.primary).toBe("#0b6e4f");
    expect(colors.primaryText).toBe("#0b6e4f");
    expect(colors.accent).toBe("#f4b942");
    expect(colors.background).toBe("#f7f9f4");
    expect(colors.surface).toBe("#ffffff");
    expect(colors.text).toBe("#1b2a22");
    expect(colors.onPrimary).toBe("#ffffff");
    expect(typography).toBe("inter");
  });

  it("falls back to Racquet Collective's defaults for missing or malformed colors", () => {
    const { colors, typography } = themeFromBranding({ colors: { primary: "not a color" } });
    expect(colors.primary).toBe(defaultBrandColors.primary);
    expect(colors.background).toBe(defaultBrandColors.background);
    expect(typography).toBe("system");
    expect(themeFromBranding(undefined).colors.primary).toBe(defaultBrandColors.primary);
  });

  it("ignores typography keys the app does not ship", () => {
    expect(themeFromBranding({ typography: "comic-sans" }).typography).toBe("system");
  });

  it.each([
    ["a pale primary", { primary: "#ffe14d" }],
    ["a dark background", { background: "#111111", surface: "#1d1d1d", text: "#f0f0f0" }],
    ["unreadable text", { text: "#eeeeee" }],
    ["a mid-grey everything", { primary: "#888888", secondary: "#888888", text: "#999999" }],
  ])("keeps text readable with %s", (_name, colors) => {
    const theme = themeFromBranding({ colors }).colors;
    const surface = rgb(theme.surface);
    expect(contrast(rgb(theme.text), surface)).toBeGreaterThanOrEqual(7);
    expect(contrast(rgb(theme.textMuted), surface)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(rgb(theme.primaryText), rgb(theme.background))).toBeGreaterThanOrEqual(4.5);
    expect(contrast(rgb(theme.onPrimary), rgb(theme.primary))).toBeGreaterThanOrEqual(3);
    expect(contrast(rgb(theme.danger), surface)).toBeGreaterThanOrEqual(4.5);
  });
});

describe("themeFromBranding in dark mode", () => {
  it("derives a dark theme from a light brand, keeping its colors", () => {
    const light = themeFromBranding(riverside, "light");
    const dark = themeFromBranding(riverside, "dark");
    expect(light.dark).toBe(false);
    expect(dark.dark).toBe(true);
    expect(dark.colors.background).not.toBe(light.colors.background);
    expect(contrast(rgb(dark.colors.background), rgb("#000000"))).toBeLessThan(1.3);
    // The page color becomes the ink, and the accent stays the brand's.
    expect(dark.colors.text).toBe("#f7f9f4");
    expect(dark.colors.accent).toBe("#f4b942");
    expect(dark.typography).toBe("inter");
  });

  it("uses a brand that is dark already as it is, in both schemes", () => {
    const branding = { colors: { background: "#111111", surface: "#1d1d1d", text: "#f0f0f0" } };
    expect(themeFromBranding(branding, "dark")).toEqual(themeFromBranding(branding, "light"));
    expect(themeFromBranding(branding, "light").dark).toBe(true);
  });

  it("gives a dark brand's accent readable text", () => {
    const branding = {
      colors: { background: "#111111", surface: "#1d1d1d", text: "#f0f0f0", secondary: "#d9f24a" },
    };
    const { colors } = themeFromBranding(branding, "light");
    expect(contrast(rgb(colors.onAccent), rgb(colors.accent))).toBeGreaterThanOrEqual(4.5);
  });

  it.each([
    ["Racquet Collective's defaults", undefined],
    ["the demo club", riverside],
    ["a navy primary", { colors: { primary: "#0b1f4d", secondary: "#e63946" } }],
    ["a pale primary", { colors: { primary: "#ffe14d" } }],
    ["a near-black primary", { colors: { primary: "#111111" } }],
    ["unreadable text", { colors: { text: "#eeeeee" } }],
    [
      "a mid-grey everything",
      { colors: { primary: "#888888", secondary: "#888888", text: "#999999" } },
    ],
  ])("keeps %s readable", (_name, branding) => {
    const colors = themeFromBranding(branding, "dark").colors;
    const surface = rgb(colors.surface);
    const ratio = (one: string, other: string) => contrast(rgb(one), rgb(other));
    expect(ratio(colors.text, colors.surface)).toBeGreaterThanOrEqual(7);
    expect(ratio(colors.textMuted, colors.surface)).toBeGreaterThanOrEqual(4.5);
    expect(ratio(colors.textMuted, colors.surfaceMuted)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(rgb(colors.primary), surface)).toBeGreaterThanOrEqual(3);
    expect(ratio(colors.onPrimary, colors.primary)).toBeGreaterThanOrEqual(3);
    expect(ratio(colors.onAccent, colors.accent)).toBeGreaterThanOrEqual(4.5);
    // Tone text on the page, on cards, and on its own badge fill.
    const tones = [
      [colors.primaryText, colors.primarySoft],
      [colors.success, colors.successSoft],
      [colors.warning, colors.warningSoft],
      [colors.danger, colors.dangerSoft],
    ] as const;
    for (const [text, fill] of tones) {
      for (const behind of [colors.background, colors.surface, fill]) {
        expect(ratio(text, behind)).toBeGreaterThanOrEqual(4.5);
      }
    }
  });
});
