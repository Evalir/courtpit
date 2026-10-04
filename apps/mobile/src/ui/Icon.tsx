import Ionicons from "@expo/vector-icons/Ionicons";
import type { ComponentProps } from "react";

import { useTheme } from "@/theme/ThemeProvider";
import type { Palette } from "@/theme/theme";

/** An Ionicons glyph name. */
export type IconName = ComponentProps<typeof Ionicons>["name"];

/** An Ionicons glyph in a palette color (body text by default). */
export function Icon({
  name,
  size = 20,
  tone = "text",
}: {
  name: IconName;
  size?: number;
  tone?: keyof Palette;
}) {
  const { colors } = useTheme();
  return <Ionicons name={name} size={size} color={colors[tone]} />;
}
