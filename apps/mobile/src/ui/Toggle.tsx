import { Switch, View } from "react-native";

import { useTheme } from "@/theme/ThemeProvider";
import { space } from "@/theme/tokens";

import { Text } from "./Text";

/** An on/off setting with its label (and an optional line under it). */
export function Toggle({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint?: string;
  value: boolean;
  onChange: (value: boolean) => void;
}) {
  const { colors } = useTheme();
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: space.md, minHeight: 48 }}>
      <View style={{ flex: 1, gap: space.xxs }}>
        <Text variant="label">{label}</Text>
        {hint ? (
          <Text variant="caption" tone="textMuted">
            {hint}
          </Text>
        ) : null}
      </View>
      <Switch
        accessibilityLabel={label}
        value={value}
        onValueChange={onChange}
        trackColor={{ false: colors.border, true: colors.primary }}
        thumbColor={colors.surface}
        // react-native-web's own prop for the thumb when on (its default is teal).
        {...{ activeThumbColor: colors.surface }}
      />
    </View>
  );
}
