import { loadAsync } from "expo-font";

import type { FontWeight } from "./tokens";
import type { Typography } from "./theme";

/**
 * Font family per weight. Native platforms pick a custom font's weight by family name rather
 * than `fontWeight`, so each weight is its own family; `undefined` means the system font with
 * a numeric `fontWeight`.
 */
export const fontFamilies: Record<Typography, Record<FontWeight, string | undefined>> = {
  system: { regular: undefined, medium: undefined, semibold: undefined, bold: undefined },
  inter: {
    regular: "Inter_400Regular",
    medium: "Inter_500Medium",
    semibold: "Inter_600SemiBold",
    bold: "Inter_700Bold",
  },
};

export const fontWeights: Record<FontWeight, "400" | "500" | "600" | "700"> = {
  regular: "400",
  medium: "500",
  semibold: "600",
  bold: "700",
};

/** Loads the files a typography needs; resolves at once for the system font. */
export async function loadTypography(typography: Typography): Promise<void> {
  if (typography !== "inter") return;
  // Only the four weights the UI uses; the package's index would pull in all eighteen files.
  await loadAsync({
    Inter_400Regular: require("@expo-google-fonts/inter/400Regular/Inter_400Regular.ttf"),
    Inter_500Medium: require("@expo-google-fonts/inter/500Medium/Inter_500Medium.ttf"),
    Inter_600SemiBold: require("@expo-google-fonts/inter/600SemiBold/Inter_600SemiBold.ttf"),
    Inter_700Bold: require("@expo-google-fonts/inter/700Bold/Inter_700Bold.ttf"),
  });
}
