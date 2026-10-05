import type { components } from "@racquetcollective/api-client";
import { TextInput, View } from "react-native";

import { fontFamilies } from "@/theme/fonts";
import { createStyles, useTheme } from "@/theme/ThemeProvider";
import { radius, space } from "@/theme/tokens";
import { Text } from "@/ui/Text";

import type { SetKind } from "./score";
import type { RowInput, ScoreRow } from "./scoreForm";

type MatchFormat = components["schemas"]["MatchFormat"];

const rowTitle: Record<SetKind, (index: number) => string> = {
  set: (index) => `Set ${index + 1}`,
  pro_set: () => "Pro set to 8",
  match_tiebreak: () => "Match tiebreak (points)",
};

/** One row per set, a games box for each side; rows come and go with `scoreRows`. */
export function ScoreForm({
  format,
  rows,
  sides,
  first,
  onChange,
}: {
  format: MatchFormat;
  rows: readonly ScoreRow[];
  /** Display names of each side. */
  sides: Record<"a" | "b", string>;
  /** The side shown in the first column: the viewer's. */
  first: "a" | "b";
  onChange: (index: number, value: RowInput) => void;
}) {
  const columns: ("a" | "b")[] = first === "a" ? ["a", "b"] : ["b", "a"];
  const styles = useStyles();
  const { colors, typography } = useTheme();
  const box = (index: number, side: "a" | "b", label: string) => {
    const row = rows[index];
    if (!row) return null;
    return (
      <TextInput
        accessibilityLabel={`${rowTitle[row.kind](index)}, ${label}`}
        value={row[side]}
        onChangeText={(text) =>
          onChange(index, { ...row, [side]: text.replace(/\D/g, "").slice(0, 2) })
        }
        inputMode="numeric"
        maxLength={2}
        selectTextOnFocus
        placeholder="–"
        placeholderTextColor={colors.textMuted}
        style={[styles.box, { fontFamily: fontFamilies[typography].semibold }]}
      />
    );
  };
  return (
    <View style={styles.table}>
      <View style={styles.row}>
        <View style={styles.title} />
        {columns.map((side) => (
          <Text key={side} variant="caption" tone="textMuted" numberOfLines={2} style={styles.side}>
            {sides[side]}
          </Text>
        ))}
      </View>
      {rows.map((row, index) => (
        <View key={index} style={styles.row}>
          <Text variant="label" weight="semibold" style={styles.title}>
            {rowTitle[row.kind](index)}
          </Text>
          {columns.map((side) => (
            <View key={side} style={styles.side}>
              {box(index, side, sides[side])}
            </View>
          ))}
        </View>
      ))}
      <Text variant="caption" tone="textMuted">
        {format.tiebreak_at != null
          ? `Sets to ${format.games_per_set} games, tiebreak at ${format.tiebreak_at}–${format.tiebreak_at} (enter 7–6).`
          : `Advantage sets to ${format.games_per_set} games, won by two.`}
      </Text>
    </View>
  );
}

const useStyles = createStyles(({ colors }) => ({
  table: { gap: space.md },
  row: { flexDirection: "row", alignItems: "center", gap: space.md },
  title: { flex: 1 },
  side: { width: 88, alignItems: "center", textAlign: "center" },
  box: {
    width: 64,
    height: 56,
    borderRadius: radius.md,
    borderWidth: 1,
    borderColor: colors.border,
    backgroundColor: colors.surface,
    color: colors.text,
    fontSize: 24,
    textAlign: "center",
  },
}));
