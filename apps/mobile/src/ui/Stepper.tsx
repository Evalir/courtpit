import { View } from "react-native";

import { space } from "@/theme/tokens";

import { Button } from "./Button";
import { Text } from "./Text";

/** A small whole number with − and + (box sizes, counts). */
export function Stepper({
  label,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}) {
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: space.sm }}>
      <Text variant="label" style={{ flex: 1 }}>
        {label}
      </Text>
      <Button
        label="−"
        variant="secondary"
        size="sm"
        disabled={value <= min}
        onPress={() => onChange(value - 1)}
      />
      <Text
        variant="subheading"
        accessibilityLabel={`${label}: ${value}`}
        style={{ minWidth: 28, textAlign: "center" }}
      >
        {value}
      </Text>
      <Button
        label="+"
        variant="secondary"
        size="sm"
        disabled={value >= max}
        onPress={() => onChange(value + 1)}
      />
    </View>
  );
}
