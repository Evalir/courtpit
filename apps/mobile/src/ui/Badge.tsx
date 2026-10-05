import { View } from "react-native";

import { useTheme } from "@/theme/ThemeProvider";
import type { Palette } from "@/theme/theme";
import { radius, space } from "@/theme/tokens";

import { Text, type TextTone } from "./Text";

/** Badge colors: a soft fill with matching text. */
export type BadgeTone = "neutral" | "primary" | "accent" | "success" | "warning" | "danger";

const fills: Record<BadgeTone, [keyof Palette, TextTone]> = {
  neutral: ["surfaceMuted", "textMuted"],
  primary: ["primarySoft", "primaryText"],
  accent: ["accent", "onAccent"],
  success: ["successSoft", "success"],
  warning: ["warningSoft", "warning"],
  danger: ["dangerSoft", "danger"],
};

/** A short status label. */
export function Badge({ label, tone = "neutral" }: { label: string; tone?: BadgeTone }) {
  const { colors } = useTheme();
  const [fill, text] = fills[tone];
  return (
    <View
      style={{
        backgroundColor: colors[fill],
        borderRadius: radius.pill,
        paddingHorizontal: space.sm,
        paddingVertical: space.xxs,
        alignSelf: "flex-start",
      }}
    >
      <Text variant="caption" weight="semibold" tone={text}>
        {label}
      </Text>
    </View>
  );
}
