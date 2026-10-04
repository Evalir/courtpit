import { ActivityIndicator, Pressable, View, type PressableProps } from "react-native";

import { createStyles, useTheme } from "@/theme/ThemeProvider";
import { radius, space } from "@/theme/tokens";

import { Icon, type IconName } from "./Icon";
import { Text, type TextTone } from "./Text";

type Variant = "primary" | "secondary" | "ghost" | "danger";

export interface ButtonProps extends Omit<PressableProps, "children" | "style"> {
  label: string;
  variant?: Variant;
  size?: "md" | "sm";
  icon?: IconName;
  /** Shows a spinner and ignores presses. */
  loading?: boolean;
  /** Stretches to the container's width. */
  block?: boolean;
}

const labelTone: Record<Variant, TextTone> = {
  primary: "onPrimary",
  secondary: "primaryText",
  ghost: "primaryText",
  danger: "danger",
};

/** The app's one button. Primary for the screen's main action, secondary/ghost for the rest. */
export function Button({
  label,
  variant = "primary",
  size = "md",
  icon,
  loading = false,
  block = false,
  disabled,
  ...props
}: ButtonProps) {
  const styles = useStyles();
  const { colors } = useTheme();
  const inactive = disabled || loading;
  const tone = labelTone[variant];
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityState={{ disabled: !!inactive, busy: loading }}
      disabled={inactive}
      {...props}
      style={({ pressed }) => [
        styles.base,
        styles[variant],
        size === "sm" && styles.small,
        block && styles.block,
        pressed && styles.pressed,
        inactive && styles.inactive,
      ]}
    >
      {loading ? (
        <ActivityIndicator color={colors[tone]} />
      ) : (
        <View style={styles.content}>
          {icon ? <Icon name={icon} size={size === "sm" ? 16 : 18} tone={tone} /> : null}
          <Text variant="label" weight="semibold" tone={tone}>
            {label}
          </Text>
        </View>
      )}
    </Pressable>
  );
}

const useStyles = createStyles(({ colors }) => ({
  base: {
    minHeight: 48,
    paddingHorizontal: space.xl,
    borderRadius: radius.md,
    alignItems: "center",
    justifyContent: "center",
  },
  small: { minHeight: 36, paddingHorizontal: space.md },
  block: { alignSelf: "stretch" },
  content: { flexDirection: "row", alignItems: "center", gap: space.sm },
  primary: { backgroundColor: colors.primary },
  secondary: { backgroundColor: colors.primarySoft },
  ghost: { backgroundColor: "transparent" },
  danger: { backgroundColor: colors.dangerSoft },
  pressed: { opacity: 0.75 },
  inactive: { opacity: 0.5 },
}));
