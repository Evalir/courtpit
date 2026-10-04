import { contrast, parseColor } from "./color";
import { defaultBrandColors, themeFromBranding } from "./theme";

const rgb = (value: string) => {
  const parsed = parseColor(value);
  if (!parsed) throw new Error(`unparsable ${value}`);
  return parsed;
};

// The demo community's branding, as `courtpit-server seed` writes it.
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

  it("falls back to Courtpit's defaults for missing or malformed colors", () => {
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
