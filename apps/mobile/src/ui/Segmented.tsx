import { Pressable, View } from "react-native";

import { createStyles } from "@/theme/ThemeProvider";
import { radius, space } from "@/theme/tokens";

import { Text } from "./Text";

/** A row of mutually exclusive options (rankings discipline, list filters). */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
}: {
  options: readonly { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
}) {
  const styles = useStyles();
  return (
    <View style={styles.track} accessibilityRole="tablist">
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <Pressable
            key={option.value}
            accessibilityRole="tab"
            accessibilityState={{ selected }}
            onPress={() => onChange(option.value)}
            style={[styles.segment, selected && styles.selected]}
          >
            <Text variant="label" weight="semibold" tone={selected ? "text" : "textMuted"}>
              {option.label}
            </Text>
          </Pressable>
        );
      })}
    </View>
  );
}

const useStyles = createStyles(({ colors }) => ({
  track: {
    flexDirection: "row",
    backgroundColor: colors.surfaceMuted,
    borderRadius: radius.md,
    padding: space.xxs + 1,
  },
  segment: {
    flex: 1,
    minHeight: 36,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: radius.md - 2,
  },
  selected: {
    backgroundColor: colors.surface,
    boxShadow: "0 1px 3px rgba(0, 0, 0, 0.1)",
  },
}));
