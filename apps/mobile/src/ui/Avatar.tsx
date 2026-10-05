import { View } from "react-native";

import { Text } from "./Text";

// Muted fills that all carry white initials at AA; picked by a hash of the player id so a
// player keeps their color everywhere.
const fills = [
  "#5b5bd6",
  "#0d7d6c",
  "#b2477a",
  "#a35a00",
  "#3f6fb5",
  "#7a5bb2",
  "#2f7d32",
  "#b23b3b",
];

/** First letters of the first and last word: "Ana María Ruiz" → "AR". */
export function initials(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const first = words[0]?.[0] ?? "?";
  const last = words.length > 1 ? (words[words.length - 1]?.[0] ?? "") : "";
  return (first + last).toUpperCase();
}

function fillFor(id: string): string {
  let hash = 0;
  for (const char of id) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return fills[hash % fills.length] ?? "#5b5bd6";
}

/** Initials in a colored circle (v1 has no avatar uploads, spec §16). */
export function Avatar({ id, name, size = 40 }: { id: string; name: string; size?: number }) {
  return (
    <View
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
      style={{
        width: size,
        height: size,
        borderRadius: size / 2,
        backgroundColor: fillFor(id),
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <Text
        variant={size >= 56 ? "heading" : "label"}
        weight="semibold"
        style={{ color: "#ffffff" }}
      >
        {initials(name)}
      </Text>
    </View>
  );
}
