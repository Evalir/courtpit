import { Pressable } from "react-native";

import { createStyles } from "@/theme/ThemeProvider";
import { radius, space } from "@/theme/tokens";

import { Text } from "./Text";

/** A selectable pill: day and time slots, suggestions, filters. */
export function Chip({
  label,
  selected = false,
  disabled = false,
  onPress,
  accessibilityLabel,
}: {
  label: string;
  selected?: boolean;
  disabled?: boolean;
  onPress: () => void;
  accessibilityLabel?: string;
}) {
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={accessibilityLabel ?? label}
      accessibilityState={{ selected, disabled }}
      disabled={disabled}
      onPress={onPress}
      style={({ pressed }) => [
        styles.chip,
        selected && styles.selected,
        disabled && styles.disabled,
        pressed && !selected && styles.pressed,
      ]}
    >
      <Text
        variant="label"
        weight={selected ? "semibold" : "medium"}
        tone={selected ? "onPrimary" : disabled ? "textMuted" : "text"}
      >
        {label}
      </Text>
    </Pressable>
  );
}

const useStyles = createStyles(({ colors }) => ({
  chip: {
    minHeight: 40,
    minWidth: 64,
    paddingHorizontal: space.md,
    borderRadius: radius.pill,
    borderWidth: 1,
    borderColor: colors.border,
    backgroundColor: colors.surface,
    alignItems: "center",
    justifyContent: "center",
  },
  selected: { backgroundColor: colors.primary, borderColor: colors.primary },
  disabled: { opacity: 0.45 },
  pressed: { backgroundColor: colors.surfaceMuted },
}));
