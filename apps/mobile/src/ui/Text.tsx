import { Text as NativeText, type TextProps as NativeTextProps } from "react-native";

import { fontFamilies, fontWeights } from "@/theme/fonts";
import { useTheme } from "@/theme/ThemeProvider";
import type { Palette } from "@/theme/theme";
import { typeScale, type FontWeight, type TextVariant } from "@/theme/tokens";

/** Palette entries that work as text colors. */
export type TextTone = keyof Pick<
  Palette,
  "text" | "textMuted" | "primaryText" | "onPrimary" | "onAccent" | "success" | "warning" | "danger"
>;

export interface TextProps extends NativeTextProps {
  /** Size, line height and default weight. */
  variant?: TextVariant;
  /** Color from the palette. */
  tone?: TextTone;
  /** Overrides the variant's weight. */
  weight?: FontWeight;
  align?: "left" | "center" | "right";
}

/** Themed text: the community's typeface and colors, the app's type scale. */
export function Text({
  variant = "body",
  tone = "text",
  weight,
  align,
  style,
  ...props
}: TextProps) {
  const { colors, typography } = useTheme();
  const scale = typeScale[variant];
  const resolved = weight ?? scale.weight;
  const fontFamily = fontFamilies[typography][resolved];
  return (
    <NativeText
      {...props}
      style={[
        {
          color: colors[tone],
          fontSize: scale.fontSize,
          lineHeight: scale.lineHeight,
          textAlign: align,
          ...(fontFamily ? { fontFamily } : { fontWeight: fontWeights[resolved] }),
          ...(variant === "overline" ? { textTransform: "uppercase", letterSpacing: 0.6 } : null),
        },
        style,
      ]}
    />
  );
}
